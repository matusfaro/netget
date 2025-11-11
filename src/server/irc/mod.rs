//! IRC server implementation
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
use crate::protocol::Event;
use crate::server::IrcProtocol;
use crate::state::app_state::AppState;
use actions::IRC_MESSAGE_RECEIVED_EVENT;

/// IRC server that forwards messages to LLM
pub struct IrcServer;

impl IrcServer {
    /// Spawn IRC server with integrated LLM actions
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
        info!("IRC server (action-based) listening on {}", local_addr);

        let protocol = Arc::new(IrcProtocol::new());

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
                                    "IRC received {} bytes on connection {}: {}",
                                    n,
                                    connection_id,
                                    preview.trim()
                                );
                                let _ = status_clone.send(format!(
                                    "[DEBUG] IRC received {} bytes on connection {}: {}",
                                    n,
                                    connection_id,
                                    preview.trim()
                                ));

                                // TRACE: Log full text payload
                                trace!("IRC data (text): {:?}", line.trim());
                                let _ = status_clone
                                    .send(format!("[TRACE] IRC data (text): {:?}", line.trim()));

                                let event = Event::new(
                                    &IRC_MESSAGE_RECEIVED_EVENT,
                                    serde_json::json!({
                                        "message": line.trim()
                                    }),
                                );

                                debug!("IRC calling LLM for connection {}", connection_id);
                                let _ = status_clone.send(format!(
                                    "[DEBUG] IRC calling LLM for connection {}",
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
                                            "IRC got {} protocol results",
                                            execution_result.protocol_results.len()
                                        );
                                        let _ = status_clone.send(format!(
                                            "[DEBUG] IRC got {} protocol results",
                                            execution_result.protocol_results.len()
                                        ));

                                        for protocol_result in execution_result.protocol_results {
                                            match protocol_result {
                                                ActionResult::Output(data) => {
                                                    let response = String::from_utf8_lossy(&data);
                                                    let formatted = if response.ends_with("\r\n") {
                                                        response.to_string()
                                                    } else if response.ends_with('\n') {
                                                        format!("{response}\r")
                                                    } else {
                                                        format!("{response}\r\n")
                                                    };
                                                    let mut write = write_half_arc.lock().await;
                                                    let _ =
                                                        write.write_all(formatted.as_bytes()).await;
                                                    let _ = write.flush().await;

                                                    // DEBUG: Log summary with text preview
                                                    let preview = if formatted.len() > 100 {
                                                        format!("{}...", &formatted[..100])
                                                    } else {
                                                        formatted.clone()
                                                    };
                                                    debug!(
                                                        "IRC sent {} bytes on connection {}: {}",
                                                        formatted.len(),
                                                        connection_id,
                                                        preview.trim()
                                                    );
                                                    let _ = status_clone.send(format!("[DEBUG] IRC sent {} bytes on connection {}: {}", formatted.len(), connection_id, preview.trim()));

                                                    // TRACE: Log full text payload
                                                    trace!(
                                                        "IRC sent (text): {:?}",
                                                        formatted.trim()
                                                    );
                                                    let _ = status_clone.send(format!(
                                                        "[TRACE] IRC sent (text): {:?}",
                                                        formatted.trim()
                                                    ));
                                                }
                                                ActionResult::CloseConnection => break,
                                                _ => {}
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("IRC LLM call failed: {}", e);
                                        let _ =
                                            status_clone.send(format!("✗ IRC LLM error: {}", e));
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
                        error!("Failed to accept IRC connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Send an IRC response
pub async fn send_irc_response(
    write_half: &mut tokio::net::tcp::WriteHalf<'_>,
    response: &str,
) -> Result<()> {
    // Ensure IRC messages end with \r\n
    let formatted = if response.ends_with("\r\n") {
        response.to_string()
    } else if response.ends_with('\n') {
        format!("{response}\r")
    } else {
        format!("{response}\r\n")
    };

    write_half.write_all(formatted.as_bytes()).await?;
    write_half.flush().await?;
    Ok(())
}
