//! NNTP server implementation
pub mod actions;

use crate::server::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use actions::NNTP_COMMAND_RECEIVED_EVENT;
use crate::server::NntpProtocol;
use crate::protocol::Event;
use crate::state::app_state::AppState;

/// NNTP server that forwards commands to LLM
pub struct NntpServer;

impl NntpServer {
    /// Spawn NNTP server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let listener = crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("NNTP server (action-based) listening on {}", local_addr);

        let protocol = Arc::new(NntpProtocol::new());

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            let (read_half, write_half) = tokio::io::split(stream);
                            let write_half_arc = Arc::new(tokio::sync::Mutex::new(write_half));

                            // Add connection to ServerInstance
                            use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
                            let now = std::time::Instant::now();
                            let conn_state = ServerConnectionState {
                                id: connection_id,
                                remote_addr,
                                local_addr: local_addr_conn,
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

                            // Send initial greeting
                            debug!("NNTP sending greeting to connection {}", connection_id);
                            let _ = status_clone.send(format!("[DEBUG] NNTP sending greeting to connection {}", connection_id));

                            let greeting_event = Event::new(&NNTP_COMMAND_RECEIVED_EVENT, serde_json::json!({
                                "command": "GREETING"
                            }));
                            match call_llm(
                                &llm_clone,
                                &state_clone,
                                server_id,
                                Some(connection_id),
                                &greeting_event,
                                protocol_clone.as_ref(),
                            ).await {
                                Ok(execution_result) => {
                                    for message in &execution_result.messages {
                                        info!("{}", message);
                                        let _ = status_clone.send(format!("[INFO] {}", message));
                                    }

                                    for protocol_result in execution_result.protocol_results {
                                        if let ActionResult::Output(data) = protocol_result {
                                            let mut write = write_half_arc.lock().await;
                                            let _ = write.write_all(&data).await;
                                            let _ = write.flush().await;

                                            // DEBUG: Log summary
                                            let response = String::from_utf8_lossy(&data);
                                            let preview = if response.len() > 100 {
                                                format!("{}...", &response[..100])
                                            } else {
                                                response.to_string()
                                            };
                                            debug!("NNTP sent {} bytes on connection {}: {}", data.len(), connection_id, preview.trim());
                                            let _ = status_clone.send(format!("[DEBUG] NNTP sent {} bytes on connection {}: {}", data.len(), connection_id, preview.trim()));

                                            // TRACE: Log full payload
                                            trace!("NNTP sent (text): {:?}", response.trim());
                                            let _ = status_clone.send(format!("[TRACE] NNTP sent (text): {:?}", response.trim()));
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("NNTP greeting LLM error on connection {}: {}", connection_id, e);
                                    let _ = status_clone.send(format!("[ERROR] NNTP greeting LLM error on connection {}: {}", connection_id, e));
                                }
                            }

                            // Read commands from client
                            let mut reader = BufReader::new(read_half);
                            let mut line = String::new();

                            while let Ok(n) = reader.read_line(&mut line).await {
                                if n == 0 { break; }

                                // DEBUG: Log summary with text preview
                                let preview = if line.len() > 100 {
                                    format!("{}...", &line[..100])
                                } else {
                                    line.to_string()
                                };
                                debug!("NNTP received {} bytes on connection {}: {}", n, connection_id, preview.trim());
                                let _ = status_clone.send(format!("[DEBUG] NNTP received {} bytes on connection {}: {}", n, connection_id, preview.trim()));

                                // TRACE: Log full text payload
                                trace!("NNTP data (text): {:?}", line.trim());
                                let _ = status_clone.send(format!("[TRACE] NNTP data (text): {:?}", line.trim()));

                                let event = Event::new(&NNTP_COMMAND_RECEIVED_EVENT, serde_json::json!({
                                    "command": line.trim()
                                }));

                                debug!("NNTP calling LLM for connection {}", connection_id);
                                let _ = status_clone.send(format!("[DEBUG] NNTP calling LLM for connection {}", connection_id));

                                match call_llm(
                                    &llm_clone,
                                    &state_clone,
                                    server_id,
                                    Some(connection_id),
                                    &event,
                                    protocol_clone.as_ref(),
                                ).await {
                                    Ok(execution_result) => {
                                        for message in &execution_result.messages {
                                            info!("{}", message);
                                            let _ = status_clone.send(format!("[INFO] {}", message));
                                        }

                                        debug!("NNTP got {} protocol results", execution_result.protocol_results.len());
                                        let _ = status_clone.send(format!("[DEBUG] NNTP got {} protocol results", execution_result.protocol_results.len()));

                                        for protocol_result in execution_result.protocol_results {
                                            match protocol_result {
                                                ActionResult::Output(data) => {
                                                    let mut write = write_half_arc.lock().await;
                                                    let _ = write.write_all(&data).await;
                                                    let _ = write.flush().await;

                                                    // DEBUG: Log summary with text preview
                                                    let response = String::from_utf8_lossy(&data);
                                                    let preview = if response.len() > 100 {
                                                        format!("{}...", &response[..100])
                                                    } else {
                                                        response.to_string()
                                                    };
                                                    debug!("NNTP sent {} bytes on connection {}: {}", data.len(), connection_id, preview.trim());
                                                    let _ = status_clone.send(format!("[DEBUG] NNTP sent {} bytes on connection {}: {}", data.len(), connection_id, preview.trim()));

                                                    // TRACE: Log full text payload
                                                    trace!("NNTP sent (text): {:?}", response.trim());
                                                    let _ = status_clone.send(format!("[TRACE] NNTP sent (text): {:?}", response.trim()));
                                                }
                                                ActionResult::CloseConnection => break,
                                                _ => {}
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("NNTP LLM error on connection {}: {}", connection_id, e);
                                        let _ = status_clone.send(format!("[ERROR] NNTP LLM error on connection {}: {}", connection_id, e));
                                    }
                                }

                                line.clear();
                            }

                            debug!("NNTP connection {} closed", connection_id);
                            let _ = status_clone.send(format!("[DEBUG] NNTP connection {} closed", connection_id));

                            // Remove connection from server instance
                            state_clone.remove_connection_from_server(server_id, connection_id).await;
                            let _ = status_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept NNTP connection: {}", e);
                        let _ = status_tx.send(format!("[ERROR] Failed to accept NNTP connection: {}", e));
                    }
                }
            }
        });

        Ok(local_addr)
    }
}
