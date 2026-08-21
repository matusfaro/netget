//! SVN (Subversion) server implementation
pub mod actions;

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::logging::emit::Log;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use actions::{SVN_COMMAND_EVENT, SVN_GREETING_EVENT};
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};

pub struct SvnServer;

impl SvnServer {
    /// Spawn SVN server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let listener = TcpListener::bind(listen_addr).await?;
        let local_addr = listener.local_addr()?;

        Log::new(Some(&status_tx)).info(format!("SVN server listening on {}", local_addr));

        let protocol = Arc::new(actions::SvnProtocol::new());

        let task_registrar = app_state.clone();
        let accept_handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, peer_addr)) => {
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);

                        // Add connection to ServerInstance
                        use crate::state::server::{
                            ConnectionState as ServerConnectionState, ConnectionStatus,
                        };
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
                            protocol_info: crate::state::server::ProtocolConnectionInfo::new(
                                serde_json::json!({
                                    "protocol": "svn",
                                    "authenticated": false,
                                    "repository_url": null,
                                    "commands_processed": 0
                                }),
                            ),
                        };
                        app_state
                            .add_connection_to_server(server_id, conn_state)
                            .await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        Log::new(Some(&status_tx))
                            .info(format!("SVN client connected from {}", peer_addr));

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();
                        let connection_id_clone = connection_id;

                        tokio::spawn(async move {
                            handle_svn_connection(
                                socket,
                                peer_addr,
                                llm_clone,
                                state_clone,
                                status_clone,
                                server_id,
                                protocol_clone,
                                connection_id_clone,
                            )
                            .await
                        });
                    }
                    Err(e) => {
                        Log::new(Some(&status_tx)).error(format!("SVN accept error: {}", e));
                        break;
                    }
                }
            }
        });

        task_registrar
            .register_server_task(server_id, accept_handle)
            .await;

        Ok(local_addr)
    }
}

async fn handle_svn_connection(
    socket: tokio::net::TcpStream,
    peer_addr: SocketAddr,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    server_id: crate::state::ServerId,
    protocol: Arc<actions::SvnProtocol>,
    connection_id: ConnectionId,
) {
    // Split into an owned read half and a shared write half. The write half is an
    // Arc<Mutex<..>> so the reader below and the dashboard's peer-command task both
    // write through the same guarded sink (CLAUDE.md "Connection I/O").
    let (reader, write_half) = tokio::io::split(socket);
    let write_half = Arc::new(Mutex::new(write_half));
    let mut buf_reader = BufReader::new(reader);
    let log = Log::new(Some(&status_tx));

    // Peer messaging: the dashboard can inject an action (send_svn_success,
    // close_connection, ...) into THIS connection through the same executor the
    // model's actions use. The task ends when the handle is dropped by one of the
    // close paths below (each calls remove_peer_handle) or by server teardown.
    let peer_rx = crate::server::peer_support::register_peer_channel(
        &app_state,
        server_id,
        connection_id.as_u32(),
    )
    .await;
    crate::server::peer_support::spawn_peer_command_task(
        peer_rx,
        protocol.clone(),
        app_state.clone(),
        server_id,
        connection_id.as_u32(),
        write_half.clone(),
        status_tx.clone(),
    );

    // Send greeting event to LLM
    let greeting_event = Event::new(&SVN_GREETING_EVENT, serde_json::json!({}));

    log.debug(format!("SVN sending greeting to {}", peer_addr));

    match call_llm(
        &llm_client,
        &app_state,
        server_id,
        Some(connection_id),
        &greeting_event,
        protocol.as_ref(),
    )
    .await
    {
        Ok(execution_result) => {
            // Display messages from LLM
            for message in &execution_result.messages {
                log.info(message);
            }

            // Send greeting responses
            for protocol_result in execution_result.protocol_results {
                if let crate::llm::actions::protocol_trait::ActionResult::Output(output_data) =
                    protocol_result
                {
                    {
                        let mut writer = write_half.lock().await;
                        if let Err(e) = writer.write_all(&output_data).await {
                            log.error(format!("SVN write error: {}", e));
                            drop(writer);
                            app_state
                                .remove_peer_handle(server_id, connection_id.as_u32())
                                .await;
                            return;
                        }
                        let _ = writer.flush().await;
                    }

                    // Update connection stats
                    app_state
                        .update_connection_stats(
                            server_id,
                            connection_id,
                            None,
                            Some(output_data.len() as u64),
                            None,
                            Some(1),
                        )
                        .await;

                    // Full payload FileOnly: the send_svn_* action template already
                    // reports the send to the TUI.
                    log.trace(format!(
                        "SVN sent greeting: {}",
                        String::from_utf8_lossy(&output_data)
                    ));
                }
            }
        }
        Err(e) => {
            // Non-fatal: the greeting handler failed, connection is closed.
            log.warn(format!("SVN LLM call failed during greeting: {}", e));
            app_state
                .remove_peer_handle(server_id, connection_id.as_u32())
                .await;
            return;
        }
    }

    // Main command loop
    let mut buffer = String::new();
    loop {
        buffer.clear();

        match buf_reader.read_line(&mut buffer).await {
            Ok(0) => {
                log.info(format!("SVN client {} disconnected", peer_addr));

                // Update connection status
                use crate::state::server::ConnectionStatus;
                app_state
                    .update_connection_status(server_id, connection_id, ConnectionStatus::Closed)
                    .await;
                let _ = status_tx.send("__UPDATE_UI__".to_string());
                break;
            }
            Ok(n) => {
                // Update connection stats
                app_state
                    .update_connection_stats(
                        server_id,
                        connection_id,
                        Some(n as u64),
                        None,
                        Some(1),
                        None,
                    )
                    .await;

                // Parse SVN command
                let command_line = buffer.trim().to_string();

                // Summary + full payload FileOnly: the svn_command event template
                // renders the equivalent line to the TUI.
                log.debug(format!("SVN received {} bytes from {}", n, peer_addr));
                log.trace(format!("SVN command: {}", command_line));

                // Parse SVN protocol command
                let parsed_command = parse_svn_command(&command_line);

                // Create event
                let event = Event::new(
                    &SVN_COMMAND_EVENT,
                    serde_json::json!({
                        "command_line": command_line,
                        "command": parsed_command.command,
                        "args": parsed_command.args,
                    }),
                );

                log.debug(format!("SVN calling LLM for command from {}", peer_addr));

                // Call LLM
                match call_llm(
                    &llm_client,
                    &app_state,
                    server_id,
                    Some(connection_id),
                    &event,
                    protocol.as_ref(),
                )
                .await
                {
                    Ok(execution_result) => {
                        // Display messages from LLM
                        for message in &execution_result.messages {
                            log.info(message);
                        }

                        log.debug(format!(
                            "SVN got {} protocol results",
                            execution_result.protocol_results.len()
                        ));

                        // Send all outputs to client and check for close
                        let mut should_close = false;
                        for protocol_result in execution_result.protocol_results {
                            match protocol_result {
                                crate::llm::actions::protocol_trait::ActionResult::Output(output_data) => {
                                    {
                                        let mut writer = write_half.lock().await;
                                        if let Err(e) = writer.write_all(&output_data).await {
                                            log.error(format!("SVN write error: {}", e));
                                            drop(writer);
                                            app_state
                                                .remove_peer_handle(
                                                    server_id,
                                                    connection_id.as_u32(),
                                                )
                                                .await;
                                            return;
                                        }
                                        let _ = writer.flush().await;
                                    }

                                    // Update connection stats
                                    app_state
                                        .update_connection_stats(
                                            server_id,
                                            connection_id,
                                            None,
                                            Some(output_data.len() as u64),
                                            None,
                                            Some(1),
                                        )
                                        .await;

                                    // Summary + full payload FileOnly: the send_svn_*
                                    // action template already reports the send to the TUI.
                                    log.debug(format!(
                                        "SVN sent {} bytes to {}",
                                        output_data.len(),
                                        peer_addr
                                    ));
                                    log.trace(format!(
                                        "SVN response: {}",
                                        String::from_utf8_lossy(&output_data)
                                    ));
                                }
                                crate::llm::actions::protocol_trait::ActionResult::CloseConnection => {
                                    should_close = true;
                                    log.debug("SVN closing connection per LLM request");
                                }
                                _ => {} // Ignore other action results
                            }
                        }

                        // Break loop if LLM requested connection close
                        if should_close {
                            break;
                        }
                    }
                    Err(e) => {
                        // Non-fatal: the command handler failed, connection is closed.
                        log.warn(format!("SVN LLM call failed: {}", e));
                        break;
                    }
                }
            }
            Err(e) => {
                log.error(format!("SVN read error from {}: {}", peer_addr, e));
                break;
            }
        }
    }

    // Update connection status to closed. Reached by every loop `break` (EOF,
    // read error, close_connection, LLM failure), so this is the one place the
    // peer handle must be dropped — otherwise the rail keeps offering
    // "message this peer" on a dead connection.
    use crate::state::server::ConnectionStatus;
    app_state
        .remove_peer_handle(server_id, connection_id.as_u32())
        .await;
    app_state
        .update_connection_status(server_id, connection_id, ConnectionStatus::Closed)
        .await;
    let _ = status_tx.send("__UPDATE_UI__".to_string());
}

#[derive(Debug, Clone)]
struct ParsedSvnCommand {
    command: String,
    args: Vec<String>,
}

/// Parse SVN protocol command from line
/// SVN protocol uses S-expression-like format: ( command args... )
fn parse_svn_command(line: &str) -> ParsedSvnCommand {
    let line = line.trim();

    // Simple parser for SVN protocol format
    if line.starts_with('(') && line.ends_with(')') {
        let inner = &line[1..line.len() - 1];
        let parts: Vec<String> = inner.split_whitespace().map(String::from).collect();

        if parts.is_empty() {
            ParsedSvnCommand {
                command: String::new(),
                args: Vec::new(),
            }
        } else {
            ParsedSvnCommand {
                command: parts[0].clone(),
                args: parts[1..].to_vec(),
            }
        }
    } else {
        // Not a valid SVN command format, return as-is
        ParsedSvnCommand {
            command: line.to_string(),
            args: Vec::new(),
        }
    }
}
