//! BitTorrent DHT client implementation
pub mod actions;

pub use actions::TorrentDhtClientProtocol;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{error, info, trace};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::torrent_dht::actions::DHT_RESPONSE_EVENT;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// DHT query/response types
#[derive(Debug, Deserialize, Serialize)]
struct DhtMessage {
    #[serde(rename = "t")]
    transaction_id: serde_bencode::value::Value,
    #[serde(rename = "y")]
    message_type: String, // "q" = query, "r" = response, "e" = error
    #[serde(rename = "q")]
    query_type: Option<String>, // "ping", "find_node", "get_peers", "announce_peer"
    #[serde(rename = "a")]
    arguments: Option<serde_bencode::value::Value>,
    #[serde(rename = "r")]
    response: Option<serde_bencode::value::Value>,
    #[serde(rename = "e")]
    error: Option<serde_bencode::value::Value>,
}

/// BitTorrent DHT client
pub struct TorrentDhtClient;

impl TorrentDhtClient {
    /// Connect to a BitTorrent DHT node with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Parse remote address
        let remote_sock_addr: SocketAddr = remote_addr.parse()
            .context(format!("Invalid DHT node address: {}", remote_addr))?;

        // Create UDP socket
        let socket = UdpSocket::bind("0.0.0.0:0").await
            .context("Failed to bind UDP socket for DHT")?;

        let local_addr = socket.local_addr()?;


        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        console_info!(status_tx, "[CLIENT] BitTorrent DHT client {} connected", client_id);
        console_info!(status_tx, "__UPDATE_UI__");

        let socket_arc = Arc::new(socket);

        // Spawn read loop for DHT responses
        let socket_clone = socket_arc.clone();
        let app_state_clone = app_state.clone();
        let status_tx_clone = status_tx.clone();
        let llm_client_clone = llm_client.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 65536];
            loop {
                match socket_clone.recv_from(&mut buf).await {
                    Ok((len, peer)) => {
                        trace!("DHT client {} received {} bytes from {}", client_id, len, peer);

                        // Parse bencode response
                        match serde_bencode::from_bytes::<DhtMessage>(&buf[..len]) {
                            Ok(msg) => {
                                trace!("DHT message: {:?}", msg);

                                // Call LLM with response
                                if let Some(instruction) = app_state_clone.get_instruction_for_client(client_id).await {
                                    let protocol = Arc::new(crate::client::torrent_dht::actions::TorrentDhtClientProtocol::new());
                                    let event = Event::new(
                                        &DHT_RESPONSE_EVENT,
                                        serde_json::json!({
                                            "message_type": msg.message_type,
                                            "query_type": msg.query_type,
                                            "response": format!("{:?}", msg.response),
                                            "error": format!("{:?}", msg.error),
                                            "peer": peer.to_string(),
                                        }),
                                    );

                                    let memory = app_state_clone.get_memory_for_client(client_id).await.unwrap_or_default();

                                    match call_llm_for_client(
                                        &llm_client_clone,
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
                                                if let Err(e) = Self::execute_dht_action(
                                                    client_id,
                                                    action,
                                                    &socket_clone,
                                                    remote_sock_addr,
                                                    protocol.as_ref(),
                                                ).await {
                                                    error!("Failed to execute DHT action: {}", e);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            error!("LLM error for DHT client {}: {}", client_id, e);
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Failed to parse DHT message: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("DHT client {} recv error: {}", client_id, e);
                        app_state_clone.update_client_status(client_id, ClientStatus::Error(e.to_string())).await;
                        let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        break;
                    }
                }
            }
        });

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::torrent_dht::actions::TorrentDhtClientProtocol::new());
            let event = Event::new(
                &DHT_RESPONSE_EVENT,
                serde_json::json!({
                    "status": "connected",
                    "local_addr": local_addr.to_string(),
                    "remote_addr": remote_sock_addr.to_string(),
                }),
            );

            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();
            let socket_clone = socket_arc.clone();
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
                        if let Some(mem) = memory_updates {
                            app_state_clone.set_memory_for_client(client_id, mem).await;
                        }

                        for action in actions {
                            if let Err(e) = Self::execute_dht_action(
                                client_id,
                                action,
                                &socket_clone,
                                remote_sock_addr,
                                protocol.as_ref(),
                            ).await {
                                error!("Failed to execute DHT action: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("LLM error: {}", e);
                    }
                }
            });
        }

        Ok(local_addr)
    }

    /// Execute a DHT action
    async fn execute_dht_action(
        client_id: ClientId,
        action: serde_json::Value,
        socket: &UdpSocket,
        remote_addr: SocketAddr,
        protocol: &dyn crate::llm::actions::client_trait::Client,
    ) -> Result<()> {
        use crate::llm::actions::client_trait::ClientActionResult;

        match protocol.execute_action(action)? {
            ClientActionResult::Custom { name, data } if name == "dht_query" => {
                let query_type = data.get("query_type").and_then(|v| v.as_str()).context("Missing query_type")?;
                let transaction_id = data.get("transaction_id").and_then(|v| v.as_str()).unwrap_or("aa");
                let node_id = data.get("node_id").and_then(|v| v.as_str()).context("Missing node_id")?;

                // Build DHT query message
                let mut args = serde_json::Map::new();
                args.insert("id".to_string(), serde_json::Value::String(node_id.to_string()));

                if let Some(target) = data.get("target").and_then(|v| v.as_str()) {
                    args.insert("target".to_string(), serde_json::Value::String(target.to_string()));
                }

                if let Some(info_hash) = data.get("info_hash").and_then(|v| v.as_str()) {
                    args.insert("info_hash".to_string(), serde_json::Value::String(info_hash.to_string()));
                }

                let message = serde_json::json!({
                    "t": transaction_id,
                    "y": "q",
                    "q": query_type,
                    "a": args,
                });

                // Encode as bencode
                let encoded = serde_bencode::to_bytes(&message)?;

                // Send query
                socket.send_to(&encoded, remote_addr).await?;
                trace!("DHT client {} sent {} query", client_id, query_type);
            }
            ClientActionResult::Disconnect => {
                info!("DHT client {} disconnecting", client_id);
            }
            _ => {}
        }

        Ok(())
    }
}
