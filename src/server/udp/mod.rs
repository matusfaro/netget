//! UDP server implementation for raw UDP stack
pub mod actions;

use crate::server::connection::ConnectionId;
use actions::UdpProtocol;
use anyhow::Result;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use actions::UDP_DATAGRAM_RECEIVED_EVENT;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// UDP server that manages UDP connections
pub struct UdpServer;

impl UdpServer {
    /// Spawn UDP server with action-based LLM handling
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        info!("UDP server listening on {} (action-based)", local_addr);

        let protocol = Arc::new(UdpProtocol::with_socket(socket.clone()));

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 65535]; // Maximum UDP datagram size

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);

                        // Add connection to ServerInstance (UDP "connection" = recent peer)
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
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        // DEBUG: Log summary with data preview
                        if data.iter().all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace()) {
                            let data_str = String::from_utf8_lossy(&data);
                            let preview = if data_str.len() > 100 {
                                format!("{}...", &data_str[..100])
                            } else {
                                data_str.to_string()
                            };
                            console_debug!(status_tx, "UDP received {} bytes from {}: {}", n, peer_addr, preview);

                            // TRACE: Log full text payload
                            console_trace!(status_tx, "UDP data (text): {:?}", data_str);
                        } else {
                            console_debug!(status_tx, "UDP received {} bytes from {} (binary data)", n, peer_addr);

                            // TRACE: Log full hex payload
                            let hex_str = hex::encode(&data);
                            console_trace!(status_tx, "UDP data (hex): {}", hex_str);
                        }

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            // Build event data with data preview
                            let data_preview = if data.len() > 200 {
                                format!("{:?}...", &data[..200])
                            } else {
                                format!("{:?}", data)
                            };

                            let event = Event::new(&UDP_DATAGRAM_RECEIVED_EVENT, serde_json::json!({
                                "peer_address": peer_addr.to_string(),
                                "data_length": data.len(),
                                "data_preview": data_preview
                            }));

                            debug!("UDP calling LLM for datagram from {}", peer_addr);
                            let _ = status_clone.send(format!("[DEBUG] UDP calling LLM for datagram from {}", peer_addr));

                            match call_llm(
                                &llm_clone,
                                &state_clone,
                                server_id,
                                None,
                                &event,
                                protocol_clone.as_ref(),
                            ).await {
                                Ok(execution_result) => {
                                    for message in &execution_result.messages {
                                        info!("{}", message);
                                        let _ = status_clone.send(format!("[INFO] {}", message));
                                    }

                                    debug!("UDP got {} protocol results", execution_result.protocol_results.len());
                                    let _ = status_clone.send(format!("[DEBUG] UDP got {} protocol results", execution_result.protocol_results.len()));

                                    for protocol_result in execution_result.protocol_results {
                                        if let Some(output_data) = protocol_result.get_all_output().first() {
                                            if let Err(e) = socket_clone.send_to(output_data, peer_addr).await {
                                                error!("Failed to send UDP response: {}", e);
                                            } else {
                                                // DEBUG: Log summary with data preview
                                                if output_data.iter().all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace()) {
                                                    let data_str = String::from_utf8_lossy(output_data);
                                                    let preview = if data_str.len() > 100 {
                                                        format!("{}...", &data_str[..100])
                                                    } else {
                                                        data_str.to_string()
                                                    };
                                                    debug!("UDP sent {} bytes to {}: {}", output_data.len(), peer_addr, preview);
                                                    let _ = status_clone.send(format!("[DEBUG] UDP sent {} bytes to {}: {}", output_data.len(), peer_addr, preview));

                                                    // TRACE: Log full text payload
                                                    trace!("UDP sent (text): {:?}", data_str);
                                                    let _ = status_clone.send(format!("[TRACE] UDP sent (text): {:?}", data_str));
                                                } else {
                                                    debug!("UDP sent {} bytes to {} (binary data)", output_data.len(), peer_addr);
                                                    let _ = status_clone.send(format!("[DEBUG] UDP sent {} bytes to {} (binary data)", output_data.len(), peer_addr));

                                                    // TRACE: Log full hex payload
                                                    let hex_str = hex::encode(output_data);
                                                    trace!("UDP sent (hex): {}", hex_str);
                                                    let _ = status_clone.send(format!("[TRACE] UDP sent (hex): {}", hex_str));
                                                }

                                                let _ = status_clone.send(format!(
                                                    "→ UDP response to {} ({} bytes)",
                                                    peer_addr, output_data.len()
                                                ));
                                            }
                                        } else {
                                            debug!("UDP protocol result has no output data");
                                            let _ = status_clone.send("[DEBUG] UDP protocol result has no output data".to_string());
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("UDP LLM call failed: {}", e);
                                    let _ = status_clone.send(format!("✗ UDP LLM error: {}", e));
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("UDP receive error: {}", e);
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Shared UDP socket for sending responses
pub type SharedUdpSocket = Arc<Mutex<Arc<UdpSocket>>>;

/// Map from connection ID to peer address for UDP responses
pub type UdpPeerMap = Arc<Mutex<HashMap<ConnectionId, SocketAddr>>>;
