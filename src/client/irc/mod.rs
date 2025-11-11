//! IRC client implementation
pub mod actions;

pub use actions::IrcClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace, warn};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::irc::actions::{IRC_CLIENT_CONNECTED_EVENT, IRC_CLIENT_MESSAGE_RECEIVED_EVENT};

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
    queued_messages: Vec<String>,
    memory: String,
    nickname: String,
}

/// IRC client that connects to an IRC server
pub struct IrcClient;

impl IrcClient {
    /// Connect to an IRC server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        // Parse startup params
        let nickname = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("nickname"))
            .unwrap_or_else(|| "netget_user".to_string());
        let username = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("username"))
            .unwrap_or_else(|| "netget".to_string());
        let realname = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("realname"))
            .unwrap_or_else(|| "NetGet IRC Client".to_string());

        // Resolve and connect
        let stream = TcpStream::connect(&remote_addr)
            .await
            .context(format!("Failed to connect to {}", remote_addr))?;

        let local_addr = stream.local_addr()?;
        let remote_sock_addr = stream.peer_addr()?;


        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        console_info!(status_tx, "[CLIENT] IRC client {} connected to {}", client_id, remote_sock_addr);
        console_info!(status_tx, "__UPDATE_UI__");

        // Split stream
        let (read_half, write_half) = tokio::io::split(stream);
        let write_half_arc = Arc::new(Mutex::new(write_half));

        // Send IRC registration
        let mut writer = write_half_arc.lock().await;
        writer.write_all(format!("NICK {}\r\n", nickname).as_bytes()).await?;
        writer.write_all(format!("USER {} 0 * :{}\r\n", username, realname).as_bytes()).await?;
        drop(writer);

        debug!("IRC client {} sent registration (nick: {})", client_id, nickname);

        // Initialize client data
        let client_data = Arc::new(Mutex::new(ClientData {
            state: ConnectionState::Idle,
            queued_messages: Vec::new(),
            memory: String::new(),
            nickname: nickname.clone(),
        }));

        // Clone for spawned task
        let write_half_clone = write_half_arc.clone();
        let client_data_clone = client_data.clone();
        let nickname_clone = nickname.clone();

        // Spawn read loop
        tokio::spawn(async move {
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();
            let mut registered = false;

            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                        console_info!(status_tx, "[CLIENT] IRC client {} disconnected", client_id);
                        console_info!(status_tx, "__UPDATE_UI__");
                        break;
                    }
                    Ok(_) => {
                        let line = line.trim_end().to_string();
                        trace!("IRC client {} received: {}", client_id, line);

                        // Handle PING immediately
                        if line.starts_with("PING ") {
                            let pong = line.replace("PING", "PONG");
                            if let Ok(_) = write_half_clone.lock().await.write_all(format!("{}\r\n", pong).as_bytes()).await {
                                trace!("IRC client {} sent PONG", client_id);
                            }
                            continue;
                        }

                        // Check for registration complete (001 welcome message)
                        if !registered && line.contains(" 001 ") {
                            registered = true;
                            info!("IRC client {} registration complete", client_id);

                            // Call LLM with connected event
                            if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                                let protocol = Arc::new(IrcClientProtocol::new());
                                let event = Event::new(
                                    &IRC_CLIENT_CONNECTED_EVENT,
                                    serde_json::json!({
                                        "remote_addr": remote_sock_addr.to_string(),
                                        "nickname": nickname_clone,
                                    }),
                                );

                                if let Err(e) = Self::handle_llm_call(
                                    &llm_client,
                                    &app_state,
                                    client_id,
                                    &instruction,
                                    &client_data_clone,
                                    Some(&event),
                                    protocol,
                                    &write_half_clone,
                                    &status_tx,
                                ).await {
                                    error!("IRC client {} LLM error on connect: {}", client_id, e);
                                }
                            }
                            continue;
                        }

                        // Skip if not yet registered
                        if !registered {
                            continue;
                        }

                        // Parse IRC message
                        let parsed = Self::parse_irc_message(&line);

                        // Handle message with LLM
                        let mut client_data_lock = client_data_clone.lock().await;

                        match client_data_lock.state {
                            ConnectionState::Idle => {
                                // Process immediately
                                client_data_lock.state = ConnectionState::Processing;
                                drop(client_data_lock);

                                // Call LLM
                                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                                    let protocol = Arc::new(IrcClientProtocol::new());
                                    let event = Event::new(
                                        &IRC_CLIENT_MESSAGE_RECEIVED_EVENT,
                                        serde_json::json!({
                                            "source": parsed.source,
                                            "command": parsed.command,
                                            "target": parsed.target,
                                            "message": parsed.message,
                                            "raw_message": line,
                                        }),
                                    );

                                    if let Err(e) = Self::handle_llm_call(
                                        &llm_client,
                                        &app_state,
                                        client_id,
                                        &instruction,
                                        &client_data_clone,
                                        Some(&event),
                                        protocol,
                                        &write_half_clone,
                                        &status_tx,
                                    ).await {
                                        error!("IRC client {} LLM error: {}", client_id, e);
                                    }
                                }

                                // Process queued messages if any
                                let mut client_data_lock = client_data_clone.lock().await;
                                if !client_data_lock.queued_messages.is_empty() {
                                    client_data_lock.queued_messages.clear();
                                }
                                client_data_lock.state = ConnectionState::Idle;
                            }
                            ConnectionState::Processing => {
                                // Queue message
                                client_data_lock.queued_messages.push(line.clone());
                                client_data_lock.state = ConnectionState::Accumulating;
                            }
                            ConnectionState::Accumulating => {
                                // Continue queuing
                                client_data_lock.queued_messages.push(line.clone());
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

    /// Parse an IRC message into components
    fn parse_irc_message(line: &str) -> ParsedMessage {
        let mut parts = line.split_whitespace();

        let source = if line.starts_with(':') {
            parts.next().map(|s| s.trim_start_matches(':').to_string())
        } else {
            None
        };

        let command = parts.next().map(|s| s.to_uppercase()).unwrap_or_default();

        let remaining: Vec<&str> = parts.collect();
        let (target, message) = if command == "PRIVMSG" || command == "NOTICE" {
            let target = remaining.first().map(|s| s.to_string());
            let message = if let Some(idx) = remaining.iter().position(|s| s.starts_with(':')) {
                let msg_parts: Vec<&str> = remaining[idx..].iter().map(|s| *s).collect();
                Some(msg_parts.join(" ").trim_start_matches(':').to_string())
            } else {
                None
            };
            (target, message)
        } else {
            (None, None)
        };

        ParsedMessage {
            source,
            command,
            target,
            message,
        }
    }

    /// Handle LLM call and execute actions
    async fn handle_llm_call(
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        client_id: ClientId,
        instruction: &str,
        client_data: &Arc<Mutex<ClientData>>,
        event: Option<&Event>,
        protocol: Arc<IrcClientProtocol>,
        write_half: &Arc<Mutex<tokio::io::WriteHalf<TcpStream>>>,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let memory = client_data.lock().await.memory.clone();

        match call_llm_for_client(
            llm_client,
            app_state,
            client_id.to_string(),
            instruction,
            &memory,
            event,
            protocol.as_ref(),
            status_tx,
        ).await {
            Ok(ClientLlmResult { actions, memory_updates }) => {
                // Update memory
                if let Some(mem) = memory_updates {
                    client_data.lock().await.memory = mem;
                }

                // Execute actions
                for action in actions {
                    use crate::llm::actions::client_trait::Client;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};
                    match protocol.as_ref().execute_action(action) {
                        Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data }) => {
                            Self::execute_irc_action(&name, data, write_half, client_data).await?;
                        }
                        Ok(crate::llm::actions::client_trait::ClientActionResult::WaitForMore) => {
                            // Do nothing, just wait
                        }
                        Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                            info!("IRC client {} disconnecting", client_id);
                            return Err(anyhow::anyhow!("Disconnect requested"));
                        }
                        Err(e) => {
                            warn!("IRC client {} action execution error: {}", client_id, e);
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                return Err(e);
            }
        }

        Ok(())
    }

    /// Execute IRC-specific actions
    async fn execute_irc_action(
        name: &str,
        data: serde_json::Value,
        write_half: &Arc<Mutex<tokio::io::WriteHalf<TcpStream>>>,
        client_data: &Arc<Mutex<ClientData>>,
    ) -> Result<()> {
        let mut writer = write_half.lock().await;

        match name {
            "join_channel" => {
                let channel = data["channel"].as_str().context("Missing channel")?;
                writer.write_all(format!("JOIN {}\r\n", channel).as_bytes()).await?;
                debug!("IRC: JOIN {}", channel);
            }
            "part_channel" => {
                let channel = data["channel"].as_str().context("Missing channel")?;
                let message = data["message"].as_str();
                if let Some(msg) = message {
                    writer.write_all(format!("PART {} :{}\r\n", channel, msg).as_bytes()).await?;
                } else {
                    writer.write_all(format!("PART {}\r\n", channel).as_bytes()).await?;
                }
                debug!("IRC: PART {}", channel);
            }
            "change_nick" => {
                let new_nick = data["new_nick"].as_str().context("Missing new_nick")?;
                writer.write_all(format!("NICK {}\r\n", new_nick).as_bytes()).await?;
                client_data.lock().await.nickname = new_nick.to_string();
                debug!("IRC: NICK {}", new_nick);
            }
            "send_privmsg" => {
                let target = data["target"].as_str().context("Missing target")?;
                let message = data["message"].as_str().context("Missing message")?;
                writer.write_all(format!("PRIVMSG {} :{}\r\n", target, message).as_bytes()).await?;
                debug!("IRC: PRIVMSG {} :{}", target, message);
            }
            "send_notice" => {
                let target = data["target"].as_str().context("Missing target")?;
                let message = data["message"].as_str().context("Missing message")?;
                writer.write_all(format!("NOTICE {} :{}\r\n", target, message).as_bytes()).await?;
                debug!("IRC: NOTICE {} :{}", target, message);
            }
            "send_raw" => {
                let command = data["command"].as_str().context("Missing command")?;
                writer.write_all(format!("{}\r\n", command).as_bytes()).await?;
                debug!("IRC: RAW {}", command);
            }
            "disconnect" => {
                let quit_message = data["quit_message"].as_str().unwrap_or("Leaving");
                writer.write_all(format!("QUIT :{}\r\n", quit_message).as_bytes()).await?;
                debug!("IRC: QUIT");
            }
            _ => {
                warn!("Unknown IRC action: {}", name);
            }
        }

        Ok(())
    }
}

/// Parsed IRC message
#[derive(Debug)]
struct ParsedMessage {
    source: Option<String>,
    command: String,
    target: Option<String>,
    message: Option<String>,
}
