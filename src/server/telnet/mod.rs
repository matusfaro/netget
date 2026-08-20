//! Telnet server implementation
pub mod actions;

use crate::server::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::logging::emit::Log;
use crate::protocol::Event;
use crate::server::TelnetProtocol;
use crate::state::app_state::AppState;
use actions::{TELNET_CONNECTION_OPENED_EVENT, TELNET_MESSAGE_RECEIVED_EVENT};

/// Truncate `text` to at most `max` bytes without splitting a UTF-8 character.
///
/// Slicing `&text[..max]` directly panics when the boundary lands inside a multi-byte
/// character, and both the received line and the handler's response are attacker- or
/// model-controlled, so that panic is reachable from the network.
#[cfg(feature = "telnet")]
fn preview(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}

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
        send_first: bool,
    ) -> Result<SocketAddr> {
        let listener =
            crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        Log::new(Some(&status_tx)).info(format!("Telnet server listening on {}", local_addr));

        let protocol = Arc::new(TelnetProtocol::new());

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
                            let log = Log::new(Some(&status_clone));

                            // If the server was started with send_first, give the handler a
                            // chance to greet before the client says anything. Without this
                            // Telnet has no connect-time event at all and cannot show a
                            // login banner or prompt.
                            if send_first {
                                let event = Event::new(
                                    &TELNET_CONNECTION_OPENED_EVENT,
                                    serde_json::json!({}),
                                );
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
                                        for protocol_result in execution_result.protocol_results {
                                            if let ActionResult::Output(data) = protocol_result {
                                                use tokio::io::AsyncWriteExt;
                                                let mut write = write_half_arc.lock().await;
                                                let _ = write.write_all(&data).await;
                                                let _ = write.flush().await;
                                                log.debug(format!(
                                                    "Telnet sent greeting ({} bytes) on connection {}",
                                                    data.len(),
                                                    connection_id
                                                ));
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        // The client asked for a banner and is sitting at a
                                        // blank screen. Telnet has no error frame - it is a
                                        // byte stream with a human on the other end - so the
                                        // protocol-appropriate answer is a plain notice line.
                                        // Non-fatal: a wire notice is still delivered
                                        // (fallback), so this is WARN not ERROR.
                                        log.warn(format!(
                                            "Telnet greeting handler failed on connection {}: {}",
                                            connection_id, e
                                        ));
                                        let notice = telnet_failure_notice(&e);
                                        use tokio::io::AsyncWriteExt;
                                        let mut write = write_half_arc.lock().await;
                                        let _ = write.write_all(notice.as_bytes()).await;
                                        let _ = write.flush().await;
                                    }
                                }
                            }

                            // Line-based reading. Note: Telnet option negotiation (IAC) is not
                            // implemented, so negotiation bytes are delivered as ordinary data.
                            use tokio::io::{AsyncBufReadExt, BufReader};
                            let mut reader = BufReader::new(read_half);
                            let mut line = String::new();
                            let mut close_requested = false;

                            loop {
                                let n = match reader.read_line(&mut line).await {
                                    Ok(n) => n,
                                    Err(e) => {
                                        // read_line fails on non-UTF-8 input; log it instead of
                                        // treating it as a clean disconnect.
                                        log.debug(format!(
                                            "Telnet read error on connection {}: {}",
                                            connection_id, e
                                        ));
                                        break;
                                    }
                                };
                                if n == 0 {
                                    break;
                                }

                                // Summary + full payload are FileOnly: the
                                // telnet_message_received event template renders the
                                // equivalent line to the TUI, so streaming it here too
                                // would duplicate it and load the unbounded status channel.
                                let line_preview = preview(&line, 100);
                                log.debug(format!(
                                    "Telnet received {} bytes on connection {}: {}",
                                    n,
                                    connection_id,
                                    line_preview.trim()
                                ));
                                log.trace(format!("Telnet data (text): {:?}", line.trim()));

                                let event = Event::new(
                                    &TELNET_MESSAGE_RECEIVED_EVENT,
                                    serde_json::json!({
                                        "message": line.trim()
                                    }),
                                );

                                log.debug(format!(
                                    "Telnet calling LLM for connection {}",
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
                                            log.info(message);
                                        }

                                        log.debug(format!(
                                            "Telnet got {} protocol results",
                                            execution_result.protocol_results.len()
                                        ));

                                        for protocol_result in execution_result.protocol_results {
                                            match protocol_result {
                                                ActionResult::Output(data) => {
                                                    let mut write = write_half_arc.lock().await;

                                                    // Write the action's bytes verbatim. Going
                                                    // via String::from_utf8_lossy would replace
                                                    // any non-UTF-8 byte with U+FFFD before it
                                                    // reached the wire.
                                                    use tokio::io::AsyncWriteExt;
                                                    let _ = write.write_all(&data).await;
                                                    let _ = write.flush().await;
                                                    drop(write);

                                                    let response = String::from_utf8_lossy(&data);

                                                    // Summary + full payload FileOnly: the
                                                    // send_telnet_* action template already
                                                    // reports the send to the TUI.
                                                    let response_preview = preview(&response, 100);
                                                    log.debug(format!(
                                                        "Telnet sent {} bytes on connection {}: {}",
                                                        data.len(),
                                                        connection_id,
                                                        response_preview.trim()
                                                    ));
                                                    log.trace(format!(
                                                        "Telnet sent (text): {:?}",
                                                        response.trim()
                                                    ));
                                                }
                                                // Only flags the intent: `break` here would
                                                // leave the read loop running and the socket
                                                // open, so close_connection did nothing.
                                                ActionResult::CloseConnection => {
                                                    close_requested = true;
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        // Same reasoning as the greeting: a Telnet client that
                                        // sent a line and gets nothing back cannot tell a
                                        // broken server from a slow one. A notice is not a
                                        // prompt and not a shell result, so nothing downstream
                                        // can read it as the command having run.
                                        // Non-fatal: a wire notice is still delivered
                                        // (fallback), so this is WARN not ERROR.
                                        log.warn(format!(
                                            "Telnet LLM call failed on connection {}: {}",
                                            connection_id, e
                                        ));
                                        let notice = telnet_failure_notice(&e);
                                        use tokio::io::AsyncWriteExt;
                                        let mut write = write_half_arc.lock().await;
                                        let _ = write.write_all(notice.as_bytes()).await;
                                        let _ = write.flush().await;
                                    }
                                }
                                line.clear();

                                if close_requested {
                                    log.info(format!(
                                        "Telnet connection {} closed by handler",
                                        connection_id
                                    ));
                                    use tokio::io::AsyncWriteExt;
                                    let mut write = write_half_arc.lock().await;
                                    let _ = write.shutdown().await;
                                    break;
                                }
                            }

                            // Connection closed - mark as closed
                            state_clone
                                .close_connection_on_server(server_id, connection_id)
                                .await;
                            let _ = status_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        Log::new(Some(&status_tx))
                            .error(format!("Failed to accept Telnet connection: {}", e));
                        break;
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

/// The line to print when the LLM backend fails.
///
/// Telnet is an unstructured byte stream: there is no status code to send and no framing a
/// client could key off, so the only useful thing to do is tell whoever is on the other end,
/// in words, that the server could not answer - and to say so on its own line so it cannot be
/// mistaken for the output of whatever they typed.
///
/// The text is a category, never the error: see `crate::utils::wire_failure`. This is the
/// path that put netget's own retry message on a stranger's terminal.
///
/// CRLF because a raw Telnet client is usually in a mode where a bare LF does not return the
/// carriage, which would leave the message stair-stepping across the terminal.
#[cfg(feature = "telnet")]
fn telnet_failure_notice(err: &anyhow::Error) -> String {
    format!(
        "\r\n[netget] {}\r\n",
        crate::utils::WireFailure::classify(err).text()
    )
}
