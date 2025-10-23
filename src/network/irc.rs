//! IRC server implementation

use crate::network::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::ollama_client::OllamaClient;
use crate::llm::prompt::PromptBuilder;
use crate::llm::{ActionResponse, execute_actions, NetworkContext, ProtocolActions, ActionResult};
use crate::network::IrcProtocol;
use crate::state::app_state::AppState;

/// Get LLM context and output format instructions for IRC stack
pub fn get_llm_protocol_prompt() -> (&'static str, &'static str) {
    let context = r#"You are handling IRC chat protocol (port 6667).
Respond to IRC commands like JOIN, PART, PRIVMSG, NICK, USER, PING, etc.
Use IRC response codes (e.g., 001 for welcome, 332 for topic)."#;

    let output_format = r#"IMPORTANT: Respond with a JSON object:
{
  "output": "IRC response message (null if no response)",
  "close_connection": false,  // Close this connection after sending
  "message": null  // Optional message for user
}"#;

    (context, output_format)
}

/// IRC server that forwards messages to LLM
pub struct IrcServer;

impl IrcServer {
    /// Spawn IRC server with integrated LLM handling
    pub async fn spawn_with_llm(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<SocketAddr> {
        let listener = crate::network::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("IRC server listening on {}", local_addr);

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        let connection_id = ConnectionId::new();
                        info!("Accepted IRC connection {} from {}", connection_id, peer_addr);

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();

                        tokio::spawn(async move {
                            if let Err(e) = handle_irc_with_llm(
                                stream,
                                connection_id,
                                llm_clone,
                                state_clone,
                                status_clone,
                            ).await {
                                error!("IRC connection error: {}", e);
                            }
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

    /// Spawn IRC server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let listener = crate::network::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("IRC server (action-based) listening on {}", local_addr);

        let protocol = Arc::new(IrcProtocol::new());

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new();
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            let (read_half, write_half) = tokio::io::split(stream);
                            let write_half_arc = Arc::new(tokio::sync::Mutex::new(write_half));

                            // Add connection to ServerInstance
                            use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus, ProtocolState};
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
                                protocol_info: ProtocolConnectionInfo::Irc {
                                    write_half: write_half_arc.clone(),
                                    state: ProtocolState::Idle,
                                    queued_data: Vec::new(),
                                },
                            };
                            state_clone.add_connection_to_server(server_id, conn_state).await;
                            let _ = status_clone.send("__UPDATE_UI__".to_string());

                            let mut reader = BufReader::new(read_half);
                            let mut line = String::new();
                            let model = state_clone.get_ollama_model().await;

                            while let Ok(n) = reader.read_line(&mut line).await {
                                if n == 0 { break; }

                                // DEBUG: Log summary with text preview
                                let preview = if line.len() > 100 {
                                    format!("{}...", &line[..100])
                                } else {
                                    line.to_string()
                                };
                                debug!("IRC received {} bytes on connection {}: {}", n, connection_id, preview.trim());
                                let _ = status_clone.send(format!("[DEBUG] IRC received {} bytes on connection {}: {}", n, connection_id, preview.trim()));

                                // TRACE: Log full text payload
                                trace!("IRC data (text): {:?}", line.trim());
                                let _ = status_clone.send(format!("[TRACE] IRC data (text): {:?}", line.trim()));

                                let event_description = format!("IRC message: {}", line.trim());
                                let context = NetworkContext::IrcConnection { connection_id, write_half: write_half_arc.clone(), status_tx: status_clone.clone() };
                                let protocol_actions = protocol_clone.get_sync_actions(&context);
                                let prompt = PromptBuilder::build_network_event_action_prompt(
                                    &state_clone, &event_description, protocol_actions).await;

                                if let Ok(llm_output) = llm_clone.generate(&model, &prompt).await {
                                    if let Ok(action_response) = ActionResponse::from_str(&llm_output) {
                                        if let Ok(result) = execute_actions(action_response.actions, &state_clone,
                                            Some(protocol_clone.as_ref()), Some(&context)).await {
                                            for protocol_result in result.protocol_results {
                                                match protocol_result {
                                                    ActionResult::Output(data) => {
                                                        let response = String::from_utf8_lossy(&data);
                                                        let formatted = if response.ends_with("\r\n") {
                                                            response.to_string()
                                                        } else if response.ends_with('\n') {
                                                            format!("{}\r", response)
                                                        } else {
                                                            format!("{}\r\n", response)
                                                        };
                                                        let mut write = write_half_arc.lock().await;
                                                        let _ = write.write_all(formatted.as_bytes()).await;
                                                        let _ = write.flush().await;

                                                        // DEBUG: Log summary with text preview
                                                        let preview = if formatted.len() > 100 {
                                                            format!("{}...", &formatted[..100])
                                                        } else {
                                                            formatted.clone()
                                                        };
                                                        debug!("IRC sent {} bytes on connection {}: {}", formatted.len(), connection_id, preview.trim());
                                                        let _ = status_clone.send(format!("[DEBUG] IRC sent {} bytes on connection {}: {}", formatted.len(), connection_id, preview.trim()));

                                                        // TRACE: Log full text payload
                                                        trace!("IRC sent (text): {:?}", formatted.trim());
                                                        let _ = status_clone.send(format!("[TRACE] IRC sent (text): {:?}", formatted.trim()));
                                                    }
                                                    ActionResult::CloseConnection => break,
                                                    _ => {}
                                                }
                                            }
                                        }
                                    }
                                }
                                line.clear();
                            }

                            // Connection closed - mark as closed
                            state_clone.close_connection_on_server(server_id, connection_id).await;
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

/// Handle IRC connection with integrated LLM
async fn handle_irc_with_llm(
    stream: TcpStream,
    connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
) -> Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                // Connection closed
                let _ = status_tx.send(format!("✗ IRC connection {} closed", connection_id));
                break;
            }
            Ok(n) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                // DEBUG: Log summary
                debug!("IRC received {} bytes on connection {}", n, connection_id);
                let _ = status_tx.send(format!("[DEBUG] IRC received {} bytes on connection {}", n, connection_id));

                // TRACE: Log full payload (IRC is text)
                trace!("IRC data (text): {:?}", trimmed);
                let _ = status_tx.send(format!("[TRACE] IRC data (text): {:?}", trimmed));

                // Call LLM with the IRC line
                let model = app_state.get_ollama_model().await;
                let prompt_config = get_llm_protocol_prompt();

                // Build event description
                let event_description = format!("IRC message received on connection {}: {}", connection_id, trimmed);

                let prompt = PromptBuilder::build_network_event_prompt(
                    &app_state,
                    connection_id,
                    &event_description,
                    prompt_config,
                ).await;

                match llm_client.generate_llm_response(&model, &prompt).await {
                    Ok(response) => {
                        // Handle common actions
                        use crate::llm::response_handler;
                        let processed = response_handler::handle_llm_response(response, &app_state).await;

                        // Send output if present
                        if let Some(output) = processed.output {
                            // Ensure IRC messages end with \r\n
                            let formatted = if output.ends_with("\r\n") {
                                output.clone()
                            } else if output.ends_with('\n') {
                                format!("{}\r", output)
                            } else {
                                format!("{}\r\n", output)
                            };

                            if let Err(e) = write_half.write_all(formatted.as_bytes()).await {
                                error!("Failed to send IRC response: {}", e);
                                break;
                            }
                            if let Err(e) = write_half.flush().await {
                                error!("Failed to flush IRC response: {}", e);
                                break;
                            }

                            // DEBUG: Log summary
                            debug!("IRC sent {} bytes on connection {}", formatted.len(), connection_id);
                            let _ = status_tx.send(format!("[DEBUG] IRC sent {} bytes on connection {}", formatted.len(), connection_id));

                            // TRACE: Log full payload (IRC is text)
                            trace!("IRC sent (text): {:?}", formatted.trim());
                            let _ = status_tx.send(format!("[TRACE] IRC sent (text): {:?}", formatted.trim()));

                            let _ = status_tx.send(format!("→ IRC to {}: {}", connection_id, formatted.trim()));
                        }

                        // Handle close
                        if processed.close_connection {
                            let _ = status_tx.send(format!("✗ Closing IRC connection {}", connection_id));
                            break;
                        }
                    }
                    Err(e) => {
                        error!("LLM error for IRC: {}", e);
                        let _ = status_tx.send(format!("✗ LLM error for IRC: {}", e));
                    }
                }
            }
            Err(e) => {
                error!("IRC read error: {}", e);
                break;
            }
        }
    }

    Ok(())
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
        format!("{}\r", response)
    } else {
        format!("{}\r\n", response)
    };

    write_half.write_all(formatted.as_bytes()).await?;
    write_half.flush().await?;
    Ok(())
}