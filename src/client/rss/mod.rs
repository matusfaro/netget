//! RSS (Really Simple Syndication) Feed Client
//!
//! Fetches and parses RSS 2.0 XML feeds with LLM-controlled interpretation.
//! The LLM decides which feeds to fetch and how to process items.

pub mod actions;

use crate::client::rss::actions::{RSS_CLIENT_CONNECTED_EVENT, RSS_CLIENT_FEED_FETCHED_EVENT};
use crate::llm::client::OllamaClient;
use crate::llm::llm_helpers::call_llm_for_client;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::client::{ClientConnectionState, ClientId};
use anyhow::{Context, Result};
use reqwest;
use rss::Channel;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tracing::{debug, error, info};

/// RSS client state
pub struct RssClient;

impl RssClient {
    /// Connect to RSS feed source and set up LLM integration
    pub async fn connect_with_llm_actions(
        remote_addr: SocketAddr,
        llm_client: Arc<OllamaClient>,
        app_state: Arc<AppState>,
        status_tx: Sender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        info!("RSS client connecting to {}", remote_addr);
        status_tx
            .send(format!("[RSS CLIENT] Connecting to {}", remote_addr))
            .await
            .ok();

        // Store base URL in protocol_data
        let base_url = format!("http://{}", remote_addr);
        app_state.clients.update_protocol_data(
            client_id,
            json!({
                "base_url": base_url,
            }),
        )?;

        // Mark as connected
        app_state.clients.set_status(
            client_id,
            crate::state::client::ClientStatus::Connected {
                peer_addr: remote_addr,
            },
        )?;

        info!("RSS client connected (ready to fetch feeds)");
        status_tx
            .send(format!("[RSS CLIENT] Connected to {}", remote_addr))
            .await
            .ok();

        // Call LLM with connected event
        let event = Event::new(&RSS_CLIENT_CONNECTED_EVENT, json!({ "base_url": base_url }));
        call_llm_for_client(
            Arc::clone(&llm_client),
            Arc::clone(&app_state),
            status_tx.clone(),
            client_id,
            Some(&event),
        )
        .await?;

        // Spawn background monitor
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

                // Check if client still exists
                if app_state.clients.get(client_id).is_err() {
                    debug!("RSS client {} removed, exiting monitor", client_id);
                    break;
                }
            }
        });

        Ok(remote_addr)
    }

    /// Fetch RSS feed from URL
    pub async fn fetch_feed(
        client_id: ClientId,
        url: String,
        llm_client: Arc<OllamaClient>,
        app_state: Arc<AppState>,
        status_tx: Sender<String>,
    ) -> Result<()> {
        // Check connection state - prevent concurrent LLM calls
        {
            let state = app_state.clients.get_connection_state(client_id)?;
            match state {
                ClientConnectionState::Processing => {
                    debug!(
                        "RSS client {} already processing, ignoring fetch",
                        client_id
                    );
                    return Ok(());
                }
                ClientConnectionState::Idle => {
                    // Mark as processing
                    app_state
                        .clients
                        .set_connection_state(client_id, ClientConnectionState::Processing)?;
                }
                ClientConnectionState::Accumulating => {
                    // Should not happen for RSS client (request/response)
                    debug!(
                        "RSS client {} in accumulating state, ignoring fetch",
                        client_id
                    );
                    return Ok(());
                }
            }
        }

        info!("RSS client {} fetching feed: {}", client_id, url);
        status_tx
            .send(format!("[RSS CLIENT] Fetching feed: {}", url))
            .await
            .ok();

        // Fetch RSS feed via HTTP
        let response = reqwest::get(&url)
            .await
            .context("Failed to fetch RSS feed")?;

        if !response.status().is_success() {
            error!("RSS fetch failed with status: {}", response.status());
            status_tx
                .send(format!("[RSS CLIENT] Fetch failed: {}", response.status()))
                .await
                .ok();

            // Reset to idle
            app_state
                .clients
                .set_connection_state(client_id, ClientConnectionState::Idle)?;
            return Err(anyhow::anyhow!(
                "RSS fetch failed with status: {}",
                response.status()
            ));
        }

        let body = response
            .text()
            .await
            .context("Failed to read response body")?;

        // Parse RSS XML
        let channel = Channel::read_from(body.as_bytes()).context("Failed to parse RSS feed")?;

        info!(
            "RSS feed parsed: {} items from '{}'",
            channel.items().len(),
            channel.title()
        );
        status_tx
            .send(format!(
                "[RSS CLIENT] Parsed feed: {} items",
                channel.items().len()
            ))
            .await
            .ok();

        // Convert items to JSON
        let items_json: Vec<_> = channel
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

        // Call LLM with feed data
        let event = Event::new(
            &RSS_CLIENT_FEED_FETCHED_EVENT,
            json!({
                "url": url,
                "feed_title": channel.title(),
                "feed_link": channel.link(),
                "feed_description": channel.description(),
                "item_count": channel.items().len(),
                "items": items_json,
            }),
        );

        call_llm_for_client(
            llm_client,
            Arc::clone(&app_state),
            status_tx,
            client_id,
            Some(&event),
        )
        .await?;

        // Reset to idle
        app_state
            .clients
            .set_connection_state(client_id, ClientConnectionState::Idle)?;

        Ok(())
    }
}
