//! STUN server implementation
pub mod actions;

use crate::server::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use actions::STUN_BINDING_REQUEST_EVENT;
use crate::server::StunProtocol;
use crate::protocol::Event;
use crate::state::app_state::AppState;

/// STUN server that handles binding requests
pub struct StunServer;

impl StunServer {
    /// Spawn STUN server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        console_info!(status_tx, "[INFO] STUN server listening on {}", local_addr);

        let protocol = Arc::new(StunProtocol::new());

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 2048]; // STUN messages are typically < 2KB

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);

                        // Add connection to ServerInstance (STUN "connection" = transaction)
                        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
use crate::{console_trace, console_debug, console_info, console_warn, console_error};
                        let now = std::time::Instant::now();
                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr: peer_addr,
                            local_addr,
                            bytes_sent: 0,
                            bytes_received: n as u64,
                            packets_sent: 0,
                            packets_received: 1,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::empty(),
                        };
                        app_state.add_connection_to_server(server_id, conn_state).await;
                        console_info!(status_tx, "__UPDATE_UI__");

                        // DEBUG: Log summary
                        console_debug!(status_tx, "[DEBUG] STUN received {} bytes from {}", n, peer_addr);

                        // TRACE: Log full payload
                        let hex_str = hex::encode(&data);
                        console_trace!(status_tx, "[TRACE] STUN data (hex): {}", hex_str);

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            // Parse STUN message to extract transaction ID and message type
                            let (transaction_id, message_type, is_valid) = Self::parse_stun_header(&data);

                            if !is_valid {
                                debug!("STUN invalid message from {}", peer_addr);
                                let _ = status_clone.send(format!("[DEBUG] STUN invalid message from {}", peer_addr));
                                return;
                            }

                            let transaction_id_hex = transaction_id.map(|tid| hex::encode(tid)).unwrap_or_default();

                            // Create STUN binding request event
                            let event_data = serde_json::json!({
                                "peer_addr": peer_addr.to_string(),
                                "local_addr": local_addr.to_string(),
                                "transaction_id": transaction_id_hex,
                                "message_type": message_type,
                                "bytes_received": data.len()
                            });

                            let event = Event::new(&STUN_BINDING_REQUEST_EVENT, event_data);

                            debug!("STUN calling LLM for binding request from {}", peer_addr);
                            let _ = status_clone.send(format!("[DEBUG] STUN calling LLM for binding request from {}", peer_addr));

                            match call_llm(
                                &llm_clone,
                                &state_clone,
                                server_id,
                                None,  // STUN uses UDP, no persistent connection
                                &event,
                                protocol_clone.as_ref(),
                            ).await {
                                Ok(execution_result) => {
                                    // Display messages from LLM
                                    for message in &execution_result.messages {
                                        info!("{}", message);
                                        let _ = status_clone.send(format!("[INFO] {}", message));
                                    }

                                    debug!("STUN parsed {} actions", execution_result.raw_actions.len());
                                    let _ = status_clone.send(format!("[DEBUG] STUN parsed {} actions", execution_result.raw_actions.len()));

                                    // Process protocol results
                                    debug!("STUN got {} protocol results", execution_result.protocol_results.len());
                                    let _ = status_clone.send(format!("[DEBUG] STUN got {} protocol results", execution_result.protocol_results.len()));

                                    for protocol_result in execution_result.protocol_results {
                                        if let Some(output_data) = protocol_result.get_all_output().first() {
                                            let _ = socket_clone.send_to(output_data, peer_addr).await;

                                            // DEBUG: Log summary
                                            debug!("STUN sent {} bytes to {}", output_data.len(), peer_addr);
                                            let _ = status_clone.send(format!("[DEBUG] STUN sent {} bytes to {}", output_data.len(), peer_addr));

                                            // TRACE: Log full payload
                                            let hex_str = hex::encode(output_data);
                                            trace!("STUN sent (hex): {}", hex_str);
                                            let _ = status_clone.send(format!("[TRACE] STUN sent (hex): {}", hex_str));

                                            let _ = status_clone.send(format!("→ STUN response to {} ({} bytes)", peer_addr, output_data.len()));
                                        } else {
                                            debug!("STUN protocol result has no output data");
                                            let _ = status_clone.send("[DEBUG] STUN protocol result has no output data".to_string());
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("STUN LLM call failed: {}", e);
                                    let _ = status_clone.send(format!("✗ STUN LLM error: {}", e));
                                }
                            }
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "✗ STUN receive error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Parse STUN message header to extract transaction ID and message type
    /// Returns (transaction_id, message_type_string, is_valid)
    fn parse_stun_header(data: &[u8]) -> (Option<Vec<u8>>, String, bool) {
        // STUN message header is 20 bytes minimum
        if data.len() < 20 {
            return (None, "invalid".to_string(), false);
        }

        // Check magic cookie (bytes 4-7 should be 0x2112A442)
        let magic_cookie = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        if magic_cookie != 0x2112A442 {
            return (None, "invalid".to_string(), false);
        }

        // Extract message type (first 2 bytes)
        let message_type_raw = u16::from_be_bytes([data[0], data[1]]);

        // Message type encoding: 0bMMMMMMMMMMCCCCMM
        // M = method (14 bits), C = class (2 bits)
        let class = ((message_type_raw & 0x0110) >> 4) | ((message_type_raw & 0x0100) >> 7);
        let method = (message_type_raw & 0x000F)
                   | ((message_type_raw & 0x00E0) >> 1)
                   | ((message_type_raw & 0x3E00) >> 2);

        let message_type = match (class, method) {
            (0, 1) => "BindingRequest",
            (1, 1) => "BindingResponse",
            (2, 1) => "BindingError",
            _ => "Unknown",
        };

        // Extract transaction ID (12 bytes, from byte 8 to 19)
        let transaction_id = data[8..20].to_vec();

        (Some(transaction_id), message_type.to_string(), true)
    }
}
