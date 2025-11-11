//! NNTP (Network News Transfer Protocol) client implementation
pub mod actions;

pub use actions::NntpClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, trace, debug};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::nntp::actions::{NNTP_CLIENT_CONNECTED_EVENT, NNTP_CLIENT_RESPONSE_RECEIVED_EVENT};

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-client data for LLM handling
struct ClientData {
    state: ConnectionState,
    queued_lines: Vec<String>,
    memory: String,
    last_command: Option<String>,
    pending_post_article: Option<String>,
}

/// NNTP client that connects to a remote NNTP server
pub struct NntpClient;

impl NntpClient {
    /// Connect to an NNTP server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Resolve and connect
        let stream = TcpStream::connect(&remote_addr)
            .await
            .context(format!("Failed to connect to {}", remote_addr))?;

        let local_addr = stream.local_addr()?;
        let remote_sock_addr = stream.peer_addr()?;


        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        console_info!(status_tx, "[CLIENT] NNTP client {} connected", client_id);
        console_info!(status_tx, "__UPDATE_UI__");

        // Split stream for reading and writing
        let (read_half, write_half) = tokio::io::split(stream);
        let write_half_arc = Arc::new(Mutex::new(write_half));

        // Initialize client data
        let client_data = Arc::new(Mutex::new(ClientData {
            state: ConnectionState::Idle,
            queued_lines: Vec::new(),
            memory: String::new(),
            last_command: None,
            pending_post_article: None,
        }));

        // Spawn read loop
        tokio::spawn(async move {
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();

            // Read welcome message
            match reader.read_line(&mut line).await {
                Ok(0) => {
                    app_state.update_client_status(client_id, ClientStatus::Error("No welcome message".to_string())).await;
                    console_error!(status_tx, "__UPDATE_UI__");
                    return;
                }
                Ok(_) => {
                    let welcome = line.trim();
                    info!("NNTP client {} received welcome: {}", client_id, welcome);

                    // Parse status code (for future use)
                    let _status_code = welcome.split_whitespace().next()
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(0);

                    // Call LLM with connected event
                    if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                        let protocol = Arc::new(crate::client::nntp::actions::NntpClientProtocol::new());
                        let event = Event::new(
                            &NNTP_CLIENT_CONNECTED_EVENT,
                            serde_json::json!({
                                "remote_addr": remote_sock_addr.to_string(),
                                "welcome_message": welcome,
                            }),
                        );

                        match call_llm_for_client(
                            &llm_client,
                            &app_state,
                            client_id.to_string(),
                            &instruction,
                            &client_data.lock().await.memory,
                            Some(&event),
                            protocol.as_ref(),
                            &status_tx,
                        ).await {
                            Ok(ClientLlmResult { actions, memory_updates }) => {
                                // Update memory
                                if let Some(mem) = memory_updates {
                                    client_data.lock().await.memory = mem;
                                }

                                // Execute actions from LLM
                                Self::execute_actions(
                                    actions,
                                    &protocol,
                                    &write_half_arc,
                                    &client_data,
                                    client_id,
                                    &status_tx,
                                ).await;
                            }
                            Err(e) => {
                                error!("LLM error for NNTP client {}: {}", client_id, e);
                            }
                        }
                    }
                }
                Err(e) => {
                    app_state.update_client_status(client_id, ClientStatus::Error(e.to_string())).await;
                    console_error!(status_tx, "__UPDATE_UI__");
                    return;
                }
            }

            // Main read loop
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                        console_info!(status_tx, "[CLIENT] NNTP client {} disconnected", client_id);
                        console_info!(status_tx, "__UPDATE_UI__");
                        break;
                    }
                    Ok(_) => {
                        let response = line.trim().to_string();
                        if response.is_empty() {
                            continue;
                        }

                        trace!("NNTP client {} received: {}", client_id, response);

                        // Parse status code
                        let status_code = response.split_whitespace().next()
                            .and_then(|s| s.parse::<u32>().ok())
                            .unwrap_or(0);

                        // Check if this is a multi-line response
                        let is_multiline = matches!(status_code,
                            100 | // HELP text
                            215 | // LIST response
                            220 | // ARTICLE follows
                            221 | // HEAD follows
                            222 | // BODY follows
                            224 | // XOVER follows
                            230 | // NEWNEWS follows
                            231   // NEWGROUPS follows
                        );

                        // Collect multi-line responses
                        let full_response = if is_multiline {
                            let mut lines = vec![response.clone()];
                            loop {
                                line.clear();
                                match reader.read_line(&mut line).await {
                                    Ok(0) => break,
                                    Ok(_) => {
                                        let data_line = line.trim();
                                        if data_line == "." {
                                            // End of multi-line response
                                            break;
                                        }
                                        lines.push(data_line.to_string());
                                    }
                                    Err(e) => {
                                        error!("Error reading multi-line response: {}", e);
                                        break;
                                    }
                                }
                            }
                            lines.join("\n")
                        } else {
                            response.clone()
                        };

                        debug!("NNTP client {} full response: {}", client_id, full_response);

                        // Handle 340 POST response - send pending article immediately
                        if status_code == 340 {
                            let mut client_data_lock = client_data.lock().await;
                            if let Some(article_data) = client_data_lock.pending_post_article.take() {
                                drop(client_data_lock);

                                trace!("NNTP client {} sending article for POST", client_id);
                                if let Err(e) = write_half_arc.lock().await.write_all(article_data.as_bytes()).await {
                                } else {
                                    console_error!(status_tx, "[CLIENT] NNTP {} > [article sent]", client_id);
                                }
                                continue; // Skip LLM processing for 340
                            }
                        }

                        // Handle response with LLM
                        let mut client_data_lock = client_data.lock().await;

                        match client_data_lock.state {
                            ConnectionState::Idle => {
                                // Process immediately
                                client_data_lock.state = ConnectionState::Processing;
                                let last_command = client_data_lock.last_command.clone();
                                drop(client_data_lock);

                                // Call LLM
                                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                                    let protocol = Arc::new(crate::client::nntp::actions::NntpClientProtocol::new());
                                    let event = Event::new(
                                        &NNTP_CLIENT_RESPONSE_RECEIVED_EVENT,
                                        serde_json::json!({
                                            "status_code": status_code,
                                            "response": full_response,
                                            "command": last_command,
                                        }),
                                    );

                                    match call_llm_for_client(
                                        &llm_client,
                                        &app_state,
                                        client_id.to_string(),
                                        &instruction,
                                        &client_data.lock().await.memory,
                                        Some(&event),
                                        protocol.as_ref(),
                                        &status_tx,
                                    ).await {
                                        Ok(ClientLlmResult { actions, memory_updates }) => {
                                            // Update memory
                                            if let Some(mem) = memory_updates {
                                                client_data.lock().await.memory = mem;
                                            }

                                            // Execute actions
                                            Self::execute_actions(
                                                actions,
                                                &protocol,
                                                &write_half_arc,
                                                &client_data,
                                                client_id,
                                                &status_tx,
                                            ).await;
                                        }
                                        Err(e) => {
                                            error!("LLM error for NNTP client {}: {}", client_id, e);
                                        }
                                    }
                                }

                                // Process queued lines if any
                                let mut client_data_lock = client_data.lock().await;
                                if !client_data_lock.queued_lines.is_empty() {
                                    client_data_lock.queued_lines.clear();
                                }
                                client_data_lock.state = ConnectionState::Idle;
                            }
                            ConnectionState::Processing => {
                                // Queue data
                                client_data_lock.queued_lines.push(full_response);
                                client_data_lock.state = ConnectionState::Accumulating;
                            }
                            ConnectionState::Accumulating => {
                                // Continue queuing
                                client_data_lock.queued_lines.push(full_response);
                            }
                        }
                    }
                    Err(e) => {
                        app_state.update_client_status(client_id, ClientStatus::Error(e.to_string())).await;
                        console_error!(status_tx, "__UPDATE_UI__");
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Execute actions from LLM
    async fn execute_actions(
        actions: Vec<serde_json::Value>,
        protocol: &Arc<crate::client::nntp::actions::NntpClientProtocol>,
        write_half_arc: &Arc<Mutex<tokio::io::WriteHalf<TcpStream>>>,
        client_data: &Arc<Mutex<ClientData>>,
        client_id: ClientId,
        status_tx: &mpsc::UnboundedSender<String>,
    ) {
        use crate::llm::actions::client_trait::Client;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

        for action in actions {
            match protocol.as_ref().execute_action(action) {
                Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data }) => {
                    if name == "nntp_command" {
                        // Send NNTP command
                        if let Some(command) = data["command"].as_str() {
                            let command_line = format!("{}\r\n", command);
                            if let Ok(_) = write_half_arc.lock().await.write_all(command_line.as_bytes()).await {
                                console_trace!(status_tx, "[CLIENT] NNTP {} > {}", client_id, command);
                                client_data.lock().await.last_command = Some(command.to_string());
                            }
                        }
                    } else if name == "nntp_post" {
                        // Handle POST command - send POST and wait for 340 response
                        if let (Some(headers), Some(body)) = (data["headers"].as_object(), data["body"].as_str()) {
                            // Build article data to send after receiving 340
                            let mut article = String::new();

                            // Add headers
                            for (key, value) in headers {
                                if let Some(val_str) = value.as_str() {
                                    article.push_str(&format!("{}: {}\r\n", key, val_str));
                                }
                            }

                            // Blank line between headers and body
                            article.push_str("\r\n");

                            // Add body
                            article.push_str(body);

                            // Terminate with CRLF.CRLF
                            article.push_str("\r\n.\r\n");

                            // Store article for sending after 340 response
                            client_data.lock().await.pending_post_article = Some(article);

                            // Send POST command
                            let post_command = "POST\r\n";
                            if let Ok(_) = write_half_arc.lock().await.write_all(post_command.as_bytes()).await {
                                console_trace!(status_tx, "[CLIENT] NNTP {} > POST", client_id);
                                client_data.lock().await.last_command = Some("POST".to_string());
                            } else {
                                // Failed to send POST, clear pending article
                                client_data.lock().await.pending_post_article = None;
                            }
                        }
                    }
                }
                Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                    // Send QUIT command
                    let quit_command = "QUIT\r\n";
                    if let Ok(_) = write_half_arc.lock().await.write_all(quit_command.as_bytes()).await {
                        console_info!(status_tx, "[CLIENT] NNTP {} > QUIT", client_id);
                    }
                    break;
                }
                Ok(crate::llm::actions::client_trait::ClientActionResult::WaitForMore) => {
                    // Do nothing, just wait
                    trace!("NNTP client {} waiting for more data", client_id);
                }
                Ok(_) => {
                    // Other action results not applicable to NNTP
                }
                Err(e) => {
                    error!("Error executing action for NNTP client {}: {}", client_id, e);
                }
            }
        }
    }
}
