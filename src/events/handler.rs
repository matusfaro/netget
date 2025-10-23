//! Event handler - coordinates responses to events using LLM

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::types::{AppEvent, UserCommand};
use crate::cli::server_startup;
use crate::llm::OllamaClient;
use crate::llm::{ActionResponse, CommonAction, execute_actions, ProtocolActions};
use crate::protocol::BaseStack;
use crate::state::app_state::{AppState, Mode};
use crate::ui::App;

/// Event handler that coordinates all event processing
#[derive(Clone)]
pub struct EventHandler {
    /// Application state
    state: AppState,
    /// Ollama client
    llm: OllamaClient,
}

impl EventHandler {
    /// Create a new event handler
    pub fn new(state: AppState, llm: OllamaClient) -> Self {
        Self {
            state,
            llm,
        }
    }

    /// Handle an application event
    /// Returns Ok(true) if the application should quit
    pub async fn handle_event(&mut self, event: AppEvent, ui: &mut App) -> Result<bool> {
        match event {
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
            UserCommand::Interpret { input: _ } => {
                ui.add_llm_message("Internal error: Interpret command should use async path".to_string());
                Ok(false)
            }
        }
    }

    /// Handle interpret command asynchronously via channel
    /// This method can be spawned in a task without blocking the UI
    pub async fn handle_interpret(
        &mut self,
        input: String,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        use crate::llm::PromptBuilder;

        let _ = status_tx.send(format!("[INFO] Interpreting: {}", input));

        // Ask LLM to interpret the command
        let prompt = PromptBuilder::build_user_input_prompt(&self.state, &input).await;
        let model = self.state.get_ollama_model().await;

        match self.llm.generate_command_interpretation(&model, &prompt).await {
            Ok(interpretation) => {
                // Display message if provided
                if let Some(msg) = &interpretation.message {
                    let _ = status_tx.send(msg.clone());
                }

                // Execute each action in order
                for action in interpretation.actions {
                    if let Err(e) = self.execute_command_action(action, &status_tx).await {
                        let _ = status_tx.send(format!("[ERROR] Error executing action: {}", e));
                    }
                }
            }
            Err(e) => {
                let _ = status_tx.send(format!("[ERROR] LLM error: {}", e));
            }
        }

        Ok(())
    }

    /// Handle interpret command using NEW action-based system
    /// This method can be spawned in a task without blocking the UI
    pub async fn handle_interpret_with_actions(
        &mut self,
        input: String,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Option<Box<dyn ProtocolActions + Send>>,
    ) -> Result<()> {
        use crate::llm::PromptBuilder;

        let _ = status_tx.send(format!("[INFO] Interpreting: {}", input));

        // Get protocol async actions if available
        let protocol_async_actions = if let Some(ref proto) = protocol {
            proto.get_async_actions(&self.state)
        } else {
            Vec::new()
        };

        // Build prompt with new action system
        let prompt = PromptBuilder::build_user_input_action_prompt(
            &self.state,
            &input,
            protocol_async_actions,
        ).await;
        let model = self.state.get_ollama_model().await;

        // Call LLM
        match self.llm.generate(&model, &prompt).await {
            Ok(llm_output) => {
                // Parse action response
                match ActionResponse::from_str(&llm_output) {
                    Ok(action_response) => {
                        // Execute actions
                        // Convert protocol reference properly
                        let protocol_ref: Option<&dyn ProtocolActions> = protocol.as_ref()
                            .map(|p| p.as_ref() as &dyn ProtocolActions);

                        match execute_actions(
                            action_response.actions.clone(),
                            &self.state,
                            protocol_ref,
                            None, // No network context for user commands
                        ).await {
                            Ok(result) => {
                                // Display messages
                                for msg in result.messages {
                                    let _ = status_tx.send(msg);
                                }

                                // Handle server management actions separately
                                for action_value in &action_response.actions {
                                    if let Ok(common_action) = CommonAction::from_json(action_value) {
                                        if let Err(e) = self.execute_server_management_action(common_action, &status_tx).await {
                                            let _ = status_tx.send(format!("[ERROR] Error executing action: {}", e));
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = status_tx.send(format!("[ERROR] Failed to execute actions: {}", e));
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse action response, falling back to old system: {}", e);
                        // Fall back to old system
                        return self.handle_interpret(input, status_tx).await;
                    }
                }
            }
            Err(e) => {
                let _ = status_tx.send(format!("[ERROR] LLM error: {}", e));
            }
        }

        Ok(())
    }

    /// Execute server management actions (open_server, close_server, etc.)
    async fn execute_server_management_action(
        &mut self,
        action: CommonAction,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        match action {
            CommonAction::OpenServer { port, base_stack, send_first: _, initial_memory, instruction } => {
                use crate::state::server::{ServerInstance, ServerStatus};

                // Parse base stack
                let stack = BaseStack::from_str(&base_stack)
                    .unwrap_or(BaseStack::TcpRaw);

                // Create a new server instance
                let mut server = ServerInstance::new(
                    crate::state::ServerId::new(0), // Temporary ID, will be replaced by add_server
                    port,
                    stack,
                    instruction,
                );

                // Set initial memory if provided
                if let Some(mem) = initial_memory {
                    server.memory = mem;
                }

                server.status = ServerStatus::Starting;

                // Add server to state (this assigns the real ID)
                let server_id = self.state.add_server(server).await;

                let _ = status_tx.send(format!("[SERVER] Opening server #{} on port {} with stack {}", server_id.as_u32(), port, stack));

                // Spawn the server directly (no more message passing!)
                if let Err(e) = server_startup::start_server_by_id(&self.state, server_id, &self.llm, status_tx).await {
                    let _ = status_tx.send(format!("[ERROR] Failed to start server #{}: {}", server_id.as_u32(), e));
                }
            }
            CommonAction::CloseServer => {
                // For now, close all servers (TODO: support targeting specific server ID)
                let server_ids = self.state.get_all_server_ids().await;
                for server_id in server_ids {
                    self.state.remove_server(server_id).await;
                    let _ = status_tx.send(format!("[SERVER] Closed server #{}", server_id.as_u32()));
                    let _ = status_tx.send(format!("__STOP_SERVER__:{}", server_id.as_u32()));
                }
                if self.state.get_all_server_ids().await.is_empty() {
                    self.state.set_mode(Mode::Idle).await;
                }
            }
            CommonAction::UpdateInstruction { instruction } => {
                // Update instruction for first server (TODO: support targeting specific server ID)
                if let Some(server_id) = self.state.get_first_server_id().await {
                    self.state.set_instruction(server_id, instruction.clone()).await;
                    let _ = status_tx.send(format!("[INFO] Server #{} instruction: {}", server_id.as_u32(), instruction));
                } else {
                    let _ = status_tx.send("[WARN] No server to update instruction for".to_string());
                }
            }
            CommonAction::ChangeModel { model } => {
                self.state.set_ollama_model(model.clone()).await;
                let _ = status_tx.send(format!("Changed model to: {}", model));

                // Signal main loop to update UI
                let _ = status_tx.send("__UPDATE_UI__".to_string());
            }
            CommonAction::ShowMessage { message } => {
                let _ = status_tx.send(message);
            }
            // Memory actions need server_id context
            CommonAction::SetMemory { value } => {
                if let Some(server_id) = self.state.get_first_server_id().await {
                    self.state.set_memory(server_id, value).await;
                }
            }
            CommonAction::AppendMemory { value } => {
                if let Some(server_id) = self.state.get_first_server_id().await {
                    self.state.append_memory(server_id, value).await;
                }
            }
        }

        Ok(())
    }


    /// Execute a command action (already in async context)
    async fn execute_command_action(
        &mut self,
        action: crate::llm::CommandAction,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        use crate::llm::CommandAction;
        use crate::state::server::{ServerInstance, ServerStatus};

        match action {
            CommandAction::UpdateInstruction { instruction } => {
                if let Some(server_id) = self.state.get_first_server_id().await {
                    self.state.set_instruction(server_id, instruction.clone()).await;
                    let _ = status_tx.send(format!("[INFO] Server #{} instruction: {}", server_id.as_u32(), instruction));
                }
            }
            CommandAction::OpenServer { port, base_stack, send_first: _, initial_memory, instruction } => {
                // Parse base stack
                let stack = crate::protocol::BaseStack::from_str(&base_stack)
                    .unwrap_or(BaseStack::TcpRaw);

                // Create a new server instance
                let mut server = ServerInstance::new(
                    crate::state::ServerId::new(0), // Temporary ID, will be replaced by add_server
                    port,
                    stack,
                    instruction,
                );

                // Set initial memory if provided
                if let Some(mem) = initial_memory {
                    server.memory = mem;
                }

                server.status = ServerStatus::Starting;

                // Add server to state (this assigns the real ID)
                let server_id = self.state.add_server(server).await;

                let _ = status_tx.send(format!("[SERVER] Opening server #{} on port {} with stack {}", server_id.as_u32(), port, stack));

                // Spawn the server directly (no more message passing!)
                if let Err(e) = server_startup::start_server_by_id(&self.state, server_id, &self.llm, status_tx).await {
                    let _ = status_tx.send(format!("[ERROR] Failed to start server #{}: {}", server_id.as_u32(), e));
                }
            }
            CommandAction::OpenClient { address, base_stack: _ } => {
                let _ = status_tx.send(format!("Client mode not yet implemented ({})", address));
            }
            CommandAction::CloseConnection { connection_id } => {
                // TODO: Update this to work with per-server connections
                if let Some(conn_id_str) = connection_id {
                    if let Some(_conn_id) = crate::network::ConnectionId::from_string(&conn_id_str) {
                        let _ = status_tx.send("[WARN] Close connection not yet implemented for multi-server".to_string());
                    }
                } else {
                    let _ = status_tx.send("[INFO] Close all connections not supported".to_string());
                }
            }
            CommandAction::CloseServer => {
                // Close all servers
                let server_ids = self.state.get_all_server_ids().await;
                for server_id in server_ids {
                    self.state.remove_server(server_id).await;
                    let _ = status_tx.send(format!("[SERVER] Closed server #{}", server_id.as_u32()));
                    let _ = status_tx.send(format!("__STOP_SERVER__:{}", server_id.as_u32()));
                }
                if self.state.get_all_server_ids().await.is_empty() {
                    self.state.set_mode(Mode::Idle).await;
                }
                let _ = status_tx.send("[INFO] Server closed".to_string());

                // Signal main loop to update UI
                let _ = status_tx.send("__UPDATE_UI__".to_string());
            }
            CommandAction::ShowMessage { message } => {
                let _ = status_tx.send(message);
            }
            CommandAction::ChangeModel { model } => {
                self.state.set_ollama_model(model.clone()).await;
                let _ = status_tx.send(format!("Changed model to: {}", model));

                // Signal main loop to update UI
                let _ = status_tx.send("__UPDATE_UI__".to_string());
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
        ui.add_llm_message(format!("Status: {}", summary));

        // Show instruction for first server
        if let Some(server_id) = self.state.get_first_server_id().await {
            if let Some(instruction) = self.state.get_instruction(server_id).await {
                if !instruction.is_empty() {
                    ui.add_llm_message(format!("Server #{} instruction: {}", server_id.as_u32(), instruction));
                }
            }
        } else {
            ui.add_llm_message("No servers running".to_string());
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
}
