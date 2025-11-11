//! SVN (Subversion) server implementation
pub mod actions;

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use actions::{SVN_COMMAND_EVENT, SVN_GREETING_EVENT};
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

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

        // INFO: Log lifecycle event
        console_info!(status_tx, "[INFO] SVN server (action-based) listening on {}");

        let protocol = Arc::new(actions::SvnProtocol::new());

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, peer_addr)) => {
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);

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
                                })
                            ),
                        };
                        app_state
                            .add_connection_to_server(server_id, conn_state)
                            .await;
                        console_info!(status_tx, "__UPDATE_UI__");

                        // DEBUG: Log connection summary
                        debug!("SVN client connected from {}", peer_addr);
                        let _ = status_tx
                            .send(format!("[DEBUG] SVN client connected from {}", peer_addr));

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
                        // ERROR: Critical failure
                        console_error!(status_tx, "[ERROR] SVN accept error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

async fn handle_svn_connection(
    mut socket: tokio::net::TcpStream,
    peer_addr: SocketAddr,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    server_id: crate::state::ServerId,
    protocol: Arc<actions::SvnProtocol>,
    connection_id: ConnectionId,
) {
    let (reader, mut writer) = tokio::io::split(&mut socket);
    let mut buf_reader = BufReader::new(reader);

    // Send greeting event to LLM
    let greeting_event = Event::new(&SVN_GREETING_EVENT, serde_json::json!({}));

    console_debug!(status_tx, "[DEBUG] SVN sending greeting to {}", peer_addr);

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
                console_info!(status_tx, "[INFO] {}", message);
            }

            // Send greeting responses
            for protocol_result in execution_result.protocol_results {
                if let crate::llm::actions::protocol_trait::ActionResult::Output(output_data) = protocol_result {
                    if let Err(e) = writer.write_all(&output_data).await {
                        console_error!(status_tx, "[ERROR] SVN write error: {}", e);
                        return;
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

                    console_trace!(status_tx, "[TRACE] SVN sent greeting: {}", String::from_utf8_lossy(&output_data).trim());
                }
            }
        }
        Err(e) => {
            console_error!(status_tx, "✗ SVN LLM error: {}", e);
            return;
        }
    }

    // Main command loop
    let mut buffer = String::new();
    loop {
        buffer.clear();

        match buf_reader.read_line(&mut buffer).await {
            Ok(0) => {
                // DEBUG: Connection closed
                debug!("SVN client {} disconnected", peer_addr);
                let _ = status_tx
                    .send(format!("[DEBUG] SVN client {} disconnected", peer_addr));

                // Update connection status
                use crate::state::server::ConnectionStatus;
                app_state
                    .update_connection_status(server_id, connection_id, ConnectionStatus::Closed)
                    .await;
                console_info!(status_tx, "__UPDATE_UI__");
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

                // DEBUG: Log summary
                console_debug!(status_tx, "[DEBUG] SVN received {} bytes from {}");

                // TRACE: Log full payload
                console_trace!(status_tx, "[TRACE] SVN command: {}", command_line);

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

                // DEBUG: Log LLM call
                console_debug!(status_tx, "[DEBUG] SVN calling LLM for command from {}");

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
                            console_info!(status_tx, "[INFO] {}", message);
                        }

                        // DEBUG: Log protocol results count
                        console_debug!(status_tx, "[DEBUG] SVN got {} protocol results");

                        // Send all outputs to client and check for close
                        let mut should_close = false;
                        for protocol_result in execution_result.protocol_results {
                            match protocol_result {
                                crate::llm::actions::protocol_trait::ActionResult::Output(output_data) => {
                                    if let Err(e) = writer.write_all(&output_data).await {
                                        // ERROR: Write failed
                                        error!("SVN write error: {}", e);
                                        let _ =
                                            status_tx.send(format!("[ERROR] SVN write error: {}", e));
                                        return;
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

                                    // DEBUG: Log summary
                                    console_debug!(status_tx, "[DEBUG] SVN sent {} bytes to {}");

                                    // TRACE: Log full payload
                                    console_trace!(status_tx, "[TRACE] SVN response: {}");

                                    // INFO: User-facing message
                                    console_info!(status_tx, "→ SVN response to {} ({} bytes)");
                                }
                                crate::llm::actions::protocol_trait::ActionResult::CloseConnection => {
                                    should_close = true;
                                    debug!("SVN closing connection per LLM request");
                                    let _ = status_tx
                                        .send("[DEBUG] SVN closing connection per LLM request".to_string());
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
                        // ERROR: LLM call failed
                        console_error!(status_tx, "✗ SVN LLM error: {}", e);
                        break;
                    }
                }
            }
            Err(e) => {
                // ERROR: Read failed
                error!("SVN read error from {}: {}", peer_addr, e);
                let _ =
                    status_tx.send(format!("[ERROR] SVN read error from {}: {}", peer_addr, e));
                break;
            }
        }
    }

    // Update connection status to closed
    use crate::state::server::ConnectionStatus;
    app_state
        .update_connection_status(server_id, connection_id, ConnectionStatus::Closed)
        .await;
    console_info!(status_tx, "__UPDATE_UI__");
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
        let inner = &line[1..line.len()-1];
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
