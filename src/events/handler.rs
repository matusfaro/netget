//! Event handler - coordinates responses to events using LLM

use anyhow::Result;
use std::collections::HashMap;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tracing::{error, info};

use super::types::{AppEvent, NetworkEvent, UserCommand};
use crate::llm::{OllamaClient, PromptBuilder};
use crate::network::connection::ConnectionId;
use crate::network::tcp;
use crate::protocol::BaseStack;
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

    /// Format bytes for display - as text if printable, otherwise as hex
    fn format_data(&self, data: &[u8], max_len: usize) -> String {
        // Check if data is printable ASCII/UTF-8
        let is_text = data.iter().all(|&b| {
            b == b'\n' || b == b'\r' || b == b'\t' || (b >= 32 && b < 127)
        });

        if is_text {
            // Try to display as UTF-8 text
            match std::str::from_utf8(data) {
                Ok(text) => {
                    let display_text = text.replace('\r', "\\r").replace('\n', "\\n").replace('\t', "\\t");
                    if display_text.len() > max_len {
                        format!("{}... ({} bytes)", &display_text[..max_len], data.len())
                    } else {
                        format!("{} ({} bytes)", display_text, data.len())
                    }
                }
                Err(_) => self.format_as_hex(data, max_len),
            }
        } else {
            self.format_as_hex(data, max_len)
        }
    }

    /// Format bytes as hexadecimal
    fn format_as_hex(&self, data: &[u8], max_len: usize) -> String {
        let hex_chars = max_len / 3; // Each byte is "XX " (3 chars)
        let bytes_to_show = hex_chars.min(data.len());

        let hex: String = data.iter()
            .take(bytes_to_show)
            .map(|b| format!("{:02x} ", b))
            .collect();

        if data.len() > bytes_to_show {
            format!("{}... ({} bytes, hex)", hex.trim(), data.len())
        } else {
            format!("{} ({} bytes, hex)", hex.trim(), data.len())
        }
    }


    /// Handle an application event
    /// Returns Ok(true) if the application should quit
    pub async fn handle_event(&mut self, event: AppEvent, ui: &mut App) -> Result<bool> {
        match event {
            AppEvent::Network(net_event) => {
                self.handle_network_event(net_event, ui).await?;
                Ok(false)
            }
            AppEvent::UserCommand(cmd) => {
                self.handle_user_command(cmd, ui).await
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
                // Note: Legacy event handler doesn't support per-connection memory
                let prompt = PromptBuilder::build_connection_established_prompt(&self.state, connection_id, "").await;

                match self.llm.generate_llm_response(&model, &prompt).await {
                    Ok(llm_response) => {
                        if let Some(output) = llm_response.output {
                            // Send the LLM's response
                            if let Some(stream) = self.connections.get_mut(&connection_id) {
                                tcp::send_data(stream, output.as_bytes()).await?;
                                let formatted = self.format_data(output.as_bytes(), 80);
                                ui.add_status_message(format!(
                                    "→ Sent to {}: {}",
                                    connection_id,
                                    formatted
                                ));
                            }
                        }

                        if llm_response.close_connection {
                            if let Some(mut stream) = self.connections.remove(&connection_id) {
                                let _ = stream.shutdown().await;
                                ui.add_status_message(format!("LLM requested connection {} closure", connection_id));
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
                let formatted = self.format_data(&data, 80);
                ui.add_status_message(format!(
                    "← Recv from {}: {}",
                    connection_id,
                    formatted
                ));

                // Update stats
                self.state
                    .update_connection_stats(connection_id, 0, data.len() as u64, 0, 1)
                    .await;

                // Ask LLM what to respond with
                let model = self.state.get_ollama_model().await;
                // Note: Legacy event handler doesn't support per-connection memory
                let prompt = PromptBuilder::build_data_received_prompt(&self.state, connection_id, &data, "").await;

                match self.llm.generate_llm_response(&model, &prompt).await {
                    Ok(llm_response) => {
                        // Handle output first
                        if let Some(output) = llm_response.output {
                            // Send LLM's response
                            if let Some(stream) = self.connections.get_mut(&connection_id) {
                                tcp::send_data(stream, output.as_bytes()).await?;
                                let formatted = self.format_data(output.as_bytes(), 80);
                                ui.add_status_message(format!(
                                    "→ Sent to {}: {}",
                                    connection_id,
                                    formatted
                                ));

                                // Update stats
                                self.state
                                    .update_connection_stats(connection_id, output.len() as u64, 0, 1, 0)
                                    .await;
                            }
                        } else if !llm_response.close_connection && !llm_response.wait_for_more {
                            ui.add_status_message("LLM: No response needed".to_string());
                        }

                        // Handle connection closure
                        if llm_response.close_connection {
                            if let Some(mut stream) = self.connections.remove(&connection_id) {
                                let _ = stream.shutdown().await;
                                ui.add_status_message(format!("LLM requested connection {} closure", connection_id));
                            }
                        }

                        // Handle wait for more
                        if llm_response.wait_for_more {
                            ui.add_status_message("LLM: Waiting for more data".to_string());
                        }

                        // Handle log message
                        if let Some(log_msg) = llm_response.log_message {
                            info!("LLM: {}", log_msg);
                            ui.add_llm_message(log_msg);
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
            NetworkEvent::HttpRequest { .. } => {
                // HTTP requests are handled directly in main.rs
                // This is because they need to send responses back via oneshot channel
                Ok(())
            }
            NetworkEvent::UdpRequest { connection_id, peer_addr, data, response_tx } => {
                // UDP requests (SNMP, DNS, etc.) need to send responses via oneshot channel
                let formatted = self.format_data(&data, 80);
                ui.add_status_message(format!(
                    "← UDP from {}: {}",
                    peer_addr,
                    formatted
                ));

                // Build proper UDP prompt with protocol detection
                let model = self.state.get_ollama_model().await;
                let prompt = crate::llm::PromptBuilder::build_udp_request_prompt(
                    &self.state,
                    connection_id,
                    peer_addr,
                    &data,
                    "",  // No connection memory for UDP
                ).await;

                match self.llm.generate(&model, &prompt).await {
                    Ok(llm_text) => {
                        // Try to parse as JSON first to check for SNMP-specific response
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&llm_text) {
                            // Check if this is an SNMP response with structured data
                            if json.get("snmp_response").is_some() {
                                // Parse the SNMP message to get version, request ID, and community
                                #[cfg(feature = "snmp")]
                                {
                                    if let Ok(parsed) = crate::network::snmp::SnmpServer::parse_snmp_message(&data) {
                                        // Build SNMP response using the structured data
                                        match crate::network::snmp::SnmpServer::build_snmp_response(
                                            &llm_text,
                                            parsed.version,
                                            parsed.request_id,
                                            &parsed.community,
                                        ) {
                                            Ok(response_data) => {
                                                let udp_response = crate::events::types::UdpResponse {
                                                    data: response_data,
                                                };
                                                let _ = response_tx.send(udp_response);
                                                ui.add_status_message(format!("→ SNMP response sent to {}", peer_addr));
                                            }
                                            Err(e) => {
                                                error!("Failed to build SNMP response: {}", e);
                                                let udp_response = crate::events::types::UdpResponse {
                                                    data: Vec::new(),
                                                };
                                                let _ = response_tx.send(udp_response);
                                            }
                                        }
                                    } else {
                                        error!("Failed to parse SNMP request");
                                        let udp_response = crate::events::types::UdpResponse {
                                            data: Vec::new(),
                                        };
                                        let _ = response_tx.send(udp_response);
                                    }
                                }
                                #[cfg(not(feature = "snmp"))]
                                {
                                    let udp_response = crate::events::types::UdpResponse {
                                        data: Vec::new(),
                                    };
                                    let _ = response_tx.send(udp_response);
                                }
                            } else if let Some(output) = json.get("output").and_then(|v| v.as_str()) {
                                // Regular UDP response with raw output
                                let udp_response = crate::events::types::UdpResponse {
                                    data: output.as_bytes().to_vec(),
                                };
                                let _ = response_tx.send(udp_response);
                                ui.add_status_message(format!("→ UDP response sent to {}", peer_addr));
                            } else {
                                // No output specified
                                let udp_response = crate::events::types::UdpResponse {
                                    data: Vec::new(),
                                };
                                let _ = response_tx.send(udp_response);
                                ui.add_status_message("LLM: No UDP response needed".to_string());
                            }
                        } else {
                            // Not JSON, treat as raw output (backward compatibility)
                            let udp_response = crate::events::types::UdpResponse {
                                data: llm_text.as_bytes().to_vec(),
                            };
                            let _ = response_tx.send(udp_response);
                            ui.add_status_message(format!("→ UDP response sent to {}", peer_addr));
                        }
                    }
                    Err(e) => {
                        error!("LLM error processing UDP request: {}", e);
                        ui.add_llm_message(format!("LLM error: {}", e));
                        // Send empty response on error
                        let udp_response = crate::events::types::UdpResponse {
                            data: Vec::new(),
                        };
                        let _ = response_tx.send(udp_response);
                    }
                }

                Ok(())
            }
            NetworkEvent::PacketReceived { interface, data } => {
                // Data link packets are handled directly in main.rs
                // This is because they may need to inject packets back
                let formatted = self.format_data(&data, 80);
                ui.add_status_message(format!(
                    "← Packet on {}: {} bytes: {}",
                    interface,
                    data.len(),
                    formatted
                ));
                Ok(())
            }
        }
    }

    /// Handle user commands
    /// Returns Ok(true) if the application should quit
    async fn handle_user_command(&mut self, command: UserCommand, ui: &mut App) -> Result<bool> {
        match command {
            UserCommand::Status => {
                self.handle_status(ui).await?;
                Ok(false)
            }
            UserCommand::ShowModel => {
                self.handle_show_model(ui).await?;
                Ok(false)
            }
            UserCommand::ChangeModel { model } => {
                self.handle_change_model(model, ui).await?;
                Ok(false)
            }
            UserCommand::ShowLogLevel => {
                ui.add_llm_message(format!("Current log level: {}", ui.log_level.as_str()));
                Ok(false)
            }
            UserCommand::ChangeLogLevel { level } => {
                use crate::ui::app::LogLevel;
                if let Some(log_level) = LogLevel::from_str(&level) {
                    ui.set_log_level(log_level);
                } else {
                    ui.add_llm_message(format!("Invalid log level: {}. Use: error, warn, info, debug, or trace", level));
                }
                Ok(false)
            }
            UserCommand::Quit => {
                self.handle_quit(ui).await?;
                Ok(true) // Signal to quit
            }
            UserCommand::UnknownSlashCommand { command } => {
                ui.add_llm_message(format!("Unknown command: {}", command));
                ui.add_llm_message("Available commands: /status, /model [name], /log [level], /quit".to_string());
                Ok(false)
            }
            UserCommand::Interpret { input } => {
                self.handle_interpret(input, ui).await?;
                Ok(false)
            }
        }
    }

    async fn handle_interpret(&mut self, input: String, ui: &mut App) -> Result<()> {
        use crate::llm::PromptBuilder;

        ui.add_status_message(format!("Interpreting: {}", input));

        // Ask LLM to interpret the command
        let prompt = PromptBuilder::build_command_interpretation_prompt(&self.state, &input).await;
        let model = self.state.get_ollama_model().await;

        match self.llm.generate_command_interpretation(&model, &prompt).await {
            Ok(interpretation) => {
                // Display message if provided
                if let Some(msg) = &interpretation.message {
                    ui.add_llm_message(msg.clone());
                }

                // Execute each action in order
                for action in interpretation.actions {
                    if let Err(e) = self.execute_command_action(action, ui).await {
                        ui.add_llm_message(format!("Error executing action: {}", e));
                    }
                }
            }
            Err(e) => {
                ui.add_llm_message(format!("LLM error: {}", e));
            }
        }

        Ok(())
    }

    async fn execute_command_action(&mut self, action: crate::llm::CommandAction, ui: &mut App) -> Result<()> {
        use crate::llm::CommandAction;

        match action {
            CommandAction::UpdateInstruction { instruction } => {
                self.state.set_instruction(instruction.clone()).await;
                ui.add_status_message(format!("Instruction: {}", instruction));
            }
            CommandAction::OpenServer { port, base_stack, send_banner, initial_memory } => {
                // Parse base stack
                let stack = crate::protocol::BaseStack::from_str(&base_stack)
                    .unwrap_or(BaseStack::TcpRaw);

                self.state.set_mode(Mode::Server).await;
                self.state.set_base_stack(stack).await;
                self.state.set_port(port).await;
                self.state.set_send_banner(send_banner).await;

                // Set initial memory if provided
                if let Some(mem) = initial_memory {
                    self.state.set_memory(mem).await;
                }

                ui.add_llm_message(format!("Opening server on port {} with stack {}", port, stack));
                ui.connection_info.mode = Mode::Server.to_string();
                ui.connection_info.state = format!("Listening on port {}", port);

                // Note: Actual server startup happens in main.rs after this returns
            }
            CommandAction::OpenClient { address, base_stack: _ } => {
                ui.add_llm_message(format!("Client mode not yet implemented ({})", address));
            }
            CommandAction::CloseConnection { connection_id } => {
                if let Some(conn_id_str) = connection_id {
                    if let Some(conn_id) = crate::network::ConnectionId::from_string(&conn_id_str) {
                        self.state.remove_connection(conn_id).await;
                        ui.add_status_message(format!("Closed connection {}", conn_id));
                    }
                } else {
                    // Close all connections
                    for (id, _) in self.connections.drain() {
                        self.state.remove_connection(id).await;
                    }
                    ui.add_status_message("Closed all connections".to_string());
                }
            }
            CommandAction::ShowMessage { message } => {
                ui.add_llm_message(message);
            }
            CommandAction::ChangeModel { model } => {
                self.state.set_ollama_model(model.clone()).await;
                ui.add_llm_message(format!("Changed model to: {}", model));
            }
        }

        Ok(())
    }

    async fn handle_quit(&mut self, ui: &mut App) -> Result<()> {
        ui.add_llm_message("Quitting...".to_string());
        // The main event loop will handle the actual quit
        Ok(())
    }

    async fn handle_status(&mut self, ui: &mut App) -> Result<()> {
        let summary = self.state.get_summary().await;
        let instruction = self.state.get_instruction().await;

        ui.add_llm_message(format!("Status: {}", summary));
        if !instruction.is_empty() {
            ui.add_llm_message(format!("Instruction: {}", instruction));
        } else {
            ui.add_llm_message("No instruction set".to_string());
        }

        Ok(())
    }

    async fn handle_show_model(&mut self, ui: &mut App) -> Result<()> {
        let current_model = self.state.get_ollama_model().await;

        ui.add_llm_message(format!("Current model: {}", current_model));
        ui.add_llm_message("".to_string());
        ui.add_llm_message("Fetching available models...".to_string());

        // Fetch model list from Ollama
        match self.llm.list_models().await {
            Ok(models) => {
                if models.is_empty() {
                    ui.add_llm_message("No models found. Please pull a model first.".to_string());
                    ui.add_llm_message("Example: ollama pull llama3.2".to_string());
                } else {
                    ui.add_llm_message(format!("Available models ({}):", models.len()));
                    for model in &models {
                        if model == &current_model {
                            ui.add_llm_message(format!("  * {} (current)", model));
                        } else {
                            ui.add_llm_message(format!("    {}", model));
                        }
                    }
                    ui.add_llm_message("".to_string());
                    ui.add_llm_message("To change model, use: /model <name>".to_string());
                }
            }
            Err(e) => {
                ui.add_llm_message(format!("Failed to fetch models: {}", e));
                ui.add_llm_message("Make sure Ollama is running.".to_string());
            }
        }

        Ok(())
    }

    async fn handle_change_model(&mut self, model: String, ui: &mut App) -> Result<()> {
        // Validate model exists
        match self.llm.list_models().await {
            Ok(models) => {
                if models.contains(&model) {
                    self.state.set_ollama_model(model.clone()).await;
                    ui.add_llm_message(format!("✓ Changed model to: {}", model));
                } else {
                    ui.add_llm_message(format!("✗ Model '{}' not found", model));
                    ui.add_llm_message("".to_string());

                    if models.is_empty() {
                        ui.add_llm_message("No models available. Pull a model first:".to_string());
                        ui.add_llm_message("  ollama pull llama3.2".to_string());
                    } else {
                        ui.add_llm_message("Available models:".to_string());
                        for available_model in &models {
                            ui.add_llm_message(format!("  {}", available_model));
                        }
                        ui.add_llm_message("".to_string());
                        ui.add_llm_message("Or pull the model:".to_string());
                        ui.add_llm_message(format!("  ollama pull {}", model));
                    }
                }
            }
            Err(e) => {
                ui.add_llm_message(format!("Failed to validate model: {}", e));
                ui.add_llm_message("Make sure Ollama is running.".to_string());
            }
        }

        Ok(())
    }

    /// Register a new connection
    pub fn add_connection(&mut self, connection_id: ConnectionId, stream: TcpStream) {
        self.connections.insert(connection_id, stream);
    }
}
