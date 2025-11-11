//! XMPP server implementation
pub mod actions;

use crate::server::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use actions::{XmppProtocol, XMPP_DATA_RECEIVED_EVENT};
use crate::protocol::Event;
use crate::state::app_state::AppState;

/// XMPP server that forwards XML stanzas to LLM
pub struct XmppServer;

impl XmppServer {
    /// Spawn XMPP server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let listener = crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        console_info!(status_tx, "[INFO] XMPP server listening on {}", local_addr);

        let protocol = Arc::new(XmppProtocol::new());

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

                        debug!("XMPP connection {} from {}", connection_id, remote_addr);
                        let _ = status_clone.send(format!("[DEBUG] XMPP connection {} from {}", connection_id, remote_addr));

                        tokio::spawn(async move {
                            let (read_half, write_half) = tokio::io::split(stream);
                            let write_half_arc = Arc::new(tokio::sync::Mutex::new(write_half));

                            // Add connection to ServerInstance
                            use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
use crate::{console_trace, console_debug, console_info, console_warn, console_error};
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

                            // Create XML buffer for streaming parsing
                            let mut read_half = read_half;
                            let mut buffer = Vec::new();
                            let mut temp_buf = vec![0u8; 4096];

                            loop {
                                match read_half.read(&mut temp_buf).await {
                                    Ok(0) => {
                                        debug!("XMPP connection {} closed by client", connection_id);
                                        let _ = status_clone.send(format!("[DEBUG] XMPP connection {} closed", connection_id));
                                        break;
                                    }
                                    Ok(n) => {
                                        buffer.extend_from_slice(&temp_buf[..n]);

                                        // DEBUG: Log summary
                                        debug!("XMPP received {} bytes on connection {}", n, connection_id);
                                        let _ = status_clone.send(format!("[DEBUG] XMPP received {} bytes on connection {}", n, connection_id));

                                        // TRACE: Log full XML data
                                        let xml_str = String::from_utf8_lossy(&buffer);
                                        trace!("XMPP data (XML): {}", xml_str);
                                        let _ = status_clone.send(format!("[TRACE] XMPP data (XML): {}", xml_str));

                                        // Try to parse XML stanzas from buffer
                                        // For simplicity, we'll pass the entire buffer to LLM for parsing
                                        // A more sophisticated implementation would parse individual stanzas

                                        let xml_data = String::from_utf8_lossy(&buffer).to_string();

                                        // Create event for LLM
                                        let event = Event::new(&XMPP_DATA_RECEIVED_EVENT, serde_json::json!({
                                            "xml_data": xml_data
                                        }));

                                        debug!("XMPP calling LLM for connection {}", connection_id);
                                        let _ = status_clone.send(format!("[DEBUG] XMPP calling LLM for connection {}", connection_id));

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

                                                debug!("XMPP got {} protocol results", execution_result.protocol_results.len());
                                                let _ = status_clone.send(format!("[DEBUG] XMPP got {} protocol results", execution_result.protocol_results.len()));

                                                let mut should_close = false;

                                                for protocol_result in execution_result.protocol_results {
                                                    match protocol_result {
                                                        ActionResult::Output(data) => {
                                                            let xml_str = String::from_utf8_lossy(&data);
                                                            let mut write = write_half_arc.lock().await;
                                                            let _ = write.write_all(&data).await;
                                                            let _ = write.flush().await;
                                                            drop(write);

                                                            // DEBUG: Log summary
                                                            debug!("XMPP sent {} bytes on connection {}", data.len(), connection_id);
                                                            let _ = status_clone.send(format!("[DEBUG] XMPP sent {} bytes on connection {}", data.len(), connection_id));

                                                            // TRACE: Log full XML
                                                            trace!("XMPP sent (XML): {}", xml_str);
                                                            let _ = status_clone.send(format!("[TRACE] XMPP sent (XML): {}", xml_str));
                                                        }
                                                        ActionResult::CloseConnection => {
                                                            should_close = true;
                                                        }
                                                        ActionResult::WaitForMore => {
                                                            // Keep buffer and wait for more data
                                                            debug!("XMPP waiting for more data");
                                                            let _ = status_clone.send("[DEBUG] XMPP waiting for more data".to_string());
                                                        }
                                                        _ => {}
                                                    }
                                                }

                                                if should_close {
                                                    break;
                                                }

                                                // Clear buffer after successful processing
                                                buffer.clear();
                                            }
                                            Err(e) => {
                                                error!("XMPP LLM call failed: {}", e);
                                                let _ = status_clone.send(format!("[ERROR] XMPP LLM error: {}", e));
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("XMPP read error on connection {}: {}", connection_id, e);
                                        let _ = status_clone.send(format!("[ERROR] XMPP read error: {}", e));
                                        break;
                                    }
                                }
                            }

                            // Connection closed - mark as closed
                            state_clone.close_connection_on_server(server_id, connection_id).await;
                            let _ = status_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "[ERROR] Failed to accept XMPP connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}
