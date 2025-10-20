//! Event handler - coordinates responses to events using LLM

use anyhow::Result;
use std::collections::HashMap;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tracing::{error, info, warn};

use super::types::{AppEvent, NetworkEvent, UserCommand};
use crate::llm::{OllamaClient, PromptBuilder};
use crate::network::connection::ConnectionId;
use crate::network::tcp;
use crate::protocol::ProtocolType;
use crate::state::app_state::{AppState, Mode};
use crate::ui::App;

/// Event handler that coordinates all event processing
pub struct EventHandler {
    /// Application state
    state: AppState,
    /// Ollama client
    llm: OllamaClient,
    /// Active connections (for sending data)
    connections: HashMap<ConnectionId, TcpStream>,
}

impl EventHandler {
    /// Create a new event handler
    pub fn new(state: AppState, llm: OllamaClient) -> Self {
        Self {
            state,
            llm,
            connections: HashMap::new(),
        }
    }

    /// Process LLM response to handle common issues
    /// - Strips b"..." wrapping if present
    /// - Unescapes common escape sequences if needed
    fn process_llm_response(&self, response: &str) -> String {
        let trimmed = response.trim();

        // Check if wrapped in b"..." or just "..."
        let unwrapped = if trimmed.starts_with("b\"") && trimmed.ends_with('"') {
            warn!("LLM returned debug format b\"...\", unwrapping");
            &trimmed[2..trimmed.len()-1]
        } else if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() > 1 {
            &trimmed[1..trimmed.len()-1]
        } else {
            trimmed
        };

        // Unescape common sequences if they appear to be escaped
        // Only do this if we see literal \n or \r (not actual newlines)
        if unwrapped.contains("\\n") || unwrapped.contains("\\r") || unwrapped.contains("\\t") {
            warn!("LLM returned escaped sequences, unescaping");
            unwrapped
                .replace("\\r\\n", "\r\n")
                .replace("\\n", "\n")
                .replace("\\r", "\r")
                .replace("\\t", "\t")
                .replace("\\\\", "\\")
        } else {
            unwrapped.to_string()
        }
    }

    /// Handle an application event
    pub async fn handle_event(&mut self, event: AppEvent, ui: &mut App) -> Result<bool> {
        match event {
            AppEvent::Network(net_event) => {
                self.handle_network_event(net_event, ui).await?;
                Ok(false)
            }
            AppEvent::UserCommand(cmd) => {
                self.handle_user_command(cmd, ui).await?;
                Ok(false)
            }
            AppEvent::Tick => {
                // Periodic updates can go here
                Ok(false)
            }
            AppEvent::Shutdown => {
                info!("Shutdown event received");
                Ok(true)
            }
        }
    }

    /// Handle network events using LLM
    async fn handle_network_event(&mut self, event: NetworkEvent, ui: &mut App) -> Result<()> {
        match event {
            NetworkEvent::Listening { addr } => {
                ui.add_status_message(format!("Listening on {}", addr));
                self.state.set_local_addr(Some(addr)).await;
                Ok(())
            }
            NetworkEvent::Connected {
                connection_id,
                remote_addr,
            } => {
                ui.add_status_message(format!(
                    "Connection {} established from {}",
                    connection_id, remote_addr
                ));

                // Ask LLM if we should send any initial data (e.g., FTP welcome)
                let model = self.state.get_ollama_model().await;
                let prompt = PromptBuilder::build_connection_established_prompt(&self.state, connection_id).await;

                match self.llm.generate(&model, &prompt).await {
                    Ok(response) => {
                        let response = self.process_llm_response(&response);
                        if !response.is_empty() && response != "NO_RESPONSE" {
                            // Send the LLM's response
                            if let Some(stream) = self.connections.get_mut(&connection_id) {
                                tcp::send_data(stream, response.as_bytes()).await?;
                                ui.add_status_message(format!(
                                    "Sent initial {} bytes to {}",
                                    response.len(),
                                    connection_id
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        error!("LLM error on connection: {}", e);
                        ui.add_llm_message(format!("LLM error: {}", e));
                    }
                }

                Ok(())
            }
            NetworkEvent::Disconnected { connection_id } => {
                ui.add_status_message(format!("Connection {} closed", connection_id));
                self.connections.remove(&connection_id);
                self.state.remove_connection(connection_id).await;
                Ok(())
            }
            NetworkEvent::DataReceived {
                connection_id,
                data,
            } => {
                ui.add_status_message(format!(
                    "Received {} bytes from {}",
                    data.len(),
                    connection_id
                ));

                // Update stats
                self.state
                    .update_connection_stats(connection_id, 0, data.len() as u64, 0, 1)
                    .await;

                // Ask LLM what to respond with
                let model = self.state.get_ollama_model().await;
                let prompt = PromptBuilder::build_data_received_prompt(&self.state, connection_id, &data).await;

                ui.add_status_message("Asking LLM for response...".to_string());

                match self.llm.generate(&model, &prompt).await {
                    Ok(response) => {
                        let response = self.process_llm_response(&response);

                        if response == "CLOSE_CONNECTION" {
                            if let Some(mut stream) = self.connections.remove(&connection_id) {
                                let _ = stream.shutdown().await;
                                ui.add_status_message(format!("LLM requested connection {} closure", connection_id));
                            }
                        } else if !response.is_empty() && response != "NO_RESPONSE" {
                            // Send LLM's response
                            if let Some(stream) = self.connections.get_mut(&connection_id) {
                                tcp::send_data(stream, response.as_bytes()).await?;
                                ui.add_status_message(format!(
                                    "LLM: Sent {} bytes to {}",
                                    response.len(),
                                    connection_id
                                ));

                                // Update stats
                                self.state
                                    .update_connection_stats(connection_id, response.len() as u64, 0, 1, 0)
                                    .await;
                            }
                        } else {
                            ui.add_status_message("LLM: No response needed".to_string());
                        }
                    }
                    Err(e) => {
                        error!("LLM error processing data: {}", e);
                        ui.add_llm_message(format!("LLM error: {}", e));
                    }
                }

                Ok(())
            }
            NetworkEvent::DataSent {
                connection_id,
                data,
            } => {
                ui.add_status_message(format!(
                    "Sent {} bytes to {}",
                    data.len(),
                    connection_id
                ));
                Ok(())
            }
            NetworkEvent::Error {
                connection_id,
                error,
            } => {
                let msg = if let Some(id) = connection_id {
                    format!("Error on connection {}: {}", id, error)
                } else {
                    format!("Network error: {}", error)
                };
                ui.add_status_message(msg.clone());
                error!("{}", msg);
                Ok(())
            }
        }
    }

    /// Handle user commands
    async fn handle_user_command(&mut self, command: UserCommand, ui: &mut App) -> Result<()> {
        match command {
            UserCommand::Listen { port, protocol } => {
                self.handle_listen(port, protocol, ui).await
            }
            UserCommand::Connect { addr: _, protocol: _ } => {
                ui.add_llm_message("Client mode not yet implemented".to_string());
                Ok(())
            }
            UserCommand::Close => {
                self.handle_close(ui).await
            }
            UserCommand::AddFile { name, content } => {
                self.handle_add_file(name, content, ui).await
            }
            UserCommand::Status => {
                self.handle_status(ui).await
            }
            UserCommand::ChangeModel { model } => {
                self.handle_change_model(model, ui).await
            }
            UserCommand::Raw { input } => {
                self.handle_raw_input(input, ui).await
            }
        }
    }

    async fn handle_listen(&mut self, port: u16, protocol_type: ProtocolType, ui: &mut App) -> Result<()> {
        ui.add_llm_message(format!("Starting {} server on port {}...", protocol_type, port));

        // Set mode and protocol
        self.state.set_mode(Mode::Server).await;
        self.state.set_protocol_type(protocol_type).await;

        ui.add_status_message(format!("Protocol set to: {}", protocol_type));
        ui.add_llm_message("LLM will handle all protocol responses".to_string());

        // Update UI connection info
        ui.connection_info.mode = Mode::Server.to_string();
        ui.connection_info.protocol = protocol_type.to_string();
        ui.connection_info.state = "Ready to listen".to_string();

        Ok(())
    }

    async fn handle_close(&mut self, ui: &mut App) -> Result<()> {
        ui.add_llm_message("Closing all connections...".to_string());

        for (id, _) in self.connections.drain() {
            self.state.remove_connection(id).await;
        }

        ui.add_status_message("All connections closed".to_string());
        Ok(())
    }

    async fn handle_add_file(&mut self, name: String, content: Vec<u8>, ui: &mut App) -> Result<()> {
        // Store as an instruction for the LLM
        let instruction = format!("Serve file '{}' with content: {:?}", name, String::from_utf8_lossy(&content));
        self.state.add_instruction(instruction).await;

        ui.add_llm_message(format!("Instructed to serve file '{}' ({} bytes)", name, content.len()));
        ui.add_llm_message("LLM will use this information when handling requests".to_string());
        Ok(())
    }

    async fn handle_status(&mut self, ui: &mut App) -> Result<()> {
        let summary = self.state.get_summary().await;
        let instructions = self.state.get_instructions().await;

        ui.add_llm_message(format!("Status: {}", summary));
        if !instructions.is_empty() {
            ui.add_llm_message(format!("Instructions: {} recorded", instructions.len()));
        }

        Ok(())
    }

    async fn handle_change_model(&mut self, model: String, ui: &mut App) -> Result<()> {
        self.state.set_ollama_model(model.clone()).await;
        ui.add_llm_message(format!("Changed Ollama model to: {}", model));
        Ok(())
    }

    async fn handle_raw_input(&mut self, input: String, ui: &mut App) -> Result<()> {
        // Store the instruction for LLM context
        self.state.add_instruction(input.clone()).await;

        ui.add_llm_message(format!("Instruction stored: {}", input));
        ui.add_status_message("LLM will use this when handling connections".to_string());

        Ok(())
    }

    /// Register a new connection
    pub fn add_connection(&mut self, connection_id: ConnectionId, stream: TcpStream) {
        self.connections.insert(connection_id, stream);
    }
}
