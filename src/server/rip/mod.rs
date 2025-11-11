//! RIP (Routing Information Protocol) server implementation
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
use actions::RIP_REQUEST_EVENT;
use crate::server::RipProtocol;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// RIP server that forwards routing requests to LLM
pub struct RipServer;

impl RipServer {
    /// Spawn RIP server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        console_info!(status_tx, "[INFO] RIP server listening on {}", local_addr);

        let protocol = Arc::new(RipProtocol::new());

        tokio::spawn(async move {
            // Maximum RIP packet size: 4-byte header + up to 25 route entries (20 bytes each) = 504 bytes
            let mut buffer = vec![0u8; 512];

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);

                        // Add connection to ServerInstance (RIP "connection" = recent peer)
                        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
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

                        // Parse RIP packet to determine message type
                        if n < 4 {
                            console_debug!(status_tx, "[DEBUG] RIP received invalid packet (too short: {} bytes) from {}", n, peer_addr);
                            continue;
                        }

                        let command = data[0];
                        let version = data[1];
                        let num_entries = (n - 4) / 20;

                        // DEBUG: Log summary
                        console_debug!(status_tx, "[DEBUG] RIP received {} bytes from {} (cmd={}, ver={}, entries={})");

                        // TRACE: Log full payload (hex)
                        let hex_str = hex::encode(&data);
                        console_trace!(status_tx, "[TRACE] RIP data (hex): {}", hex_str);

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            // Parse RIP message type
                            let message_type = match command {
                                1 => "request",
                                2 => "response",
                                _ => "unknown",
                            };

                            // Parse route entries
                            let mut routes = Vec::new();
                            for i in 0..num_entries {
                                let offset = 4 + (i * 20);
                                if offset + 20 <= data.len() {
                                    let afi = u16::from_be_bytes([data[offset], data[offset + 1]]);
                                    let route_tag = u16::from_be_bytes([data[offset + 2], data[offset + 3]]);
                                    let ip = format!(
                                        "{}.{}.{}.{}",
                                        data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7]
                                    );
                                    let subnet_mask = format!(
                                        "{}.{}.{}.{}",
                                        data[offset + 8], data[offset + 9], data[offset + 10], data[offset + 11]
                                    );
                                    let next_hop = format!(
                                        "{}.{}.{}.{}",
                                        data[offset + 12], data[offset + 13], data[offset + 14], data[offset + 15]
                                    );
                                    let metric = u32::from_be_bytes([
                                        data[offset + 16], data[offset + 17], data[offset + 18], data[offset + 19]
                                    ]);

                                    routes.push(serde_json::json!({
                                        "afi": afi,
                                        "route_tag": route_tag,
                                        "ip_address": ip,
                                        "subnet_mask": subnet_mask,
                                        "next_hop": next_hop,
                                        "metric": metric
                                    }));
                                }
                            }

                            // Create RIP request event
                            let event_data = serde_json::json!({
                                "command": command,
                                "version": version,
                                "message_type": message_type,
                                "routes": routes,
                                "peer_address": peer_addr.to_string(),
                                "bytes_received": data.len()
                            });

                            let event = Event::new(&RIP_REQUEST_EVENT, event_data);

                            debug!("RIP calling LLM for {} from {}", message_type, peer_addr);
                            let _ = status_clone.send(format!("[DEBUG] RIP calling LLM for {} from {}", message_type, peer_addr));

                            match call_llm(
                                &llm_clone,
                                &state_clone,
                                server_id,
                                None,  // RIP uses UDP, no persistent connection
                                &event,
                                protocol_clone.as_ref(),
                            ).await {
                                Ok(execution_result) => {
                                    // Display messages from LLM
                                    for message in &execution_result.messages {
                                        info!("{}", message);
                                        let _ = status_clone.send(format!("[INFO] {}", message));
                                    }

                                    debug!("RIP parsed {} actions", execution_result.raw_actions.len());
                                    let _ = status_clone.send(format!("[DEBUG] RIP parsed {} actions", execution_result.raw_actions.len()));

                                    // Process protocol results
                                    debug!("RIP got {} protocol results", execution_result.protocol_results.len());
                                    let _ = status_clone.send(format!("[DEBUG] RIP got {} protocol results", execution_result.protocol_results.len()));

                                    for protocol_result in execution_result.protocol_results {
                                        if let Some(output_data) = protocol_result.get_all_output().first() {
                                            let _ = socket_clone.send_to(output_data, peer_addr).await;

                                            // DEBUG: Log summary
                                            debug!("RIP sent {} bytes to {}", output_data.len(), peer_addr);
                                            let _ = status_clone.send(format!("[DEBUG] RIP sent {} bytes to {}", output_data.len(), peer_addr));

                                            // TRACE: Log full payload (hex)
                                            let hex_str = hex::encode(output_data);
                                            trace!("RIP sent (hex): {}", hex_str);
                                            let _ = status_clone.send(format!("[TRACE] RIP sent (hex): {}", hex_str));

                                            let _ = status_clone.send(format!("→ RIP response to {} ({} bytes)", peer_addr, output_data.len()));
                                        } else {
                                            debug!("RIP protocol result has no output data");
                                            let _ = status_clone.send("[DEBUG] RIP protocol result has no output data".to_string());
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("RIP LLM call failed: {}", e);
                                    let _ = status_clone.send(format!("✗ RIP LLM error: {}", e));
                                }
                            }
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "✗ RIP receive error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}
