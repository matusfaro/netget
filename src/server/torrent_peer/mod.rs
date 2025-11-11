//! BitTorrent Peer Wire Protocol server implementation
//!
//! TCP-based protocol for peer-to-peer data transfer between BitTorrent clients

pub mod actions;

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use actions::TorrentPeerProtocol;

/// BitTorrent Peer Wire Protocol server
pub struct TorrentPeerServer;

impl TorrentPeerServer {
    /// Spawn BitTorrent Peer server with LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let listener = TcpListener::bind(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        console_info!(status_tx, "[INFO] BitTorrent Peer server listening on {}", local_addr);

        let protocol = Arc::new(TorrentPeerProtocol::new());

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();

                        debug!("BitTorrent Peer accepted connection from {}", peer_addr);
                        let _ = status_clone.send(format!("[DEBUG] BitTorrent Peer accepted connection from {}", peer_addr));

                        // Split stream for read/write
                        let (read_half, write_half) = tokio::io::split(stream);
                        let write_half = Arc::new(tokio::sync::Mutex::new(write_half));

                        // Add connection to ServerInstance
                        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
                        let now = std::time::Instant::now();
                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr: peer_addr,
                            local_addr,
                            bytes_sent: 0,
                            bytes_received: 0,
                            packets_sent: 0,
                            packets_received: 0,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::empty(),
                        };
                        state_clone.add_connection_to_server(server_id, conn_state).await;
                        let _ = status_clone.send("__UPDATE_UI__".to_string());

                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_connection(
                                read_half,
                                write_half,
                                peer_addr,
                                local_addr,
                                connection_id,
                                llm_clone,
                                state_clone,
                                status_clone,
                                server_id,
                                protocol_clone,
                            ).await {
                                error!("BitTorrent Peer connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "[ERROR] BitTorrent Peer accept error: {}", e);
                    }
                }
            }
        });

        Ok(local_addr)
    }

    async fn handle_connection(
        mut read_half: tokio::io::ReadHalf<tokio::net::TcpStream>,
        write_half: Arc<tokio::sync::Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>,
        peer_addr: SocketAddr,
        _local_addr: SocketAddr,
        connection_id: ConnectionId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        protocol: Arc<TorrentPeerProtocol>,
    ) -> Result<()> {
        use tokio::io::AsyncReadExt;

        let mut buffer = vec![0u8; 16384];
        let mut handshake_complete = false;

        loop {
            let n = read_half.read(&mut buffer).await?;
            if n == 0 {
                console_debug!(status_tx, "[DEBUG] BitTorrent Peer connection closed by peer");
                break;
            }

            let data = buffer[..n].to_vec();

            console_debug!(status_tx, "[DEBUG] BitTorrent Peer received {} bytes from {}", n, peer_addr);

            // TRACE: Log full payload
            let hex_str = hex::encode(&data);
            console_trace!(status_tx, "[TRACE] BitTorrent Peer data (hex): {}", hex_str);

            // Parse message
            if !handshake_complete {
                // Parse handshake
                match Self::parse_handshake(&data) {
                    Ok((info_hash, peer_id)) => {
                        console_debug!(status_tx, "[DEBUG] BitTorrent Peer handshake: info_hash={}, peer_id={}", info_hash, peer_id);

                        handshake_complete = true;

                        // Create event for LLM
                        let event = Event::new(
                            &actions::PEER_HANDSHAKE_EVENT,
                            serde_json::json!({
                                "info_hash": info_hash,
                                "peer_id": peer_id,
                            }),
                        );

                        console_debug!(status_tx, "[DEBUG] BitTorrent Peer calling LLM for handshake");

                        // Call LLM
                        match call_llm(
                            &llm_client,
                            &app_state,
                            server_id,
                            Some(connection_id),
                            &event,
                            protocol.as_ref(),
                        ).await {
                            Ok(execution_result) => {
                                Self::process_llm_response(
                                    execution_result,
                                    &write_half,
                                    peer_addr,
                                    &status_tx,
                                ).await?;
                            }
                            Err(e) => {
                                console_error!(status_tx, "✗ BitTorrent Peer LLM error: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        console_error!(status_tx, "[ERROR] Failed to parse handshake: {}", e);
                        break;
                    }
                }
            } else {
                // Parse peer wire message
                match Self::parse_message(&data) {
                    Ok((message_type, message_data)) => {
                        console_debug!(status_tx, "[DEBUG] BitTorrent Peer message type: {}", message_type);

                        // Create event for LLM
                        let event_type = match message_type.as_str() {
                            "choke" => &actions::PEER_CHOKE_MESSAGE_EVENT,
                            "request" => &actions::PEER_REQUEST_MESSAGE_EVENT,
                            "bitfield" => &actions::PEER_BITFIELD_MESSAGE_EVENT,
                            _ => &actions::PEER_CHOKE_MESSAGE_EVENT, // Default
                        };
                        let event = Event::new(event_type, message_data);

                        console_debug!(status_tx, "[DEBUG] BitTorrent Peer calling LLM for {} message", message_type);

                        // Call LLM
                        match call_llm(
                            &llm_client,
                            &app_state,
                            server_id,
                            Some(connection_id),
                            &event,
                            protocol.as_ref(),
                        ).await {
                            Ok(execution_result) => {
                                Self::process_llm_response(
                                    execution_result,
                                    &write_half,
                                    peer_addr,
                                    &status_tx,
                                ).await?;
                            }
                            Err(e) => {
                                console_error!(status_tx, "✗ BitTorrent Peer LLM error: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        console_error!(status_tx, "[ERROR] Failed to parse peer message: {}", e);
                    }
                }
            }
        }

        Ok(())
    }

    async fn process_llm_response(
        execution_result: crate::llm::actions::executor::ExecutionResult,
        write_half: &Arc<tokio::sync::Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>,
        peer_addr: SocketAddr,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        use tokio::io::AsyncWriteExt;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

        // Display messages from LLM
        for message in &execution_result.messages {
            console_info!(status_tx, "[INFO] {}", message);
        }

        console_debug!(status_tx, "[DEBUG] BitTorrent Peer got {} protocol results", execution_result.protocol_results.len());

        // Send responses
        for protocol_result in execution_result.protocol_results {
            if let Some(output_data) = protocol_result.get_all_output().first() {
                let mut write = write_half.lock().await;
                write.write_all(output_data).await?;

                console_debug!(status_tx, "[DEBUG] BitTorrent Peer sent {} bytes to {}", output_data.len(), peer_addr);

                // TRACE: Log full response
                let hex_str = hex::encode(output_data);
                console_trace!(status_tx, "[TRACE] BitTorrent Peer sent (hex): {}", hex_str);
            }
        }

        Ok(())
    }

    fn parse_handshake(data: &[u8]) -> Result<(String, String)> {
        // Handshake format: <pstrlen><pstr><reserved><info_hash><peer_id>
        // pstrlen = 19, pstr = "BitTorrent protocol"

        if data.len() < 68 {
            return Err(anyhow::anyhow!("Handshake too short"));
        }

        let pstrlen = data[0] as usize;
        if pstrlen != 19 {
            return Err(anyhow::anyhow!("Invalid pstrlen"));
        }

        let pstr = &data[1..20];
        if pstr != b"BitTorrent protocol" {
            return Err(anyhow::anyhow!("Invalid protocol string"));
        }

        // reserved = 8 bytes (bytes 20-27)
        let info_hash = hex::encode(&data[28..48]);
        let peer_id = String::from_utf8_lossy(&data[48..68]).to_string();

        Ok((info_hash, peer_id))
    }

    fn parse_message(data: &[u8]) -> Result<(String, serde_json::Value)> {
        if data.is_empty() {
            return Ok(("keepalive".to_string(), serde_json::json!({})));
        }

        if data.len() < 4 {
            return Err(anyhow::anyhow!("Message too short"));
        }

        let length = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;

        if length == 0 {
            return Ok(("keepalive".to_string(), serde_json::json!({})));
        }

        if data.len() < 4 + length {
            return Err(anyhow::anyhow!("Incomplete message"));
        }

        let message_id = data[4];
        let payload = &data[5..4 + length];

        let (message_type, message_data) = match message_id {
            0 => ("choke", serde_json::json!({})),
            1 => ("unchoke", serde_json::json!({})),
            2 => ("interested", serde_json::json!({})),
            3 => ("not_interested", serde_json::json!({})),
            4 => {
                if payload.len() >= 4 {
                    let piece_index = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                    ("have", serde_json::json!({"piece_index": piece_index}))
                } else {
                    ("have", serde_json::json!({}))
                }
            }
            5 => {
                // Bitfield message
                ("bitfield", serde_json::json!({"bitfield": hex::encode(payload)}))
            }
            6 => {
                if payload.len() >= 12 {
                    let index = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                    let begin = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
                    let length = u32::from_be_bytes([payload[8], payload[9], payload[10], payload[11]]);
                    ("request", serde_json::json!({"index": index, "begin": begin, "length": length}))
                } else {
                    ("request", serde_json::json!({}))
                }
            }
            7 => {
                if payload.len() >= 8 {
                    let index = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                    let begin = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
                    let block = hex::encode(&payload[8..]);
                    ("piece", serde_json::json!({"index": index, "begin": begin, "block_hex": block}))
                } else {
                    ("piece", serde_json::json!({}))
                }
            }
            8 => {
                if payload.len() >= 12 {
                    let index = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                    let begin = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
                    let length = u32::from_be_bytes([payload[8], payload[9], payload[10], payload[11]]);
                    ("cancel", serde_json::json!({"index": index, "begin": begin, "length": length}))
                } else {
                    ("cancel", serde_json::json!({}))
                }
            }
            _ => ("unknown", serde_json::json!({"id": message_id, "payload_hex": hex::encode(payload)})),
        };

        Ok((message_type.to_string(), message_data))
    }
}
