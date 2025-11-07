//! Event handler - coordinates responses to events using LLM

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::info;

use super::types::{AppEvent, UserCommand};
use crate::cli::server_startup;
use crate::llm::actions::{get_all_tool_actions, get_user_input_common_actions};
use crate::llm::OllamaClient;
use crate::llm::{execute_actions, CommonAction, Server};
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
            UserCommand::TestAsk => {
                // Test web search approval prompt by triggering a search to DuckDuckGo
                use crate::llm::actions::tools::{execute_tool, ToolAction};

                ui.add_llm_message("[INFO] Testing web search approval with DuckDuckGo...".to_string());

                // Get web search mode and approval channel
                let web_search_mode = self.state.get_web_search_mode().await;
                let approval_tx = self.state.get_web_approval_channel().await;

                // Create a web search action for DuckDuckGo with a long path to test truncation
                let action = ToolAction::WebSearch {
                    query: "https://duckduckgo.com/?q=test+search+query+with+very+long+parameters&ia=web&category=general&filters=none".to_string(),
                };

                // Execute the tool (this will trigger approval prompt if in ASK mode)
                let result = execute_tool(&action, approval_tx.as_ref(), web_search_mode, Some(&self.state)).await;

                // Display the result
                if result.success {
                    ui.add_llm_message(format!("[INFO] Web search completed successfully"));
                    ui.add_llm_message(format!("[DEBUG] Result: {}", result.result));
                } else {
                    ui.add_llm_message(format!("[ERROR] Web search failed: {}", result.result));
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
        protocol: Option<Box<dyn Server + Send>>,
    ) -> Result<()> {
        use crate::llm::{ConversationHandler, PromptBuilder};

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

        // Get web search mode and approval channel
        let web_search_mode = self.state.get_web_search_mode().await;
        let approval_tx = self.state.get_web_approval_channel().await;

        // Get conversation history from persistent state
        let conversation_history = self.state.get_user_conversation_history().await;

        // Build system prompt (without user input - that's added as a message)
        let system_prompt = PromptBuilder::build_user_input_system_prompt(
            &self.state,
            protocol_async_actions.clone(),
            conversation_history,
        )
        .await;

        // Get available actions for retry correction messages
        let selected_mode = self.state.get_selected_scripting_mode().await;
        let scripting_env = self.state.get_scripting_env().await;
        // Initially disable open_server and open_client - they will be enabled after read_base_stack_docs is called in the conversation loop
        let is_open_server_enabled = false;
        let is_open_client_enabled = false;
        let mut available_actions = get_user_input_common_actions(selected_mode, &scripting_env, is_open_server_enabled, is_open_client_enabled);
        available_actions.extend(get_all_tool_actions(web_search_mode));
        available_actions.extend(protocol_async_actions);

        // Create conversation handler with tracking
        let truncated_input = if input.len() > 60 {
            format!("LLM \"{}...\"", &input[..57])
        } else {
            format!("LLM \"{}\"", input)
        };

        // Get or create persistent conversation state
        let conversation_state = self.state.get_or_create_user_conversation_state().await;

        let mut conversation = ConversationHandler::new(
            system_prompt,
            std::sync::Arc::new(llm_with_status),
            model,
        )
        .with_status_tx(status_tx.clone())
        .with_tracking(
            self.state.clone(),
            crate::state::app_state::ConversationSource::User,
            truncated_input,
        )
        .with_conversation_state(conversation_state);

        // Add user input as a separate user message
        conversation.add_user_message(input.clone());

        // Retry loop for execution-time errors (e.g., port conflicts)
        const MAX_EXECUTION_RETRIES: usize = 1;
        let mut execution_attempts = 0;

        loop {
            // Generate actions with tool calling and retry
            let actions = conversation
                .generate_with_tools_and_retry(
                    approval_tx.clone(),
                    web_search_mode,
                    available_actions.clone(),
                )
                .await;

            match actions {
                Ok(action_values) => {
                    let mut should_retry = false;
                    let mut retry_error: Option<crate::events::ActionExecutionError> = None;

                    // Handle server management actions FIRST (they need to be executed before other actions)
                    let mut state_changed = false;
                    for action_value in &action_values {
                        if let Ok(common_action) = CommonAction::from_json(action_value) {
                            // Check if this action will modify state (open_server, close_server, open_client, close_client, etc.)
                            let modifies_state = matches!(
                                common_action,
                                CommonAction::OpenServer { .. }
                                    | CommonAction::CloseServer { .. }
                                    | CommonAction::CloseAllServers
                                    | CommonAction::OpenClient { .. }
                                    | CommonAction::CloseClient { .. }
                                    | CommonAction::CloseAllClients
                            );

                            match self.execute_server_management_action(common_action, &status_tx).await {
                                Ok(_) => {
                                    // Action executed successfully
                                    if modifies_state {
                                        state_changed = true;
                                    }
                                }
                                Err(e) if e.is_retryable() && execution_attempts < MAX_EXECUTION_RETRIES => {
                                    // Retryable error (e.g., port conflict) - prepare to retry
                                    should_retry = true;
                                    retry_error = Some(e);
                                    break; // Stop processing actions, we'll retry
                                }
                                Err(e) => {
                                    // Non-retryable error or max retries exceeded
                                    let _ = status_tx.send(format!("[ERROR] Error executing action: {e}"));
                                }
                            }
                        }
                    }

                    // Update conversation state if server state changed
                    if state_changed && !should_retry {
                        conversation.update_current_state(&self.state, None).await;
                        let _ = status_tx.send("[DEBUG] Updated conversation state after server changes".to_string());
                    }

                    // If we should retry, add error to conversation and retry
                    if should_retry {
                        if let Some(error) = retry_error {
                            execution_attempts += 1;
                            let _ = status_tx.send(format!(
                                "[INFO] Execution error (attempt {}/{}), retrying with LLM feedback...",
                                execution_attempts,
                                MAX_EXECUTION_RETRIES + 1
                            ));

                            // Add error correction to conversation
                            let correction = error.build_correction_message();
                            conversation.add_user_message(correction);

                            // Continue loop to retry
                            continue;
                        }
                    }

                    // Then execute all other actions (including append_to_log)
                    let protocol_ref: Option<&dyn Server> = protocol
                        .as_ref()
                        .map(|p| p.as_ref() as &dyn Server);

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

                    // Success - break out of retry loop
                    break;
                }
                Err(e) => {
                    let _ = status_tx.send(format!("[ERROR] LLM error: {e}"));
                    break; // LLM errors don't retry at this level
                }
            }
        }

        Ok(())
    }

    /// Execute server management actions (open_server, close_server, etc.)
    async fn execute_server_management_action(
        &mut self,
        action: CommonAction,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<(), crate::events::ActionExecutionError> {
        match action {
            CommonAction::OpenServer {
                port,
                base_stack,
                send_first: _,
                initial_memory,
                instruction,
                startup_params,
                script_runtime,
                script_language: _,
                script_path: _,
                script_inline,
                script_handles,
                scheduled_tasks,
            } => {
                use crate::state::server::{ServerInstance, ServerStatus};

                // Parse protocol name using registry
                let protocol_name = crate::protocol::registry::registry()
                    .parse_from_str(&base_stack)
                    .unwrap_or_else(|| "TCP".to_string());

                // Create a new server instance
                let mut server = ServerInstance::new(
                    crate::state::ServerId::new(0), // Temporary ID, will be replaced by add_server
                    port,
                    protocol_name.clone(),
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
                        script_runtime.as_deref(),
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
                    "[SERVER] Opening server #{} on port {} with protocol {}",
                    server_id.as_u32(),
                    port,
                    protocol_name
                ));

                // Spawn the server directly (no more message passing!)
                // Propagate port conflict errors for retry, but continue for other errors
                match server_startup::start_server_by_id(&self.state, server_id, &self.llm, status_tx).await {
                    Ok(_) => {
                        // Server started successfully
                    }
                    Err(e) if e.is_retryable() => {
                        // Port conflict or other retryable error - propagate for retry
                        return Err(e);
                    }
                    Err(e) => {
                        // Fatal error - log, update status to Error, and remove server immediately
                        let error_msg = e.to_string();
                        self.state.update_server_status(server_id, ServerStatus::Error(error_msg.clone())).await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to start server #{}: {}",
                            server_id.as_u32(),
                            error_msg
                        ));
                        // Remove the failed server immediately
                        self.state.remove_server(server_id).await;
                    }
                }

                // Create tasks attached to this server if provided
                if let Some(task_defs) = scheduled_tasks {
                    use crate::state::task::{ScheduledTask, TaskScope};

                    for task_def in task_defs {
                        // Determine delay: for one-shot use delay_secs, for recurring use delay_secs or interval_secs
                        let delay = if task_def.recurring {
                            task_def.delay_secs.or(task_def.interval_secs).unwrap_or(0)
                        } else {
                            task_def.delay_secs.unwrap_or(0)
                        };

                        let task = if task_def.recurring {
                            let interval_secs = task_def.interval_secs.unwrap_or(delay);
                            ScheduledTask::new_recurring(
                                crate::state::TaskId::new(0), // Temporary, will be assigned by add_task
                                task_def.task_id.clone(),
                                TaskScope::Server(server_id),
                                interval_secs,
                                task_def.max_executions,
                                task_def.instruction,
                                task_def.context,
                            )
                        } else {
                            ScheduledTask::new_one_shot(
                                crate::state::TaskId::new(0), // Temporary, will be assigned by add_task
                                task_def.task_id.clone(),
                                TaskScope::Server(server_id),
                                delay,
                                task_def.instruction,
                                task_def.context,
                            )
                        };

                        let task_id = self.state.add_task(task).await;

                        // TODO: Add script configuration support for scheduled tasks
                        // For now, tasks use LLM by default. Script support will be added in a future iteration.

                        let _ = status_tx.send(format!(
                            "[TASK] Created {} task '{}' (ID: {}) for server #{}",
                            if task_def.recurring { "recurring" } else { "one-shot" },
                            task_def.task_id,
                            task_id,
                            server_id.as_u32()
                        ));
                    }
                }
            }
            CommonAction::CloseServer { server_id } => {
                use crate::state::server::ServerStatus;

                // Close specific server
                let sid = crate::state::ServerId::new(server_id);

                // Mark server as Stopped instead of removing it (reaper will clean up after 30s)
                self.state
                    .update_server_status(sid, ServerStatus::Stopped)
                    .await;
                let _ =
                    status_tx.send(format!("[SERVER] Stopped server #{}", sid.as_u32()));

                // Clean up tasks associated with this server
                self.state.cleanup_server_tasks(sid).await;
                let _ = status_tx.send(format!(
                    "[TASK] Cleaned up tasks for server #{}",
                    sid.as_u32()
                ));

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
            CommonAction::CloseAllServers => {
                use crate::state::server::ServerStatus;

                // Close all servers
                let server_ids = self.state.get_all_server_ids().await;

                for server_id in server_ids {
                    // Mark server as Stopped instead of removing it (reaper will clean up after 30s)
                    self.state
                        .update_server_status(server_id, ServerStatus::Stopped)
                        .await;
                    let _ =
                        status_tx.send(format!("[SERVER] Stopped server #{}", server_id.as_u32()));

                    // Clean up tasks associated with this server
                    self.state.cleanup_server_tasks(server_id).await;
                    let _ = status_tx.send(format!(
                        "[TASK] Cleaned up tasks for server #{}",
                        server_id.as_u32()
                    ));
                }

                // Set mode to Idle
                self.state.set_mode(Mode::Idle).await;

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
                script_runtime,
                script_language: _,
                script_path: _,
                script_inline,
                script_handles,
            } => {
                use crate::scripting::types::ScriptUpdateOperation;

                let target_server_id = crate::state::ServerId::new(server_id);

                // Parse operation
                let op = ScriptUpdateOperation::from_str(&operation)
                    .unwrap_or(ScriptUpdateOperation::Set);

                match op {
                    ScriptUpdateOperation::Set => {
                        // Build new script configuration using selected mode
                        let selected_mode = self.state.get_selected_scripting_mode().await;
                        match crate::scripting::ScriptManager::build_config(
                            selected_mode,
                            script_runtime.as_deref(),
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
                                // Null values mean disable/unset the script
                                self.state.set_script_config(target_server_id, None).await;
                                let _ = status_tx.send(format!(
                                    "[INFO] Server #{} script disabled (null values provided)",
                                    target_server_id.as_u32()
                                ));
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
            CommonAction::ScheduleTask {
                task_id,
                recurring,
                delay_secs,
                interval_secs,
                max_executions,
                server_id,
                connection_id,
                client_id,
                instruction,
                context,
                script_runtime,
                script_language: _,
                script_path: _,
                script_inline,
                script_handles,
            } => {
                use crate::state::task::{ScheduledTask, TaskScope};

                // Determine scope: Connection > Server > Client > Global
                let scope = if let Some(conn_id_str) = connection_id {
                    // Connection scope requires server_id
                    if let Some(sid) = server_id {
                        let server_id_obj = crate::state::ServerId::new(sid);
                        match crate::server::connection::ConnectionId::from_string(&conn_id_str) {
                            Some(cid) => {
                                // Validate connection exists on server
                                if let Some(server) = self.state.get_server(server_id_obj).await {
                                    if server.connections.contains_key(&cid) {
                                        TaskScope::Connection(server_id_obj, cid)
                                    } else {
                                        let _ = status_tx.send(format!(
                                            "[ERROR] Connection {} not found on server #{}",
                                            conn_id_str, sid
                                        ));
                                        return Ok(());
                                    }
                                } else {
                                    let _ = status_tx.send(format!(
                                        "[ERROR] Server #{} not found for connection-scoped task",
                                        sid
                                    ));
                                    return Ok(());
                                }
                            }
                            None => {
                                let _ = status_tx.send(format!(
                                    "[ERROR] Invalid connection_id format: {}. Expected 'conn-123' or '123'",
                                    conn_id_str
                                ));
                                return Ok(());
                            }
                        }
                    } else {
                        let _ = status_tx.send(
                            "[ERROR] connection_id requires server_id to be specified".to_string(),
                        );
                        return Ok(());
                    }
                } else if let Some(sid) = server_id {
                    TaskScope::Server(crate::state::ServerId::new(sid))
                } else if let Some(cid) = client_id {
                    let client_id_obj = crate::state::ClientId::new(cid);
                    // Validate client exists
                    if self.state.get_client(client_id_obj).await.is_none() {
                        let _ = status_tx.send(format!(
                            "[ERROR] Client #{} not found for client-scoped task",
                            cid
                        ));
                        return Ok(());
                    }
                    TaskScope::Client(client_id_obj)
                } else {
                    TaskScope::Global
                };

                // Determine delay: for one-shot use delay_secs, for recurring use delay_secs or interval_secs
                let delay = if recurring {
                    delay_secs.or(interval_secs).unwrap_or(0)
                } else {
                    delay_secs.unwrap_or(0)
                };

                let task = if recurring {
                    let interval = interval_secs.unwrap_or(delay);
                    ScheduledTask::new_recurring(
                        crate::state::TaskId::new(0), // Temporary, will be assigned by add_task
                        task_id.clone(),
                        scope,
                        interval,
                        max_executions,
                        instruction,
                        context,
                    )
                } else {
                    ScheduledTask::new_one_shot(
                        crate::state::TaskId::new(0), // Temporary, will be assigned by add_task
                        task_id.clone(),
                        scope,
                        delay,
                        instruction,
                        context,
                    )
                };

                let task_id_num = self.state.add_task(task).await;

                // TODO: Add script configuration support for standalone scheduled tasks
                // For now, tasks use LLM by default. Script support will be added in a future iteration.
                let _ = script_runtime; // Silence unused variable warning
                let _ = script_inline; // Silence unused variable warning
                let _ = script_handles; // Silence unused variable warning

                if recurring {
                    let interval = interval_secs.unwrap_or(delay);
                    let max_info = if let Some(max) = max_executions {
                        format!(" (max {} executions)", max)
                    } else {
                        String::new()
                    };
                    let _ = status_tx.send(format!(
                        "[TASK] Scheduled recurring task '{}' (ID: {}) to execute every {}s{}",
                        task_id, task_id_num, interval, max_info
                    ));
                } else {
                    let _ = status_tx.send(format!(
                        "[TASK] Scheduled one-shot task '{}' (ID: {}) to execute in {}s",
                        task_id, task_id_num, delay
                    ));
                }
            }
            CommonAction::CancelTask { task_id } => {
                if let Some(task) = self.state.get_task(&task_id).await {
                    self.state.remove_task(task.id).await;
                    let _ = status_tx.send(format!("[TASK] Cancelled task '{}'", task_id));
                } else {
                    let _ = status_tx.send(format!("[WARN] Task '{}' not found", task_id));
                }
            }
            CommonAction::ListTasks => {
                let tasks = self.state.get_all_tasks().await;
                if tasks.is_empty() {
                    let _ = status_tx.send("[TASK] No scheduled tasks".to_string());
                } else {
                    let _ = status_tx.send(format!("[TASK] {} scheduled task(s):", tasks.len()));
                    for task in tasks {
                        let _ = status_tx.send(format!("  {}", task.format_for_prompt()));
                    }
                }
            }
            CommonAction::OpenClient {
                protocol,
                remote_addr,
                instruction,
                startup_params,
                initial_memory,
                script_runtime,
                script_language: _,
                script_path: _,
                script_inline,
                script_handles,
                scheduled_tasks,
            } => {
                use crate::state::client::{ClientInstance, ClientStatus};

                // Create client instance with temporary ID (add_client will assign real ID)
                let mut client = ClientInstance::new(
                    crate::state::ClientId::new(0),
                    remote_addr.clone(),
                    protocol.clone(),
                    instruction.clone(),
                );

                // Set optional fields
                if let Some(mem) = initial_memory {
                    client.memory = mem;
                }
                client.startup_params = startup_params.clone();

                // Add client to state (this allocates the real client ID)
                let client_id = self.state.add_client(client).await;

                // Set script configuration if provided
                if script_runtime.is_some() || script_inline.is_some() || script_handles.is_some() {
                    let selected_mode = self.state.get_selected_scripting_mode().await;
                    match crate::scripting::ScriptManager::build_config(
                        selected_mode,
                        script_runtime.as_deref(),
                        script_inline.as_deref(),
                        script_handles,
                    ) {
                        Ok(Some(config)) => {
                            let scripting_env = self.state.get_scripting_env().await;
                            if scripting_env.is_available(config.language) {
                                self.state.set_client_script_config(client_id, Some(config)).await;
                                let _ = status_tx.send("[INFO] Script configuration applied to client".to_string());
                            } else {
                                let _ = status_tx.send(format!(
                                    "[WARN] {} not available, script not configured",
                                    config.language.as_str()
                                ));
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            let _ = status_tx.send(format!("[ERROR] Failed to build script config: {}", e));
                        }
                    }
                }

                let _ = status_tx.send(format!(
                    "[CLIENT] Opening {} client #{} to {}...",
                    protocol,
                    client_id.as_u32(),
                    remote_addr
                ));

                // Start the client connection
                let llm_client = self.llm.clone();
                let status_tx_clone = status_tx.clone();
                match crate::cli::client_startup::start_client_by_id(
                    &self.state,
                    client_id,
                    &llm_client,
                    &status_tx_clone,
                )
                .await
                {
                    Ok(_) => {
                        // Client started successfully
                        let _ = status_tx.send(format!(
                            "[CLIENT] {} client #{} connected",
                            protocol,
                            client_id.as_u32()
                        ));

                        // Create scheduled tasks if provided
                        if let Some(task_defs) = scheduled_tasks {
                            for task_def in task_defs {
                                let delay = if task_def.recurring {
                                    task_def.delay_secs.or(task_def.interval_secs).unwrap_or(0)
                                } else {
                                    task_def.delay_secs.unwrap_or(0)
                                };

                                let task = if task_def.recurring {
                                    let interval_secs = task_def.interval_secs.unwrap_or(delay);
                                    crate::state::task::ScheduledTask::new_recurring(
                                        crate::state::TaskId::new(0),
                                        task_def.task_id.clone(),
                                        crate::state::task::TaskScope::Client(client_id),
                                        interval_secs,
                                        task_def.max_executions,
                                        task_def.instruction,
                                        task_def.context,
                                    )
                                } else {
                                    crate::state::task::ScheduledTask::new_one_shot(
                                        crate::state::TaskId::new(0),
                                        task_def.task_id.clone(),
                                        crate::state::task::TaskScope::Client(client_id),
                                        delay,
                                        task_def.instruction,
                                        task_def.context,
                                    )
                                };

                                let task_id_num = self.state.add_task(task).await;

                                let _ = status_tx.send(format!(
                                    "[TASK] Created {} task '{}' (ID: {}) for client #{}",
                                    if task_def.recurring {
                                        "recurring"
                                    } else {
                                        "one-shot"
                                    },
                                    task_def.task_id,
                                    task_id_num,
                                    client_id.as_u32()
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        // Connection failed
                        self.state
                            .update_client_status(client_id, ClientStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to connect {} client #{}: {}",
                            protocol,
                            client_id.as_u32(),
                            e
                        ));
                        return Err(e);
                    }
                }

                let _ = status_tx.send("__UPDATE_UI__".to_string());
            }
            CommonAction::CloseClient { client_id } => {
                use crate::state::client::ClientStatus;

                let cid = crate::state::ClientId::new(client_id);

                // Mark client as Disconnected
                self.state
                    .update_client_status(cid, ClientStatus::Disconnected)
                    .await;
                let _ = status_tx.send(format!("[CLIENT] Closed client #{}", cid.as_u32()));

                // Clean up tasks associated with this client
                self.state.cleanup_client_tasks(cid).await;
                let _ = status_tx.send(format!(
                    "[TASK] Cleaned up tasks for client #{}",
                    cid.as_u32()
                ));

                let _ = status_tx.send("__UPDATE_UI__".to_string());
            }
            CommonAction::CloseAllClients => {
                use crate::state::client::ClientStatus;

                let client_ids = self.state.get_all_client_ids().await;

                for client_id in client_ids {
                    // Mark client as Disconnected
                    self.state
                        .update_client_status(client_id, ClientStatus::Disconnected)
                        .await;
                    let _ = status_tx.send(format!("[CLIENT] Closed client #{}", client_id.as_u32()));

                    // Clean up tasks associated with this client
                    self.state.cleanup_client_tasks(client_id).await;
                    let _ = status_tx.send(format!(
                        "[TASK] Cleaned up tasks for client #{}",
                        client_id.as_u32()
                    ));
                }

                let _ = status_tx.send("__UPDATE_UI__".to_string());
            }
            CommonAction::ReconnectClient { client_id } => {
                let cid = crate::state::ClientId::new(client_id);

                // Reconnect the client
                let llm_client = self.llm.clone();
                let status_tx_clone = status_tx.clone();

                let _ = status_tx.send(format!("[CLIENT] Reconnecting client #{}...", cid.as_u32()));

                match crate::cli::client_startup::start_client_by_id(
                    &self.state,
                    cid,
                    &llm_client,
                    &status_tx_clone,
                )
                .await
                {
                    Ok(_) => {
                        let _ = status_tx.send(format!("[CLIENT] Client #{} reconnected", cid.as_u32()));
                    }
                    Err(e) => {
                        let _ = status_tx.send(format!(
                            "[ERROR] Failed to reconnect client #{}: {}",
                            cid.as_u32(),
                            e
                        ));
                        return Err(e);
                    }
                }

                let _ = status_tx.send("__UPDATE_UI__".to_string());
            }
            CommonAction::UpdateClientInstruction {
                client_id,
                instruction,
            } => {
                let cid = crate::state::ClientId::new(client_id);

                // Update client instruction
                self.state
                    .set_instruction_for_client(cid, instruction.clone())
                    .await;

                let _ = status_tx.send(format!(
                    "[CLIENT] Updated instruction for client #{}",
                    cid.as_u32()
                ));
                let _ = status_tx.send(format!("[CLIENT] New instruction: {}", instruction));
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
