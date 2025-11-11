//! BitTorrent Tracker client implementation
pub mod actions;

pub use actions::TorrentTrackerClientProtocol;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, trace};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::torrent_tracker::actions::{
    TRACKER_ANNOUNCE_RESPONSE_EVENT, TRACKER_SCRAPE_RESPONSE_EVENT
};

/// BitTorrent tracker response (announce)
#[derive(Debug, Deserialize, Serialize)]
struct TrackerResponse {
    #[serde(rename = "failure reason")]
    failure_reason: Option<String>,
    #[serde(rename = "warning message")]
    warning_message: Option<String>,
    interval: Option<i64>,
    #[serde(rename = "min interval")]
    min_interval: Option<i64>,
    #[serde(rename = "tracker id")]
    tracker_id: Option<String>,
    complete: Option<i64>,
    incomplete: Option<i64>,
    peers: Option<serde_bencode::value::Value>,
}

/// BitTorrent tracker scrape response
#[derive(Debug, Deserialize, Serialize)]
struct ScrapeResponse {
    files: Option<serde_bencode::value::Value>,
}

/// BitTorrent Tracker client
pub struct TorrentTrackerClient;

impl TorrentTrackerClient {
    /// Connect to a BitTorrent tracker with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // BitTorrent tracker is HTTP-based, so we don't maintain a persistent connection
        // We'll just track the tracker URL and make HTTP requests as needed


        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        console_info!(status_tx, "[CLIENT] BitTorrent Tracker client {} connected to {}", client_id, remote_addr);
        console_info!(status_tx, "__UPDATE_UI__");

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::torrent_tracker::actions::TorrentTrackerClientProtocol::new());
            let event = Event::new(
                &TRACKER_ANNOUNCE_RESPONSE_EVENT,
                serde_json::json!({
                    "tracker_url": remote_addr,
                    "status": "connected",
                }),
            );

            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

            // Execute LLM call in background
            let app_state_clone = app_state.clone();
            let status_tx_clone = status_tx.clone();
            tokio::spawn(async move {
                match call_llm_for_client(
                    &llm_client,
                    &app_state_clone,
                    client_id.to_string(),
                    &instruction,
                    &memory,
                    Some(&event),
                    protocol.as_ref(),
                    &status_tx_clone,
                ).await {
                    Ok(ClientLlmResult { actions, memory_updates }) => {
                        // Update memory
                        if let Some(mem) = memory_updates {
                            app_state_clone.set_memory_for_client(client_id, mem).await;
                        }

                        // Execute actions
                        for action in actions {
                            if let Err(e) = Self::execute_tracker_action(
                                client_id,
                                action,
                                &remote_addr,
                                protocol.as_ref(),
                                &app_state_clone,
                                &llm_client,
                                &status_tx_clone,
                            ).await {
                                error!("Failed to execute tracker action: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("LLM error for Tracker client {}: {}", client_id, e);
                    }
                }
            });
        }

        // Return a dummy local address (tracker is HTTP-based)
        Ok("0.0.0.0:0".parse()?)
    }

    /// Execute a tracker action
    async fn execute_tracker_action(
        client_id: ClientId,
        action: serde_json::Value,
        tracker_url: &str,
        protocol: &dyn crate::llm::actions::client_trait::Client,
        app_state: &Arc<AppState>,
        llm_client: &OllamaClient,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        use crate::llm::actions::client_trait::ClientActionResult;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

        match protocol.execute_action(action)? {
            ClientActionResult::Custom { name, data } if name == "tracker_announce" => {
                let info_hash = data.get("info_hash").and_then(|v| v.as_str()).context("Missing info_hash")?;
                let peer_id = data.get("peer_id").and_then(|v| v.as_str()).context("Missing peer_id")?;
                let port = data.get("port").and_then(|v| v.as_u64()).context("Missing port")? as u16;
                let uploaded = data.get("uploaded").and_then(|v| v.as_u64()).unwrap_or(0);
                let downloaded = data.get("downloaded").and_then(|v| v.as_u64()).unwrap_or(0);
                let left = data.get("left").and_then(|v| v.as_u64()).unwrap_or(0);
                let event_type = data.get("event").and_then(|v| v.as_str()).unwrap_or("started");

                // Build announce URL
                let announce_url = format!(
                    "{}?info_hash={}&peer_id={}&port={}&uploaded={}&downloaded={}&left={}&event={}",
                    tracker_url, info_hash, peer_id, port, uploaded, downloaded, left, event_type
                );

                trace!("Tracker client {} announcing to: {}", client_id, announce_url);

                // Make HTTP GET request
                let response = reqwest::get(&announce_url).await?;
                let body = response.bytes().await?;

                // Parse bencode response
                match serde_bencode::from_bytes::<TrackerResponse>(&body) {
                    Ok(tracker_resp) => {
                        trace!("Tracker response: {:?}", tracker_resp);

                        // Call LLM with announce response
                        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                            let event = Event::new(
                                &TRACKER_ANNOUNCE_RESPONSE_EVENT,
                                serde_json::json!({
                                    "interval": tracker_resp.interval,
                                    "complete": tracker_resp.complete,
                                    "incomplete": tracker_resp.incomplete,
                                    "peers": format!("{:?}", tracker_resp.peers),
                                }),
                            );

                            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();
                            let protocol_ref = Arc::new(crate::client::torrent_tracker::actions::TorrentTrackerClientProtocol::new());

                            match call_llm_for_client(
                                llm_client,
                                app_state,
                                client_id.to_string(),
                                &instruction,
                                &memory,
                                Some(&event),
                                protocol_ref.as_ref(),
                                status_tx,
                            ).await {
                                Ok(ClientLlmResult { memory_updates, .. }) => {
                                    if let Some(mem) = memory_updates {
                                        app_state.set_memory_for_client(client_id, mem).await;
                                    }
                                }
                                Err(e) => {
                                    error!("LLM error: {}", e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to parse tracker response: {}", e);
                    }
                }
            }
            ClientActionResult::Custom { name, data } if name == "tracker_scrape" => {
                let info_hash = data.get("info_hash").and_then(|v| v.as_str()).context("Missing info_hash")?;

                // Build scrape URL
                let scrape_url = format!("{}?info_hash={}", tracker_url, info_hash);

                trace!("Tracker client {} scraping: {}", client_id, scrape_url);

                // Make HTTP GET request
                let response = reqwest::get(&scrape_url).await?;
                let body = response.bytes().await?;

                // Parse bencode response
                match serde_bencode::from_bytes::<ScrapeResponse>(&body) {
                    Ok(scrape_resp) => {
                        trace!("Scrape response: {:?}", scrape_resp);

                        // Call LLM with scrape response
                        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                            let event = Event::new(
                                &TRACKER_SCRAPE_RESPONSE_EVENT,
                                serde_json::json!({
                                    "files": format!("{:?}", scrape_resp.files),
                                }),
                            );

                            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();
                            let protocol_ref = Arc::new(crate::client::torrent_tracker::actions::TorrentTrackerClientProtocol::new());

                            match call_llm_for_client(
                                llm_client,
                                app_state,
                                client_id.to_string(),
                                &instruction,
                                &memory,
                                Some(&event),
                                protocol_ref.as_ref(),
                                status_tx,
                            ).await {
                                Ok(ClientLlmResult { memory_updates, .. }) => {
                                    if let Some(mem) = memory_updates {
                                        app_state.set_memory_for_client(client_id, mem).await;
                                    }
                                }
                                Err(e) => {
                                    error!("LLM error: {}", e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to parse scrape response: {}", e);
                    }
                }
            }
            ClientActionResult::Disconnect => {
                app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                console_info!(status_tx, "__UPDATE_UI__");
            }
            _ => {}
        }

        Ok(())
    }
}
