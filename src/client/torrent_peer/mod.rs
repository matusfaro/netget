//! BitTorrent Peer Wire Protocol client implementation
pub mod actions;

pub use actions::TorrentPeerClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, trace};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::torrent_peer::actions::{PEER_HANDSHAKE_EVENT, PEER_MESSAGE_EVENT};

/// Peer wire message types
#[repr(u8)]
#[allow(dead_code)]
enum MessageType {
    Choke = 0,
    Unchoke = 1,
    Interested = 2,
    NotInterested = 3,
    Have = 4,
    Bitfield = 5,
    Request = 6,
    Piece = 7,
    Cancel = 8,
    Port = 9,
}

/// BitTorrent Peer Wire Protocol client
pub struct TorrentPeerClient;

impl TorrentPeerClient {
    /// Connect to a BitTorrent peer with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Connect to peer
        let stream = TcpStream::connect(&remote_addr)
            .await
            .context(format!("Failed to connect to peer at {}", remote_addr))?;

        let local_addr = stream.local_addr()?;
        let remote_sock_addr = stream.peer_addr()?;

        info!("BitTorrent Peer client {} connected to {} (local: {})",
              client_id, remote_sock_addr, local_addr);

        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] BitTorrent Peer client {} connected", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Split stream
        let (mut read_half, write_half) = tokio::io::split(stream);
        let write_half_arc = Arc::new(Mutex::new(write_half));

        // Spawn read loop for peer messages
        let app_state_clone = app_state.clone();
        let status_tx_clone = status_tx.clone();
        let write_half_clone = write_half_arc.clone();
        let llm_client_clone = llm_client.clone();
        tokio::spawn(async move {
            // First, wait for handshake
            let mut handshake_buf = vec![0u8; 68]; // 1 + 19 + 8 + 20 + 20
            match read_half.read_exact(&mut handshake_buf).await {
                Ok(_) => {
                    trace!("Peer client {} received handshake", client_id);

                    // Parse handshake (simplified)
                    let pstrlen = handshake_buf[0];
                    if pstrlen == 19 && &handshake_buf[1..20] == b"BitTorrent protocol" {
                        let reserved = &handshake_buf[20..28];
                        let info_hash = &handshake_buf[28..48];
                        let peer_id = &handshake_buf[48..68];

                        trace!("Handshake: info_hash={:?}, peer_id={:?}",
                               hex::encode(info_hash), hex::encode(peer_id));

                        // Call LLM with handshake event
                        if let Some(instruction) = app_state_clone.get_instruction_for_client(client_id).await {
                            let protocol = Arc::new(crate::client::torrent_peer::actions::TorrentPeerClientProtocol::new());
                            let event = Event::new(
                                &PEER_HANDSHAKE_EVENT,
                                serde_json::json!({
                                    "info_hash": hex::encode(info_hash),
                                    "peer_id": hex::encode(peer_id),
                                    "reserved": hex::encode(reserved),
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
                                    if let Some(mem) = memory_updates {
                                        app_state_clone.set_memory_for_client(client_id, mem).await;
                                    }

                                    for action in actions {
                                        if let Err(e) = Self::execute_peer_action(
                                            client_id,
                                            action,
                                            &write_half_clone,
                                            protocol.as_ref(),
                                        ).await {
                                            error!("Failed to execute peer action: {}", e);
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("LLM error for Peer client {}: {}", client_id, e);
                                }
                            }
                        }
                    }

                    // Now read peer wire messages
                    loop {
                        // Read message length (4 bytes)
                        let mut len_buf = [0u8; 4];
                        match read_half.read_exact(&mut len_buf).await {
                            Ok(_) => {
                                let msg_len = u32::from_be_bytes(len_buf);

                                if msg_len == 0 {
                                    // Keep-alive message
                                    trace!("Peer client {} received keep-alive", client_id);
                                    continue;
                                }

                                // Read message
                                let mut msg_buf = vec![0u8; msg_len as usize];
                                match read_half.read_exact(&mut msg_buf).await {
                                    Ok(_) => {
                                        let msg_type = msg_buf[0];
                                        let payload = &msg_buf[1..];

                                        trace!("Peer client {} received message type {}, len {}",
                                               client_id, msg_type, payload.len());

                                        // Call LLM with message event
                                        if let Some(instruction) = app_state_clone.get_instruction_for_client(client_id).await {
                                            let protocol = Arc::new(crate::client::torrent_peer::actions::TorrentPeerClientProtocol::new());
                                            let event = Event::new(
                                                &PEER_MESSAGE_EVENT,
                                                serde_json::json!({
                                                    "message_type": msg_type,
                                                    "payload_len": payload.len(),
                                                    "payload_hex": hex::encode(payload),
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
                                                    if let Some(mem) = memory_updates {
                                                        app_state_clone.set_memory_for_client(client_id, mem).await;
                                                    }

                                                    for action in actions {
                                                        if let Err(e) = Self::execute_peer_action(
                                                            client_id,
                                                            action,
                                                            &write_half_clone,
                                                            protocol.as_ref(),
                                                        ).await {
                                                            error!("Failed to execute peer action: {}", e);
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    error!("LLM error: {}", e);
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("Peer client {} message read error: {}", client_id, e);
                                        break;
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Peer client {} length read error: {}", client_id, e);
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Peer client {} handshake read error: {}", client_id, e);
                }
            }

            app_state_clone.update_client_status(client_id, ClientStatus::Disconnected).await;
            let _ = status_tx_clone.send(format!("[CLIENT] BitTorrent Peer client {} disconnected", client_id));
            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
        });

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::torrent_peer::actions::TorrentPeerClientProtocol::new());
            let event = Event::new(
                &PEER_HANDSHAKE_EVENT,
                serde_json::json!({
                    "status": "connected",
                    "remote_addr": remote_sock_addr.to_string(),
                }),
            );

            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();
            let write_half_clone = write_half_arc.clone();

            tokio::spawn(async move {
                match call_llm_for_client(
                    &llm_client,
                    &app_state,
                    client_id.to_string(),
                    &instruction,
                    &memory,
                    Some(&event),
                    protocol.as_ref(),
                    &status_tx,
                ).await {
                    Ok(ClientLlmResult { actions, memory_updates }) => {
                        if let Some(mem) = memory_updates {
                            app_state.set_memory_for_client(client_id, mem).await;
                        }

                        for action in actions {
                            if let Err(e) = Self::execute_peer_action(
                                client_id,
                                action,
                                &write_half_clone,
                                protocol.as_ref(),
                            ).await {
                                error!("Failed to execute peer action: {}", e);
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

    /// Execute a peer action
    async fn execute_peer_action(
        client_id: ClientId,
        action: serde_json::Value,
        write_half: &Arc<Mutex<tokio::io::WriteHalf<TcpStream>>>,
        protocol: &dyn crate::llm::actions::client_trait::Client,
    ) -> Result<()> {
        use crate::llm::actions::client_trait::ClientActionResult;

        match protocol.execute_action(action)? {
            ClientActionResult::Custom { name, data } if name == "peer_handshake" => {
                let info_hash_hex = data.get("info_hash").and_then(|v| v.as_str()).context("Missing info_hash")?;
                let peer_id_hex = data.get("peer_id").and_then(|v| v.as_str()).context("Missing peer_id")?;

                let info_hash = hex::decode(info_hash_hex)?;
                let peer_id = hex::decode(peer_id_hex)?;

                // Build handshake
                let mut handshake = Vec::new();
                handshake.push(19); // pstrlen
                handshake.extend_from_slice(b"BitTorrent protocol"); // pstr
                handshake.extend_from_slice(&[0u8; 8]); // reserved
                handshake.extend_from_slice(&info_hash); // info_hash
                handshake.extend_from_slice(&peer_id); // peer_id

                write_half.lock().await.write_all(&handshake).await?;
                trace!("Peer client {} sent handshake", client_id);
            }
            ClientActionResult::Custom { name, data } if name == "peer_message" => {
                let msg_type = data.get("message_type").and_then(|v| v.as_u64()).context("Missing message_type")? as u8;
                let payload_hex = data.get("payload").and_then(|v| v.as_str()).unwrap_or("");
                let payload = if !payload_hex.is_empty() {
                    hex::decode(payload_hex)?
                } else {
                    vec![]
                };

                // Build message
                let msg_len = (1 + payload.len()) as u32;
                let mut message = Vec::new();
                message.extend_from_slice(&msg_len.to_be_bytes());
                message.push(msg_type);
                message.extend_from_slice(&payload);

                write_half.lock().await.write_all(&message).await?;
                trace!("Peer client {} sent message type {}", client_id, msg_type);
            }
            ClientActionResult::Disconnect => {
                info!("Peer client {} disconnecting", client_id);
            }
            _ => {}
        }

        Ok(())
    }
}
