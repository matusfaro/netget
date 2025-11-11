//! Telnet server implementation
pub mod actions;

use crate::server::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::TelnetProtocol;
use crate::state::app_state::AppState;
use actions::TELNET_MESSAGE_RECEIVED_EVENT;

/// Telnet server that forwards messages to LLM
pub struct TelnetServer;

#[cfg(feature = "telnet")]
impl TelnetServer {
    /// Spawn Telnet server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let listener =
            crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("Telnet server (action-based) listening on {}", local_addr);

        let protocol = Arc::new(TelnetProtocol::new());

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            let (read_half, write_half) = tokio::io::split(stream);
                            let write_half_arc = Arc::new(tokio::sync::Mutex::new(write_half));

                            // Add connection to ServerInstance
                            use crate::state::server::{
                                ConnectionState as ServerConnectionState, ConnectionStatus,
                                ProtocolConnectionInfo,
                            };
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
                            state_clone
                                .add_connection_to_server(server_id, conn_state)
                                .await;
                            let _ = status_clone.send("__UPDATE_UI__".to_string());

                            // Use nectar's TelnetCodec with line-based reading
                            use tokio::io::{AsyncBufReadExt, BufReader};
                            let mut reader = BufReader::new(read_half);
                            let mut line = String::new();

                            while let Ok(n) = reader.read_line(&mut line).await {
                                if n == 0 {
                                    break;
                                }

                                // DEBUG: Log summary with text preview
                                let preview = if line.len() > 100 {
                                    format!("{}...", &line[..100])
                                } else {
                                    line.to_string()
                                };
                                debug!(
                                    "Telnet received {} bytes on connection {}: {}",
                                    n,
                                    connection_id,
                                    preview.trim()
                                );
                                let _ = status_clone.send(format!(
                                    "[DEBUG] Telnet received {} bytes on connection {}: {}",
                                    n,
                                    connection_id,
                                    preview.trim()
                                ));

                                // TRACE: Log full text payload
                                trace!("Telnet data (text): {:?}", line.trim());
                                let _ = status_clone
                                    .send(format!("[TRACE] Telnet data (text): {:?}", line.trim()));

                                let event = Event::new(
                                    &TELNET_MESSAGE_RECEIVED_EVENT,
                                    serde_json::json!({
                                        "message": line.trim()
                                    }),
                                );

                                debug!("Telnet calling LLM for connection {}", connection_id);
                                let _ = status_clone.send(format!(
                                    "[DEBUG] Telnet calling LLM for connection {}",
                                    connection_id
                                ));

                                match call_llm(
                                    &llm_clone,
                                    &state_clone,
                                    server_id,
                                    Some(connection_id),
                                    &event,
                                    protocol_clone.as_ref(),
                                )
                                .await
                                {
                                    Ok(execution_result) => {
                                        for message in &execution_result.messages {
                                            info!("{}", message);
                                            let _ =
                                                status_clone.send(format!("[INFO] {}", message));
                                        }

                                        debug!(
                                            "Telnet got {} protocol results",
                                            execution_result.protocol_results.len()
                                        );
                                        let _ = status_clone.send(format!(
                                            "[DEBUG] Telnet got {} protocol results",
                                            execution_result.protocol_results.len()
                                        ));

                                        for protocol_result in execution_result.protocol_results {
                                            match protocol_result {
                                                ActionResult::Output(data) => {
                                                    let response = String::from_utf8_lossy(&data);
                                                    let mut write = write_half_arc.lock().await;

                                                    use tokio::io::AsyncWriteExt;
                                                    let _ =
                                                        write.write_all(response.as_bytes()).await;
                                                    let _ = write.flush().await;

                                                    // DEBUG: Log summary with text preview
                                                    let preview = if response.len() > 100 {
                                                        format!("{}...", &response[..100])
                                                    } else {
                                                        response.to_string()
                                                    };
                                                    debug!(
                                                        "Telnet sent {} bytes on connection {}: {}",
                                                        response.len(),
                                                        connection_id,
                                                        preview.trim()
                                                    );
                                                    let _ = status_clone.send(format!("[DEBUG] Telnet sent {} bytes on connection {}: {}", response.len(), connection_id, preview.trim()));

                                                    // TRACE: Log full text payload
                                                    trace!(
                                                        "Telnet sent (text): {:?}",
                                                        response.trim()
                                                    );
                                                    let _ = status_clone.send(format!(
                                                        "[TRACE] Telnet sent (text): {:?}",
                                                        response.trim()
                                                    ));
                                                }
                                                ActionResult::CloseConnection => break,
                                                _ => {}
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("Telnet LLM call failed: {}", e);
                                        let _ =
                                            status_clone.send(format!("✗ Telnet LLM error: {}", e));
                                    }
                                }
                                line.clear();
                            }

                            // Connection closed - mark as closed
                            state_clone
                                .close_connection_on_server(server_id, connection_id)
                                .await;
                            let _ = status_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept Telnet connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

#[cfg(not(feature = "telnet"))]
impl TelnetServer {
    pub async fn spawn_with_llm_actions(
        _listen_addr: SocketAddr,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        _status_tx: mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        anyhow::bail!("Telnet feature not enabled")
    }
}
