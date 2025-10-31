//! Event handler - coordinates responses to events using LLM

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::info;

use super::types::{AppEvent, UserCommand};
use crate::cli::server_startup;
use crate::llm::OllamaClient;
use crate::llm::{execute_actions, CommonAction, ProtocolActions};
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
        Self { state, llm }
    }

    /// List available models from Ollama
    pub async fn list_models(&self) -> Result<Vec<String>> {
        self.llm.list_models().await
    }

    /// Handle an application event
    /// Returns Ok(true) if the application should quit
    pub async fn handle_event(&mut self, event: AppEvent, ui: &mut App) -> Result<bool> {
        match event {
            AppEvent::UserCommand(cmd) => self.handle_user_command(cmd, ui).await,
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
                    ui.add_llm_message(format!(
                        "Invalid log level: {level}. Use: error, warn, info, debug, or trace"
                    ));
                }
                Ok(false)
            }
            UserCommand::TestOutput { count } => {
                // Generate test output lines (used for terminal overflow testing)
                for i in 0..count {
                    ui.add_llm_message(format!("Test line {} of {}", i + 1, count));
                }
                Ok(false)
            }
            UserCommand::SetFooterStatus { message } => {
                // This command is only supported in rolling TUI mode
                if message.is_some() {
                    ui.add_llm_message("Footer status command is only supported in rolling TUI mode".to_string());
                }
                Ok(false)
            }
            UserCommand::Quit => {
                self.handle_quit(ui).await?;
                Ok(true) // Signal to quit
            }
            UserCommand::UnknownSlashCommand { command } => {
                ui.add_llm_message(format!("Unknown command: {command}"));
                ui.add_llm_message(
                    "Available commands: /status, /model [name], /log [level], /docs [protocol], /quit".to_string(),
                );
                Ok(false)
            }
            UserCommand::Interpret { input: _ } => {
                ui.add_llm_message(
                    "Internal error: Interpret command should use async path".to_string(),
                );
                Ok(false)
            }
            UserCommand::ShowScriptingEnv => {
                // This command is only supported in rolling TUI mode
                ui.add_llm_message("Scripting environment command is only supported in rolling TUI mode".to_string());
                Ok(false)
            }
            UserCommand::ChangeScriptingEnv { env: _ } => {
                // This command is only supported in rolling TUI mode
                ui.add_llm_message("Scripting environment command is only supported in rolling TUI mode".to_string());
                Ok(false)
            }
            UserCommand::ShowWebSearch => {
                // This command is only supported in rolling TUI mode
                ui.add_llm_message("Web search command is only supported in rolling TUI mode".to_string());
                Ok(false)
            }
            UserCommand::SetWebSearch { mode: _ } => {
                // This command is only supported in rolling TUI mode
                ui.add_llm_message("Web search command is only supported in rolling TUI mode".to_string());
                Ok(false)
            }
            UserCommand::ShowDocs { protocol } => {
                self.handle_show_docs(protocol, ui).await?;
                Ok(false)
            }
        }
    }

    /// Handle interpret command using NEW action-based system with multi-turn tool support
    /// This method can be spawned in a task without blocking the UI
    pub async fn handle_interpret_with_actions(
        &mut self,
        input: String,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Option<Box<dyn ProtocolActions + Send>>,
    ) -> Result<()> {
        use crate::llm::PromptBuilder;

        let _ = status_tx.send(format!("[INFO] Interpreting: {input}"));

        // Get protocol async actions if available
        let protocol_async_actions = if let Some(ref proto) = protocol {
            proto.get_async_actions(&self.state)
        } else {
            Vec::new()
        };

        let model = self.state.get_ollama_model().await;

        // Create LLM client with status channel for trace logs
        let llm_with_status = self.llm.clone().with_status_tx(status_tx.clone());

        // Use multi-turn loop with tools
        let state_clone = self.state.clone();
        let input_clone = input.clone();
        let actions_clone = protocol_async_actions.clone();

        // Get web search mode and approval channel
        let web_search_mode = self.state.get_web_search_mode().await;
        let approval_tx = self.state.get_web_approval_channel().await;

        let actions = llm_with_status
            .generate_with_tools(
                &model,
                || {
                    let state = state_clone.clone();
                    let input = input_clone.clone();
                    let protocol_actions = actions_clone.clone();
                    async move {
                        PromptBuilder::build_user_input_action_prompt(
                            &state,
                            &input,
                            protocol_actions,
                        )
                        .await
                    }
                },
                5, // max 5 iterations
                approval_tx,
                web_search_mode,
            )
            .await;

        match actions {
            Ok(action_values) => {
                // Handle server management actions FIRST (they need to be executed before other actions)
                for action_value in &action_values {
                    if let Ok(common_action) = CommonAction::from_json(action_value) {
                        if let Err(e) = self
                            .execute_server_management_action(common_action, &status_tx)
                            .await
                        {
                            let _ = status_tx
                                .send(format!("[ERROR] Error executing action: {e}"));
                        }
                    }
                }

                // Then execute all other actions (including append_to_log)
                let protocol_ref: Option<&dyn ProtocolActions> = protocol
                    .as_ref()
                    .map(|p| p.as_ref() as &dyn ProtocolActions);

                match execute_actions(action_values.clone(), &self.state, protocol_ref).await {
                    Ok(result) => {
                        // Display messages
                        for msg in result.messages {
                            let _ = status_tx.send(msg);
                        }
                    }
                    Err(e) => {
                        let _ = status_tx.send(format!("[ERROR] Failed to execute actions: {e}"));
                    }
                }
            }
            Err(e) => {
                let _ = status_tx.send(format!("[ERROR] LLM error: {e}"));
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
            CommonAction::OpenServer {
                port,
                base_stack,
                send_first: _,
                initial_memory,
                instruction,
                startup_params,
                script_language: _,
                script_path: _,
                script_inline,
                script_handles,
            } => {
                use crate::state::server::{ServerInstance, ServerStatus};

                // Parse base stack using registry
                let stack = crate::protocol::registry::registry()
                    .parse_from_str(&base_stack)
                    .unwrap_or(BaseStack::Tcp);

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

                // Set startup params if provided
                server.startup_params = startup_params;

                server.status = ServerStatus::Starting;

                // Add server to state (this assigns the real ID)
                let server_id = self.state.add_server(server).await;

                // Set up script configuration if provided
                if script_inline.is_some() {
                    let selected_mode = self.state.get_selected_scripting_mode().await;
                    match crate::scripting::ScriptManager::build_config(
                        selected_mode,
                        script_inline.as_deref(),
                        script_handles,
                    ) {
                        Ok(Some(config)) => {
                            let scripting_env = self.state.get_scripting_env().await;
                            if scripting_env.is_available(config.language) {
                                self.state.set_script_config(server_id, Some(config)).await;
                                let _ = status_tx.send("[INFO] Script configuration applied to server".to_string());
                            } else {
                                let _ = status_tx.send(format!(
                                    "[WARN] {} not available, script not configured",
                                    config.language.as_str()
                                ));
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            let _ = status_tx.send(format!("[WARN] Failed to build script config: {}", e));
                        }
                    }
                }

                let _ = status_tx.send(format!(
                    "[SERVER] Opening server #{} on port {} with stack {}",
                    server_id.as_u32(),
                    port,
                    stack
                ));

                // Spawn the server directly (no more message passing!)
                if let Err(e) =
                    server_startup::start_server_by_id(&self.state, server_id, &self.llm, status_tx)
                        .await
                {
                    let _ = status_tx.send(format!(
                        "[ERROR] Failed to start server #{}: {}",
                        server_id.as_u32(),
                        e
                    ));
                }
            }
            CommonAction::CloseServer { server_id } => {
                use crate::state::server::ServerStatus;

                let server_ids = if let Some(sid) = server_id {
                    // Close specific server
                    vec![crate::state::ServerId::new(sid)]
                } else {
                    // Close all servers
                    self.state.get_all_server_ids().await
                };

                for server_id in server_ids {
                    // Mark server as Stopped instead of removing it (reaper will clean up after 10s)
                    self.state
                        .update_server_status(server_id, ServerStatus::Stopped)
                        .await;
                    let _ =
                        status_tx.send(format!("[SERVER] Stopped server #{}", server_id.as_u32()));
                }

                // Check if all servers are stopped/error
                let all_stopped =
                    self.state.get_all_servers().await.iter().all(|s| {
                        matches!(s.status, ServerStatus::Stopped | ServerStatus::Error(_))
                    });

                if all_stopped {
                    self.state.set_mode(Mode::Idle).await;
                }

                let _ = status_tx.send("__UPDATE_UI__".to_string());
            }
            CommonAction::UpdateInstruction { instruction } => {
                // Update instruction for first server (TODO: support targeting specific server ID)
                if let Some(server_id) = self.state.get_first_server_id().await {
                    self.state
                        .set_instruction(server_id, instruction.clone())
                        .await;
                    let _ = status_tx.send(format!(
                        "[INFO] Server #{} instruction: {}",
                        server_id.as_u32(),
                        instruction
                    ));
                } else {
                    let _ =
                        status_tx.send("[WARN] No server to update instruction for".to_string());
                }
            }
            CommonAction::ChangeModel { model } => {
                self.state.set_ollama_model(model.clone()).await;
                let _ = status_tx.send(format!("Changed model to: {model}"));

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
            CommonAction::UpdateScript {
                server_id,
                operation,
                script_language: _,
                script_path: _,
                script_inline,
                script_handles,
            } => {
                use crate::scripting::types::ScriptUpdateOperation;

                // Determine which server to update
                let target_server_id = if let Some(sid) = server_id {
                    crate::state::ServerId::new(sid)
                } else if let Some(sid) = self.state.get_first_server_id().await {
                    sid
                } else {
                    let _ = status_tx.send("[WARN] No server to update script for".to_string());
                    return Ok(());
                };

                // Parse operation
                let op = ScriptUpdateOperation::from_str(&operation)
                    .unwrap_or(ScriptUpdateOperation::Set);

                match op {
                    ScriptUpdateOperation::Set => {
                        // Build new script configuration using selected mode
                        let selected_mode = self.state.get_selected_scripting_mode().await;
                        match crate::scripting::ScriptManager::build_config(
                            selected_mode,
                            script_inline.as_deref(),
                            script_handles,
                        ) {
                            Ok(Some(new_config)) => {
                                // Check if language is available
                                let scripting_env = self.state.get_scripting_env().await;
                                if scripting_env.is_available(new_config.language) {
                                    self.state.set_script_config(target_server_id, Some(new_config.clone())).await;
                                    let _ = status_tx.send(format!(
                                        "[INFO] Server #{} script updated ({} handling {:?})",
                                        target_server_id.as_u32(),
                                        new_config.language.as_str(),
                                        new_config.handles_contexts
                                    ));
                                } else {
                                    let _ = status_tx.send(format!(
                                        "[WARN] {} not available, script not updated",
                                        new_config.language.as_str()
                                    ));
                                }
                            }
                            Ok(None) => {
                                let _ = status_tx.send("[WARN] No script configuration provided for 'set' operation".to_string());
                            }
                            Err(e) => {
                                let _ = status_tx.send(format!("[ERROR] Failed to build script config: {}", e));
                            }
                        }
                    }
                    ScriptUpdateOperation::AddContexts => {
                        if let Some(contexts) = script_handles {
                            if let Some(mut config) = self.state.get_script_config(target_server_id).await {
                                config.add_contexts(contexts.clone());
                                self.state.set_script_config(target_server_id, Some(config.clone())).await;
                                let _ = status_tx.send(format!(
                                    "[INFO] Server #{} script now handles: {:?}",
                                    target_server_id.as_u32(),
                                    config.handles_contexts
                                ));
                            } else {
                                let _ = status_tx.send(format!(
                                    "[WARN] Server #{} has no script configuration to update",
                                    target_server_id.as_u32()
                                ));
                            }
                        }
                    }
                    ScriptUpdateOperation::RemoveContexts => {
                        if let Some(contexts) = script_handles {
                            if let Some(mut config) = self.state.get_script_config(target_server_id).await {
                                config.remove_contexts(&contexts);
                                if config.handles_contexts.is_empty() {
                                    // No contexts left, disable script
                                    self.state.set_script_config(target_server_id, None).await;
                                    let _ = status_tx.send(format!(
                                        "[INFO] Server #{} script disabled (no contexts remaining)",
                                        target_server_id.as_u32()
                                    ));
                                } else {
                                    self.state.set_script_config(target_server_id, Some(config.clone())).await;
                                    let _ = status_tx.send(format!(
                                        "[INFO] Server #{} script now handles: {:?}",
                                        target_server_id.as_u32(),
                                        config.handles_contexts
                                    ));
                                }
                            } else {
                                let _ = status_tx.send(format!(
                                    "[WARN] Server #{} has no script configuration to update",
                                    target_server_id.as_u32()
                                ));
                            }
                        }
                    }
                    ScriptUpdateOperation::Disable => {
                        self.state.set_script_config(target_server_id, None).await;
                        let _ = status_tx.send(format!(
                            "[INFO] Server #{} script disabled, using LLM only",
                            target_server_id.as_u32()
                        ));
                    }
                }
            }
            CommonAction::AppendToLog { .. } => {
                // AppendToLog is handled by the action executor, not here
                // This match arm exists to satisfy exhaustiveness checking
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
        ui.add_llm_message(format!("Status: {summary}"));

        // Show instruction for first server
        if let Some(server_id) = self.state.get_first_server_id().await {
            if let Some(instruction) = self.state.get_instruction(server_id).await {
                if !instruction.is_empty() {
                    ui.add_llm_message(format!(
                        "Server #{} instruction: {}",
                        server_id.as_u32(),
                        instruction
                    ));
                }
            }
        } else {
            ui.add_llm_message("No servers running".to_string());
        }

        Ok(())
    }

    async fn handle_show_model(&mut self, ui: &mut App) -> Result<()> {
        let current_model = self.state.get_ollama_model().await;

        ui.add_llm_message(format!("Current model: {current_model}"));
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
                            ui.add_llm_message(format!("  * {model} (current)"));
                        } else {
                            ui.add_llm_message(format!("    {model}"));
                        }
                    }
                    ui.add_llm_message("".to_string());
                    ui.add_llm_message("To change model, use: /model <name>".to_string());
                }
            }
            Err(e) => {
                ui.add_llm_message(format!("Failed to fetch models: {e}"));
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
                    ui.add_llm_message(format!("✓ Changed model to: {model}"));
                } else {
                    ui.add_llm_message(format!("✗ Model '{model}' not found"));
                    ui.add_llm_message("".to_string());

                    if models.is_empty() {
                        ui.add_llm_message("No models available. Pull a model first:".to_string());
                        ui.add_llm_message("  ollama pull llama3.2".to_string());
                    } else {
                        ui.add_llm_message("Available models:".to_string());
                        for available_model in &models {
                            ui.add_llm_message(format!("  {available_model}"));
                        }
                        ui.add_llm_message("".to_string());
                        ui.add_llm_message("Or pull the model:".to_string());
                        ui.add_llm_message(format!("  ollama pull {model}"));
                    }
                }
            }
            Err(e) => {
                ui.add_llm_message(format!("Failed to validate model: {e}"));
                ui.add_llm_message("Make sure Ollama is running.".to_string());
            }
        }

        Ok(())
    }

    async fn handle_show_docs(&mut self, protocol: Option<String>, ui: &mut App) -> Result<()> {
        use crate::docs;

        if let Some(protocol_name) = protocol {
            // Show detailed docs for specific protocol
            match docs::show_protocol_docs(&protocol_name) {
                Ok(docs_text) => {
                    // Split into lines and add each line to the UI
                    for line in docs_text.lines() {
                        ui.add_llm_message(line.to_string());
                    }
                }
                Err(err_msg) => {
                    ui.add_llm_message(err_msg);
                }
            }
        } else {
            // List all protocols
            let docs_text = docs::list_all_protocols();
            for line in docs_text.lines() {
                ui.add_llm_message(line.to_string());
            }
        }

        Ok(())
    }
}
