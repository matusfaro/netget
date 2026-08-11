//! NNTP server implementation
pub mod actions;

use crate::server::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::console_error;
use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::NntpProtocol;
use crate::state::app_state::AppState;
use actions::NNTP_COMMAND_RECEIVED_EVENT;

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
        let listener =
            crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("NNTP server (action-based) listening on {}", local_addr);

        let protocol = Arc::new(NntpProtocol::new());

        let task_registrar = app_state.clone();
        let accept_handle = tokio::spawn(async move {
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

                            // Send initial greeting
                            debug!("NNTP sending greeting to connection {}", connection_id);
                            let _ = status_clone.send(format!(
                                "[DEBUG] NNTP sending greeting to connection {}",
                                connection_id
                            ));

                            let greeting_event = Event::new(
                                &NNTP_COMMAND_RECEIVED_EVENT,
                                serde_json::json!({
                                    "command": "GREETING"
                                }),
                            );
                            match call_llm(
                                &llm_clone,
                                &state_clone,
                                server_id,
                                Some(connection_id),
                                &greeting_event,
                                protocol_clone.as_ref(),
                            )
                            .await
                            {
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
                                            let preview =
                                                crate::utils::truncate_for_log(&response, 100);
                                            debug!(
                                                "NNTP sent {} bytes on connection {}: {}",
                                                data.len(),
                                                connection_id,
                                                preview.trim()
                                            );
                                            let _ = status_clone.send(format!(
                                                "[DEBUG] NNTP sent {} bytes on connection {}: {}",
                                                data.len(),
                                                connection_id,
                                                preview.trim()
                                            ));

                                            // TRACE: Log full payload
                                            trace!("NNTP sent (text): {:?}", response.trim());
                                            let _ = status_clone.send(format!(
                                                "[TRACE] NNTP sent (text): {:?}",
                                                response.trim()
                                            ));
                                        }
                                    }
                                }
                                Err(e) => {
                                    // RFC 3977 5.1: the initial greeting may be `400 service
                                    // temporarily unavailable`, after which the server closes.
                                    // Writing nothing left the client blocked reading a
                                    // greeting line that was never coming.
                                    error!(
                                        "NNTP greeting LLM error on connection {}: {}",
                                        connection_id, e
                                    );
                                    let line = nntp_greeting_failure_line(&e);
                                    let _ = status_clone.send(format!(
                                        "[ERROR] NNTP connection {} refused: {}",
                                        connection_id,
                                        line.trim_end()
                                    ));
                                    {
                                        let mut write = write_half_arc.lock().await;
                                        let _ = write.write_all(line.as_bytes()).await;
                                        let _ = write.flush().await;
                                        let _ = write.shutdown().await;
                                    }
                                    state_clone
                                        .remove_connection_from_server(server_id, connection_id)
                                        .await;
                                    let _ = status_clone.send("__UPDATE_UI__".to_string());
                                    return;
                                }
                            }

                            // Read commands from client
                            let mut reader = BufReader::new(read_half);
                            let mut line = String::new();

                            while let Ok(n) = reader.read_line(&mut line).await {
                                if n == 0 {
                                    break;
                                }

                                // DEBUG: Log summary with text preview
                                let preview = crate::utils::truncate_for_log(&line, 100);
                                debug!(
                                    "NNTP received {} bytes on connection {}: {}",
                                    n,
                                    connection_id,
                                    preview.trim()
                                );
                                let _ = status_clone.send(format!(
                                    "[DEBUG] NNTP received {} bytes on connection {}: {}",
                                    n,
                                    connection_id,
                                    preview.trim()
                                ));

                                // TRACE: Log full text payload
                                trace!("NNTP data (text): {:?}", line.trim());
                                let _ = status_clone
                                    .send(format!("[TRACE] NNTP data (text): {:?}", line.trim()));

                                let event = Event::new(
                                    &NNTP_COMMAND_RECEIVED_EVENT,
                                    serde_json::json!({
                                        "command": line.trim()
                                    }),
                                );

                                debug!("NNTP calling LLM for connection {}", connection_id);
                                let _ = status_clone.send(format!(
                                    "[DEBUG] NNTP calling LLM for connection {}",
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
                                            "NNTP got {} protocol results",
                                            execution_result.protocol_results.len()
                                        );
                                        let _ = status_clone.send(format!(
                                            "[DEBUG] NNTP got {} protocol results",
                                            execution_result.protocol_results.len()
                                        ));

                                        for protocol_result in execution_result.protocol_results {
                                            match protocol_result {
                                                ActionResult::Output(data) => {
                                                    let mut write = write_half_arc.lock().await;
                                                    let _ = write.write_all(&data).await;
                                                    let _ = write.flush().await;

                                                    // DEBUG: Log summary with text preview
                                                    let response = String::from_utf8_lossy(&data);
                                                    let preview = crate::utils::truncate_for_log(
                                                        &response, 100,
                                                    );
                                                    debug!(
                                                        "NNTP sent {} bytes on connection {}: {}",
                                                        data.len(),
                                                        connection_id,
                                                        preview.trim()
                                                    );
                                                    let _ = status_clone.send(format!("[DEBUG] NNTP sent {} bytes on connection {}: {}", data.len(), connection_id, preview.trim()));

                                                    // TRACE: Log full text payload
                                                    trace!(
                                                        "NNTP sent (text): {:?}",
                                                        response.trim()
                                                    );
                                                    let _ = status_clone.send(format!(
                                                        "[TRACE] NNTP sent (text): {:?}",
                                                        response.trim()
                                                    ));
                                                }
                                                ActionResult::CloseConnection => break,
                                                _ => {}
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        // NNTP is strictly one response line per command, so
                                        // silence desynchronises the client for the rest of
                                        // the session. 403 is RFC 3977's "internal fault or
                                        // problem preventing action being taken" and is a
                                        // refusal on every command NNTP has, AUTHINFO
                                        // included - there is no way for this path to
                                        // authenticate anyone or hand back an article.
                                        error!(
                                            "NNTP LLM error on connection {}: {}",
                                            connection_id, e
                                        );
                                        let (reply, close) = nntp_command_failure_line(&e);
                                        let _ = status_clone.send(format!(
                                            "[ERROR] NNTP connection {} replying: {}",
                                            connection_id,
                                            reply.trim_end()
                                        ));
                                        {
                                            let mut write = write_half_arc.lock().await;
                                            let _ = write.write_all(reply.as_bytes()).await;
                                            let _ = write.flush().await;
                                            if close {
                                                let _ = write.shutdown().await;
                                            }
                                        }
                                        if close {
                                            break;
                                        }
                                    }
                                }

                                line.clear();
                            }

                            debug!("NNTP connection {} closed", connection_id);
                            let _ = status_clone
                                .send(format!("[DEBUG] NNTP connection {} closed", connection_id));

                            // Remove connection from server instance
                            state_clone
                                .remove_connection_from_server(server_id, connection_id)
                                .await;
                            let _ = status_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "Failed to accept NNTP connection: {}", e);
                    }
                }
            }
        });

        // Register the accept loop so stop_server can abort it and release the port.
        task_registrar
            .register_server_task(server_id, accept_handle)
            .await;

        Ok(local_addr)
    }
}

/// Sanitise an error for use as the free text of a single NNTP response line.
///
/// A response is one CRLF-terminated line, so an embedded newline in the error would forge a
/// second response and desynchronise the client for the rest of the session.
fn nntp_reason(err: &anyhow::Error) -> String {
    crate::utils::truncate_for_log(&err.to_string(), 200).replace(['\r', '\n'], " ")
}

/// The greeting to write when the LLM backend cannot produce one.
///
/// RFC 3977 5.1 defines exactly two failure greetings: 400 (temporarily unavailable) and 502
/// (permanently unavailable). Neither can be mistaken for the 200/201 that means "ready", so
/// a client can never read this as a usable session.
fn nntp_greeting_failure_line(err: &anyhow::Error) -> String {
    let reason = nntp_reason(err);
    if crate::llm::is_overload_error(err) {
        format!("400 netget: backend at capacity, retry later ({reason})\r\n")
    } else {
        format!("400 netget: service temporarily unavailable ({reason})\r\n")
    }
}

/// The response line for a command the LLM backend failed to answer, and whether to close.
///
/// 403 is RFC 3977's generic "internal fault or problem preventing action being taken" and
/// leaves the session usable, which is what a one-off failure deserves. Capacity exhaustion
/// gets 400 instead - "service discontinued", which per RFC 3977 3.1 the server follows by
/// closing the connection - because telling a client to come back later is more useful than
/// letting it hammer a backend that is already saturated.
fn nntp_command_failure_line(err: &anyhow::Error) -> (String, bool) {
    let reason = nntp_reason(err);
    if crate::llm::is_overload_error(err) {
        (
            format!("400 netget: backend at capacity, retry later ({reason})\r\n"),
            true,
        )
    } else {
        (format!("403 netget: {reason}\r\n"), false)
    }
}
