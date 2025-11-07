//! Telnet client implementation with option negotiation
pub mod actions;

pub use actions::TelnetClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::telnet::actions::TELNET_CLIENT_DATA_RECEIVED_EVENT;

/// Telnet protocol constants
const IAC: u8 = 255;  // Interpret As Command
const WILL: u8 = 251;
const WONT: u8 = 252;
const DO: u8 = 253;
const DONT: u8 = 254;
const SB: u8 = 250;   // Subnegotiation Begin
const SE: u8 = 240;   // Subnegotiation End

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
    queued_data: Vec<u8>,
    memory: String,
}

/// Telnet client that connects to a remote Telnet server
pub struct TelnetClient;

impl TelnetClient {
    /// Connect to a Telnet server with integrated LLM actions
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

        info!("Telnet client {} connected to {} (local: {})", client_id, remote_sock_addr, local_addr);

        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] Telnet client {} connected", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Split stream
        let (mut read_half, write_half) = tokio::io::split(stream);
        let write_half_arc = Arc::new(Mutex::new(write_half));

        // Initialize client data
        let client_data = Arc::new(Mutex::new(ClientData {
            state: ConnectionState::Idle,
            queued_data: Vec::new(),
            memory: String::new(),
        }));

        // Clone for telnet negotiation handler
        let write_half_for_negotiation = write_half_arc.clone();
        let status_tx_for_negotiation = status_tx.clone();

        // Spawn read loop
        tokio::spawn(async move {
            let mut buffer = vec![0u8; 8192];

            loop {
                match read_half.read(&mut buffer).await {
                    Ok(0) => {
                        info!("Telnet client {} disconnected", client_id);
                        app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                        let _ = status_tx.send(format!("[CLIENT] Telnet client {} disconnected", client_id));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        break;
                    }
                    Ok(n) => {
                        let raw_data = buffer[..n].to_vec();
                        trace!("Telnet client {} received {} bytes", client_id, n);

                        // Parse Telnet protocol and extract data
                        let (data, telnet_commands) = Self::parse_telnet_data(&raw_data);

                        // Handle Telnet option negotiations
                        for cmd in &telnet_commands {
                            if let Some(response) = Self::handle_telnet_command(cmd, client_id, &status_tx_for_negotiation) {
                                if let Ok(_) = write_half_for_negotiation.lock().await.write_all(&response).await {
                                    trace!("Telnet client {} sent negotiation response: {:?}", client_id, response);
                                }
                            }
                        }

                        // Only process data if there's meaningful content
                        if data.is_empty() && telnet_commands.is_empty() {
                            continue;
                        }

                        // Handle data with LLM
                        let mut client_data_lock = client_data.lock().await;

                        match client_data_lock.state {
                            ConnectionState::Idle => {
                                // Process immediately
                                client_data_lock.state = ConnectionState::Processing;
                                drop(client_data_lock);

                                // Call LLM with received data
                                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                                    let protocol = Arc::new(crate::client::telnet::actions::TelnetClientProtocol::new());

                                    // Convert data to UTF-8 string (lossy)
                                    let data_str = String::from_utf8_lossy(&data).to_string();

                                    let event = Event::new(
                                        &TELNET_CLIENT_DATA_RECEIVED_EVENT,
                                        serde_json::json!({
                                            "data": data_str,
                                            "raw_hex": hex::encode(&raw_data),
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
                                            for action in actions {
                                                use crate::llm::actions::client_trait::Client;
                                                match protocol.as_ref().execute_action(action) {
                                                    Ok(crate::llm::actions::client_trait::ClientActionResult::SendData(bytes)) => {
                                                        if let Ok(_) = write_half_arc.lock().await.write_all(&bytes).await {
                                                            trace!("Telnet client {} sent {} bytes", client_id, bytes.len());
                                                        }
                                                    }
                                                    Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                                                        info!("Telnet client {} disconnecting", client_id);
                                                        break;
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            error!("LLM error for Telnet client {}: {}", client_id, e);
                                        }
                                    }
                                }

                                // Process queued data if any
                                let mut client_data_lock = client_data.lock().await;
                                if !client_data_lock.queued_data.is_empty() {
                                    client_data_lock.queued_data.clear();
                                }
                                client_data_lock.state = ConnectionState::Idle;
                            }
                            ConnectionState::Processing => {
                                // Queue data
                                client_data_lock.queued_data.extend_from_slice(&data);
                                client_data_lock.state = ConnectionState::Accumulating;
                            }
                            ConnectionState::Accumulating => {
                                // Continue queuing
                                client_data_lock.queued_data.extend_from_slice(&data);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Telnet client {} read error: {}", client_id, e);
                        app_state.update_client_status(client_id, ClientStatus::Error(e.to_string())).await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Parse Telnet data, separating actual data from Telnet commands
    /// Returns (data, telnet_commands)
    fn parse_telnet_data(raw: &[u8]) -> (Vec<u8>, Vec<TelnetCommand>) {
        let mut data = Vec::new();
        let mut commands = Vec::new();
        let mut i = 0;

        while i < raw.len() {
            if raw[i] == IAC {
                // Telnet command
                if i + 1 >= raw.len() {
                    break;
                }

                let cmd = raw[i + 1];

                match cmd {
                    IAC => {
                        // Escaped IAC (255 255 means literal 255)
                        data.push(IAC);
                        i += 2;
                    }
                    WILL | WONT | DO | DONT => {
                        // Option negotiation
                        if i + 2 < raw.len() {
                            let option = raw[i + 2];
                            commands.push(TelnetCommand::Negotiation {
                                command: cmd,
                                option,
                            });
                            i += 3;
                        } else {
                            i += 2;
                        }
                    }
                    SB => {
                        // Subnegotiation - find SE
                        let mut sb_end = i + 2;
                        while sb_end < raw.len() {
                            if raw[sb_end] == IAC && sb_end + 1 < raw.len() && raw[sb_end + 1] == SE {
                                break;
                            }
                            sb_end += 1;
                        }
                        commands.push(TelnetCommand::Subnegotiation);
                        i = sb_end + 2;
                    }
                    _ => {
                        // Other command
                        commands.push(TelnetCommand::Other(cmd));
                        i += 2;
                    }
                }
            } else {
                // Regular data
                data.push(raw[i]);
                i += 1;
            }
        }

        (data, commands)
    }

    /// Handle a Telnet command and return response if needed
    fn handle_telnet_command(
        cmd: &TelnetCommand,
        client_id: ClientId,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Option<Vec<u8>> {
        match cmd {
            TelnetCommand::Negotiation { command, option } => {
                let cmd_name = match *command {
                    WILL => "WILL",
                    WONT => "WONT",
                    DO => "DO",
                    DONT => "DONT",
                    _ => "UNKNOWN",
                };

                let option_name = Self::get_option_name(*option);
                debug!("Telnet client {} received {} {}", client_id, cmd_name, option_name);

                let _ = status_tx.send(format!(
                    "[CLIENT] Telnet {} negotiation: {} {}",
                    client_id, cmd_name, option_name
                ));

                // Basic negotiation strategy: refuse all options
                match *command {
                    WILL => {
                        // Server offers to do something - respond with DONT (refuse)
                        Some(vec![IAC, DONT, *option])
                    }
                    DO => {
                        // Server asks us to do something - respond with WONT (refuse)
                        Some(vec![IAC, WONT, *option])
                    }
                    _ => None,
                }
            }
            TelnetCommand::Subnegotiation => {
                debug!("Telnet client {} received subnegotiation", client_id);
                None
            }
            TelnetCommand::Other(code) => {
                debug!("Telnet client {} received command code {}", client_id, code);
                None
            }
        }
    }

    /// Get human-readable name for Telnet option
    fn get_option_name(option: u8) -> &'static str {
        match option {
            0 => "BINARY",
            1 => "ECHO",
            3 => "SUPPRESS_GO_AHEAD",
            5 => "STATUS",
            6 => "TIMING_MARK",
            24 => "TERMINAL_TYPE",
            31 => "WINDOW_SIZE",
            32 => "TERMINAL_SPEED",
            33 => "REMOTE_FLOW_CONTROL",
            34 => "LINEMODE",
            36 => "ENVIRONMENT_VARIABLES",
            _ => "UNKNOWN",
        }
    }
}

/// Telnet command types
#[derive(Debug, Clone)]
enum TelnetCommand {
    Negotiation { command: u8, option: u8 },
    Subnegotiation,
    Other(u8),
}
