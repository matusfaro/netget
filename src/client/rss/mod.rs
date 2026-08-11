//! RSS (Really Simple Syndication) Feed Client
//!
//! Fetches and parses RSS 2.0 XML feeds with LLM-controlled interpretation.
//! The LLM decides which feeds to fetch and how to process items.

pub mod actions;

pub use actions::RssClientProtocol;

use crate::client::llm_budget::call_llm_for_client;
use crate::client::rss::actions::{RSS_CLIENT_CONNECTED_EVENT, RSS_CLIENT_FEED_FETCHED_EVENT};
use crate::llm::actions::client_trait::{Client, ClientActionResult};
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use anyhow::{Context, Result};
use rss::Channel;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};

/// Guard against a model that keeps chaining `fetch_rss_feed` forever.
const MAX_CHAINED_FETCHES: usize = 16;

struct ClientData {
    memory: String,
}

/// Turn whatever the model put in `url` into an absolute URL.
///
/// A bare path (`/news.xml`) is resolved against the address the client was opened on;
/// an absolute URL is used verbatim.
pub fn resolve_feed_url(base_url: &str, url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else if let Some(path) = url.strip_prefix('/') {
        format!("{}/{}", base_url.trim_end_matches('/'), path)
    } else {
        format!("{}/{}", base_url.trim_end_matches('/'), url)
    }
}

/// Convert a parsed RSS channel into the structured event payload the model sees.
///
/// Deliberately structured fields, never raw XML — models cannot reliably parse XML back out.
pub fn channel_to_event_data(url: &str, channel: &Channel) -> serde_json::Value {
    let items: Vec<_> = channel
        .items()
        .iter()
        .map(|item| {
            json!({
                "title": item.title(),
                "link": item.link(),
                "description": item.description(),
                "author": item.author(),
                "pub_date": item.pub_date(),
                "guid": item.guid().map(|g| g.value()),
            })
        })
        .collect();

    json!({
        "url": url,
        "feed_title": channel.title(),
        "feed_link": channel.link(),
        "feed_description": channel.description(),
        "item_count": channel.items().len(),
        "items": items,
    })
}

/// RSS client
pub struct RssClient;

impl RssClient {
    /// Connect to an RSS feed source and set up LLM integration.
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // `Client::connect` must return a concrete socket address, so resolve the host now.
        // This doubles as an early failure if the address is unusable.
        let resolved: SocketAddr = tokio::net::lookup_host(remote_addr.as_str())
            .await
            .with_context(|| format!("Failed to resolve RSS feed host: {}", remote_addr))?
            .next()
            .with_context(|| format!("No addresses for RSS feed host: {}", remote_addr))?;

        let base_url = format!("http://{}", remote_addr);

        info!(
            "RSS client {} targeting {} ({})",
            client_id, base_url, resolved
        );
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!(
            "[CLIENT] RSS client {} ready ({})",
            client_id, base_url
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        let client_data = Arc::new(Mutex::new(ClientData {
            memory: String::new(),
        }));
        let protocol = Arc::new(RssClientProtocol::new());
        let instruction = app_state
            .get_instruction_for_client(client_id)
            .await
            .unwrap_or_default();

        // Ask the model which feed to fetch, then service its answer in a background task.
        let event = Event::new(
            &RSS_CLIENT_CONNECTED_EVENT,
            json!({ "base_url": base_url.clone() }),
        );

        let memory_snapshot = client_data.lock().await.memory.clone();
        let first = call_llm_for_client(
            &llm_client,
            &app_state,
            client_id.to_string(),
            &instruction,
            &memory_snapshot,
            Some(&event),
            protocol.as_ref() as &dyn Client,
            &status_tx,
        )
        .await;

        let mut pending: Vec<String> = Vec::new();
        match first {
            Ok(ClientLlmResult {
                actions,
                memory_updates,
            }) => {
                if let Some(mem) = memory_updates {
                    client_data.lock().await.memory = mem;
                }
                for action in actions {
                    match protocol.execute_action(action) {
                        Ok(ClientActionResult::Custom { name, data })
                            if name == "fetch_rss_feed" =>
                        {
                            if let Some(u) = data.get("url").and_then(|v| v.as_str()) {
                                pending.push(resolve_feed_url(&base_url, u));
                            }
                        }
                        Ok(ClientActionResult::Disconnect) => {
                            info!("RSS client {} disconnecting on model request", client_id);
                            app_state
                                .update_client_status(client_id, ClientStatus::Disconnected)
                                .await;
                            return Ok(resolved);
                        }
                        Ok(_) => {}
                        Err(e) => warn!("RSS client {} rejected action: {}", client_id, e),
                    }
                }
            }
            Err(e) => {
                error!("RSS client {} LLM error on connect: {}", client_id, e);
                let _ = status_tx.send(format!("[CLIENT] RSS LLM error on connect: {}", e));
            }
        }

        if pending.is_empty() {
            debug!("RSS client {} idle: model requested no fetch", client_id);
            return Ok(resolved);
        }

        let handle = tokio::spawn(Self::fetch_loop(
            pending,
            base_url,
            llm_client,
            app_state.clone(),
            status_tx,
            client_id,
            instruction,
            client_data,
            protocol,
        ));
        app_state.register_client_task(client_id, handle).await;

        Ok(resolved)
    }

    /// Fetch each queued feed, hand the parsed result to the model, and queue whatever it
    /// asks for next. Bounded so a looping model cannot spin forever.
    #[allow(clippy::too_many_arguments)]
    async fn fetch_loop(
        mut pending: Vec<String>,
        base_url: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        instruction: String,
        client_data: Arc<Mutex<ClientData>>,
        protocol: Arc<RssClientProtocol>,
    ) {
        let mut fetched = 0usize;

        while let Some(url) = pending.pop() {
            if fetched >= MAX_CHAINED_FETCHES {
                warn!(
                    "RSS client {} hit the {}-fetch chain limit",
                    client_id, MAX_CHAINED_FETCHES
                );
                break;
            }
            fetched += 1;

            let event_data = match Self::fetch_feed(&url).await {
                Ok(data) => data,
                Err(e) => {
                    error!("RSS client {} fetch of {} failed: {}", client_id, url, e);
                    let _ = status_tx.send(format!("[CLIENT] RSS fetch failed: {}", e));
                    app_state
                        .update_client_status(client_id, ClientStatus::Error(e.to_string()))
                        .await;
                    let _ = status_tx.send("__UPDATE_UI__".to_string());
                    return;
                }
            };

            let item_count = event_data
                .get("item_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            info!(
                "RSS client {} parsed {} ({} items)",
                client_id, url, item_count
            );
            let _ = status_tx.send(format!("[CLIENT] RSS parsed feed: {} items", item_count));

            let event = Event::new(&RSS_CLIENT_FEED_FETCHED_EVENT, event_data);
            let memory_snapshot = client_data.lock().await.memory.clone();
            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &memory_snapshot,
                Some(&event),
                protocol.as_ref() as &dyn Client,
                &status_tx,
            )
            .await
            {
                Ok(ClientLlmResult {
                    actions,
                    memory_updates,
                }) => {
                    if let Some(mem) = memory_updates {
                        client_data.lock().await.memory = mem;
                    }
                    for action in actions {
                        match protocol.execute_action(action) {
                            Ok(ClientActionResult::Custom { name, data })
                                if name == "fetch_rss_feed" =>
                            {
                                if let Some(u) = data.get("url").and_then(|v| v.as_str()) {
                                    pending.push(resolve_feed_url(&base_url, u));
                                }
                            }
                            Ok(ClientActionResult::Disconnect) => {
                                info!("RSS client {} disconnecting", client_id);
                                app_state
                                    .update_client_status(client_id, ClientStatus::Disconnected)
                                    .await;
                                let _ = status_tx.send("__UPDATE_UI__".to_string());
                                return;
                            }
                            Ok(_) => {}
                            Err(e) => warn!("RSS client {} rejected action: {}", client_id, e),
                        }
                    }
                }
                Err(e) => {
                    error!("RSS client {} LLM error: {}", client_id, e);
                }
            }
        }

        app_state
            .update_client_status(client_id, ClientStatus::Disconnected)
            .await;
        let _ = status_tx.send("__UPDATE_UI__".to_string());
    }

    /// Fetch one feed over HTTP and turn it into structured event data.
    pub async fn fetch_feed(url: &str) -> Result<serde_json::Value> {
        let response = reqwest::get(url)
            .await
            .with_context(|| format!("Failed to fetch RSS feed {}", url))?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "RSS fetch of {} failed with status {}",
                url,
                response.status()
            ));
        }

        let body = response
            .text()
            .await
            .context("Failed to read RSS response body")?;

        let channel = Channel::read_from(body.as_bytes())
            .with_context(|| format!("Failed to parse RSS feed at {}", url))?;

        Ok(channel_to_event_data(url, &channel))
    }
}
