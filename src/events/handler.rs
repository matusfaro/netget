//! Event handler - coordinates responses to events using LLM

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::info;

use super::errors::ActionExecutionError;
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

    /// Get a clone of the Ollama client
    pub fn get_llm_client(&self) -> OllamaClient {
        self.llm.clone()
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
                if let Some(log_level) = LogLevel::parse(&level) {
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

                ui.add_llm_message(
                    "[INFO] Testing web search approval with DuckDuckGo...".to_string(),
                );

                // Get web search mode and approval channel
                let web_search_mode = self.state.get_web_search_mode().await;
                let approval_tx = self.state.get_web_approval_channel().await;

                // Create a web search action for DuckDuckGo with a long path to test truncation
                let action = ToolAction::WebSearch {
                    query: "https://duckduckgo.com/?q=test+search+query+with+very+long+parameters&ia=web&category=general&filters=none".to_string(),
                };

                // Execute the tool (this will trigger approval prompt if in ASK mode)
                let result = execute_tool(
                    &action,
                    approval_tx.as_ref(),
                    web_search_mode,
                    Some(&self.state),
                )
                .await;

                // Display the result
                if result.success {
                    ui.add_llm_message("[INFO] Web search completed successfully".to_string());
                    ui.add_llm_message(format!("[DEBUG] Result: {}", result.result));
                } else {
                    ui.add_llm_message(format!("[ERROR] Web search failed: {}", result.result));
                }

                Ok(false)
            }
            UserCommand::SetFooterStatus { message } => {
                // This command is only supported in rolling TUI mode
                if message.is_some() {
                    ui.add_llm_message(
                        "Footer status command is only supported in rolling TUI mode".to_string(),
                    );
                }
                Ok(false)
            }
            UserCommand::StopAll => {
                self.handle_stop_all(ui).await?;
                Ok(false)
            }
            UserCommand::StopById { id } => {
                self.handle_stop_by_id(id, ui).await?;
                Ok(false)
            }
            UserCommand::Save { name, id } => {
                self.handle_save(name, id, ui).await?;
                Ok(false)
            }
            UserCommand::Load { name } => {
                self.handle_load(name, ui).await?;
                Ok(false)
            }
            #[cfg(feature = "sqlite")]
            UserCommand::Sqlite { db_id, query } => {
                self.handle_sqlite(db_id, query, ui).await?;
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
            UserCommand::ShowWebSearch => {
                // This command is only supported in rolling TUI mode
                ui.add_llm_message(
                    "Web search command is only supported in rolling TUI mode".to_string(),
                );
                Ok(false)
            }
            UserCommand::SetWebSearch { mode: _ } => {
                // This command is only supported in rolling TUI mode
                ui.add_llm_message(
                    "Web search command is only supported in rolling TUI mode".to_string(),
                );
                Ok(false)
            }
            UserCommand::ShowEventHandler => {
                // This command is only supported in rolling TUI mode
                ui.add_llm_message(
                    "Event handler command is only supported in rolling TUI mode".to_string(),
                );
                Ok(false)
            }
            UserCommand::SetEventHandler { mode: _ } => {
                // This command is only supported in rolling TUI mode
                ui.add_llm_message(
                    "Event handler command is only supported in rolling TUI mode".to_string(),
                );
                Ok(false)
            }
            UserCommand::ShowDocs { protocol } => {
                self.handle_show_docs(protocol, ui).await?;
                Ok(false)
            }
            UserCommand::ShowEnvironment => {
                self.handle_show_environment(ui).await?;
                Ok(false)
            }
            UserCommand::ShowUsage => {
                // This command is only supported in rolling TUI mode
                ui.add_llm_message(
                    "Usage command is only supported in rolling TUI mode".to_string(),
                );
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

        // Get protocol async actions if available
        let protocol_async_actions = if let Some(ref proto) = protocol {
            proto.get_async_actions(&self.state)
        } else {
            Vec::new()
        };

        // Get model, ensuring one is selected
        let current_model = self.state.get_ollama_model().await;
        let model = match crate::llm::ensure_model_selected(current_model.clone()).await {
            Ok(m) => m,
            Err(e) => {
                return Err(anyhow::anyhow!("Failed to ensure model is selected: {}", e));
            }
        };

        // If model was auto-selected (wasn't set before), notify via status_tx
        if current_model.is_none() {
            let _ = status_tx.send(format!(
                "⚠  Auto-selected model: {} (no model was configured)",
                model
            ));
        }

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
        // Enable open_server and open_client by default
        // LLM can still use read_server_documentation/read_client_documentation tools for detailed protocol info
        let is_open_server_enabled = true;
        let is_open_client_enabled = true;
        let mut available_actions = get_user_input_common_actions(
            selected_mode,
            &scripting_env,
            is_open_server_enabled,
            is_open_client_enabled,
        );
        available_actions.extend(get_all_tool_actions(web_search_mode));
        available_actions.extend(protocol_async_actions);

        // Get or create persistent conversation state
        let conversation_state = self.state.get_or_create_user_conversation_state().await;

        // Get rate limiter for user requests
        let rate_limiter = self.state.get_rate_limiter().await;

        let mut conversation = ConversationHandler::new(
            system_prompt,
            std::sync::Arc::new(llm_with_status),
            model,
            rate_limiter,
            crate::llm::RequestSource::User, // User input always waits for rate limits
        )
        .with_status_tx(status_tx.clone())
        .with_tracking(
            self.state.clone(),
            crate::state::app_state::ConversationSource::User,
            input.clone(),
        )
        .with_conversation_state(conversation_state);

        // Register conversation immediately so it shows in UI
        self.state
            .register_conversation(
                conversation.conversation_id().to_string(),
                crate::state::app_state::ConversationSource::User,
                input.clone(),
            )
            .await;

        // Mark as registered to prevent duplicate registration in generate_with_tools_and_retry
        conversation.mark_registered();

        // Add user input as a separate user message
        conversation.add_user_message(input);

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
                                    | CommonAction::CloseConnectionById { .. }
                            );

                            match self
                                .execute_server_management_action(common_action, &status_tx)
                                .await
                            {
                                Ok(_) => {
                                    // Action executed successfully
                                    if modifies_state {
                                        state_changed = true;
                                    }
                                }
                                Err(e)
                                    if e.is_retryable()
                                        && execution_attempts < MAX_EXECUTION_RETRIES =>
                                {
                                    // Retryable error (e.g., port conflict) - prepare to retry
                                    should_retry = true;
                                    retry_error = Some(e);
                                    break; // Stop processing actions, we'll retry
                                }
                                Err(e) => {
                                    // Non-retryable error or max retries exceeded
                                    let _ = status_tx
                                        .send(format!("[ERROR] Error executing action: {e}"));
                                }
                            }
                        }
                    }

                    // Update conversation state if server state changed
                    if state_changed && !should_retry {
                        conversation.update_current_state(&self.state, None).await;
                        let _ = status_tx.send(
                            "[DEBUG] Updated conversation state after server changes".to_string(),
                        );
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
                    let protocol_ref: Option<&dyn Server> =
                        protocol.as_ref().map(|p| p.as_ref() as &dyn Server);

                    // User input context - no specific server/client (global actions)
                    match execute_actions(action_values.clone(), &self.state, protocol_ref, None, None)
                        .await
                    {
                        Ok(result) => {
                            // Display messages
                            for msg in result.messages {
                                let _ = status_tx.send(msg);
                            }
                        }
                        Err(e) => {
                            let _ =
                                status_tx.send(format!("[ERROR] Failed to execute actions: {e}"));
                        }
                    }

                    // Success - break out of retry loop
                    break;
                }
                Err(e) => {
                    let _ = status_tx.send(format!("[ERROR] LLM error: {e}"));
                    // End conversation tracking since generate_with_tools_and_retry didn't complete
                    conversation.end_tracking().await;
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
                event_handlers,
                scheduled_tasks,
                feedback_instructions,
            } => {
                use crate::state::server::{ServerInstance, ServerStatus};

                // Parse protocol name using registry
                let protocol_name = match crate::protocol::server_registry::registry()
                    .parse_from_str(&base_stack)
                {
                    Some(name) => name,
                    None => {
                        let error_msg = format!(
                            "Unknown protocol '{}'. The protocol may not be enabled as a feature.",
                            base_stack
                        );
                        return Err(ActionExecutionError::Fatal(anyhow::anyhow!(error_msg)));
                    }
                };

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

                // Set feedback instructions if provided
                server.feedback_instructions = feedback_instructions;

                server.status = ServerStatus::Starting;

                // Add server to state (this assigns the real ID)
                let server_id = self.state.add_server(server).await;

                // Parse event handlers if provided
                if let Some(handlers_json) = event_handlers {
                    match Self::parse_event_handlers(handlers_json) {
                        Ok(config) => {
                            self.state
                                .set_event_handler_config(server_id, Some(config))
                                .await;
                            let _ = status_tx.send(
                                "[INFO] Event handler configuration applied to server".to_string(),
                            );
                        }
                        Err(e) => {
                            let _ = status_tx
                                .send(format!("[WARN] Failed to parse event handlers: {}", e));
                        }
                    }
                }

                // Spawn the server directly (no more message passing!)
                // Note: "Starting server" message is sent by start_server_by_id
                // Propagate port conflict errors for retry, but continue for other errors
                match server_startup::start_server_by_id(
                    &self.state,
                    server_id,
                    &self.llm,
                    status_tx,
                )
                .await
                {
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
                        self.state
                            .update_server_status(server_id, ServerStatus::Error(error_msg.clone()))
                            .await;
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
                            if task_def.recurring {
                                "recurring"
                            } else {
                                "one-shot"
                            },
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
                let _ = status_tx.send(format!("[SERVER] Stopped server #{}", sid.as_u32()));

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
                self.state.set_ollama_model(Some(model.clone())).await;
                let _ = status_tx.send(format!("Changed model to: {model}"));

                // Signal main loop to update UI
                let _ = status_tx.send("__UPDATE_UI__".to_string());
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
                event_handlers,
                scheduled_tasks,
                feedback_instructions,
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
                client.feedback_instructions = feedback_instructions;

                // Add client to state (this allocates the real client ID)
                let client_id = self.state.add_client(client).await;

                // Parse event handlers if provided
                if let Some(handlers_json) = event_handlers {
                    match Self::parse_event_handlers(handlers_json) {
                        Ok(config) => {
                            self.state
                                .set_client_event_handler_config(client_id, Some(config))
                                .await;
                            let _ = status_tx.send(
                                "[INFO] Event handler configuration applied to client".to_string(),
                            );
                        }
                        Err(e) => {
                            let _ = status_tx
                                .send(format!("[WARN] Failed to parse event handlers: {}", e));
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
                    let _ =
                        status_tx.send(format!("[CLIENT] Closed client #{}", client_id.as_u32()));

                    // Clean up tasks associated with this client
                    self.state.cleanup_client_tasks(client_id).await;
                    let _ = status_tx.send(format!(
                        "[TASK] Cleaned up tasks for client #{}",
                        client_id.as_u32()
                    ));
                }

                let _ = status_tx.send("__UPDATE_UI__".to_string());
            }
            CommonAction::CloseConnectionById { connection_id } => {
                use crate::server::connection::ConnectionId;

                let conn_id = ConnectionId::new(connection_id);
                let all_servers = self.state.get_all_servers().await;

                let mut found = false;
                for server in all_servers {
                    if server.connections.contains_key(&conn_id) {
                        self.state
                            .close_connection_on_server(server.id, conn_id)
                            .await;
                        let _ = status_tx.send(format!(
                            "[CONNECTION] Closed connection #{} on server #{}",
                            connection_id,
                            server.id.as_u32()
                        ));
                        found = true;
                        break;
                    }
                }

                if !found {
                    let _ =
                        status_tx.send(format!("[ERROR] Connection #{} not found", connection_id));
                }

                let _ = status_tx.send("__UPDATE_UI__".to_string());
            }
            CommonAction::ReconnectClient { client_id } => {
                let cid = crate::state::ClientId::new(client_id);

                // Reconnect the client
                let llm_client = self.llm.clone();
                let status_tx_clone = status_tx.clone();

                let _ =
                    status_tx.send(format!("[CLIENT] Reconnecting client #{}...", cid.as_u32()));

                match crate::cli::client_startup::start_client_by_id(
                    &self.state,
                    cid,
                    &llm_client,
                    &status_tx_clone,
                )
                .await
                {
                    Ok(_) => {
                        let _ = status_tx
                            .send(format!("[CLIENT] Client #{} reconnected", cid.as_u32()));
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

            #[cfg(feature = "sqlite")]
            CommonAction::CreateDatabase {
                name,
                is_memory,
                owner,
                schema_ddl,
            } => {
                use crate::state::DatabaseOwner;

                // Construct path based on is_memory flag
                let db_path = if is_memory {
                    ":memory:".to_string()
                } else {
                    format!("./netget_db_{}.db", name)
                };

                // Determine owner (default to global)
                let db_owner = if let Some(owner_str) = owner {
                    if owner_str == "global" {
                        DatabaseOwner::Global
                    } else if let Some(id_str) = owner_str.strip_prefix("server-") {
                        if let Ok(id) = id_str.parse::<u32>() {
                            DatabaseOwner::Server(crate::state::ServerId::new(id))
                        } else {
                            let _ = status_tx.send(format!(
                                "[ERROR] Invalid server ID in owner: {}",
                                owner_str
                            ));
                            return Ok(());
                        }
                    } else if let Some(id_str) = owner_str.strip_prefix("client-") {
                        if let Ok(id) = id_str.parse::<u32>() {
                            DatabaseOwner::Client(crate::state::ClientId::new(id))
                        } else {
                            let _ = status_tx.send(format!(
                                "[ERROR] Invalid client ID in owner: {}",
                                owner_str
                            ));
                            return Ok(());
                        }
                    } else {
                        let _ = status_tx.send(format!("[ERROR] Invalid owner format: {}", owner_str));
                        return Ok(());
                    }
                } else {
                    // Default to first server or global
                    if let Some(sid) = self.state.get_first_server_id().await {
                        DatabaseOwner::Server(sid)
                    } else if let Some(cid) = self.state.get_first_client_id().await {
                        DatabaseOwner::Client(cid)
                    } else {
                        DatabaseOwner::Global
                    }
                };

                // Create database
                match self
                    .state
                    .create_database(name.clone(), db_path.clone(), db_owner.clone(), schema_ddl.as_deref())
                    .await
                {
                    Ok(db_id) => {
                        let _ = status_tx.send(format!(
                            "[DB] Created database '{}' ({}) at {} (owner: {})",
                            name, db_id, db_path, db_owner
                        ));

                        // Show schema if provided
                        if let Some(db) = self.state.get_database(db_id).await {
                            if !db.tables.is_empty() {
                                let _ = status_tx.send(format!("[DB] Schema: {}", db.schema_summary()));
                            }
                        }
                    }
                    Err(e) => {
                        let _ = status_tx.send(format!("[ERROR] Failed to create database: {}", e));
                    }
                }
            }

            #[cfg(feature = "sqlite")]
            CommonAction::ExecuteSql {
                database_id,
                query,
            } => {
                let db_id = crate::state::DatabaseId::new(database_id);

                match self.state.execute_sql(db_id, &query).await {
                    Ok(result) => {
                        let formatted = result.format();
                        let _ = status_tx.send(format!("[DB] Query result:\n{}", formatted));

                        // Show updated row counts
                        if let Some(db) = self.state.get_database(db_id).await {
                            let _ = status_tx.send(format!("[DB] Database: {}", db.schema_summary()));
                        }
                    }
                    Err(e) => {
                        // Return SqlError to allow LLM retry with error context
                        return Err(crate::events::ActionExecutionError::SqlError {
                            database_id,
                            query: query.clone(),
                            error: e.to_string(),
                        });
                    }
                }
            }

            #[cfg(feature = "sqlite")]
            CommonAction::ListDatabases => {
                let databases = self.state.get_all_databases().await;
                if databases.is_empty() {
                    let _ = status_tx.send("[DB] No databases".to_string());
                } else {
                    let _ = status_tx.send(format!("[DB] {} database(s):", databases.len()));
                    for db in databases {
                        let _ = status_tx.send(format!("  {}", db.schema_summary()));
                    }
                }
            }

            #[cfg(feature = "sqlite")]
            CommonAction::DeleteDatabase { database_id } => {
                let db_id = crate::state::DatabaseId::new(database_id);

                match self.state.delete_database(db_id).await {
                    Ok(_) => {
                        let _ = status_tx.send(format!("[DB] Deleted database {}", db_id));
                    }
                    Err(e) => {
                        let _ = status_tx.send(format!("[ERROR] Failed to delete database: {}", e));
                    }
                }
            }

            CommonAction::ShowMessage { message } => {
                let _ = status_tx.send(format!("[CLIENT] {}", message));
            }
            CommonAction::ProvideFeedback { .. } => {
                // ProvideFeedback is handled by the action executor, not here
                // This match arm exists to satisfy exhaustiveness checking
            }
        }

        Ok(())
    }

    /// Parse event handlers from JSON array into EventHandlerConfig
    fn parse_event_handlers(
        handlers_json: Vec<serde_json::Value>,
    ) -> Result<crate::scripting::EventHandlerConfig> {
        use crate::scripting::{EventHandler, EventHandlerConfig, EventHandlerType, EventPattern};

        let mut config = EventHandlerConfig::new();

        for handler_json in handlers_json {
            // Parse event_pattern field
            let event_pattern = if let Some(pattern_str) =
                handler_json.get("event_pattern").and_then(|v| v.as_str())
            {
                EventPattern::from(pattern_str)
            } else {
                // Default to wildcard if not specified
                EventPattern::wildcard()
            };

            // Parse handler field
            let handler_type_json = handler_json.get("handler").ok_or_else(|| {
                anyhow::anyhow!("Missing 'handler' field in event handler configuration")
            })?;

            let handler_type_str = handler_type_json
                .get("type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'type' field in handler configuration"))?;

            let handler_type = match handler_type_str {
                "llm" => EventHandlerType::Llm,
                "script" => {
                    let language = handler_type_json
                        .get("language")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            anyhow::anyhow!("Missing 'language' field for script handler")
                        })?;
                    let code = handler_type_json
                        .get("code")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            anyhow::anyhow!("Missing 'code' field for script handler")
                        })?;
                    EventHandlerType::script(language, code)
                }
                "static" => {
                    let actions = handler_type_json
                        .get("actions")
                        .and_then(|v| v.as_array())
                        .ok_or_else(|| {
                            anyhow::anyhow!("Missing or invalid 'actions' field for static handler")
                        })?;
                    EventHandlerType::static_response(actions.clone())
                }
                _ => anyhow::bail!("Unknown handler type: {}", handler_type_str),
            };

            config.add_handler(EventHandler::new(event_pattern, handler_type));
        }

        Ok(config)
    }

    async fn handle_quit(&mut self, ui: &mut App) -> Result<()> {
        ui.add_llm_message("Quitting...".to_string());
        // The main event loop will handle the actual quit
        Ok(())
    }

    async fn handle_stop_all(&mut self, ui: &mut App) -> Result<()> {
        use crate::state::client::ClientStatus;
        use crate::state::server::ServerStatus;

        ui.add_llm_message("Stopping all servers, connections, and clients...".to_string());

        // Stop all servers
        let server_ids: Vec<_> = self.state.get_all_server_ids().await;
        for server_id in server_ids {
            self.state
                .update_server_status(server_id, ServerStatus::Stopped)
                .await;
            self.state.cleanup_server_tasks(server_id).await;
            ui.add_llm_message(format!("[SERVER] Stopped server #{}", server_id.as_u32()));
        }

        // Stop all clients
        let client_ids: Vec<_> = self.state.get_all_client_ids().await;
        for client_id in client_ids {
            self.state
                .update_client_status(client_id, ClientStatus::Disconnected)
                .await;
            self.state.cleanup_client_tasks(client_id).await;
            ui.add_llm_message(format!("[CLIENT] Stopped client #{}", client_id.as_u32()));
        }

        ui.add_llm_message("All servers and clients stopped.".to_string());
        Ok(())
    }

    async fn handle_stop_by_id(&mut self, id: u32, ui: &mut App) -> Result<()> {
        use crate::server::connection::ConnectionId;
        use crate::state::client::{ClientId, ClientStatus};
        use crate::state::server::{ServerId, ServerStatus};

        // Try to find what type of entity this ID corresponds to
        let mut found = false;

        // Check if it's a server
        let server_id = ServerId::new(id);
        if self.state.get_server(server_id).await.is_some() {
            self.state
                .update_server_status(server_id, ServerStatus::Stopped)
                .await;
            self.state.cleanup_server_tasks(server_id).await;
            ui.add_llm_message(format!("[SERVER] Stopped server #{}", id));
            found = true;
        }

        // Check if it's a client
        let client_id = ClientId::new(id);
        if self.state.get_client(client_id).await.is_some() {
            self.state
                .update_client_status(client_id, ClientStatus::Disconnected)
                .await;
            self.state.cleanup_client_tasks(client_id).await;
            ui.add_llm_message(format!("[CLIENT] Stopped client #{}", id));
            found = true;
        }

        // Check if it's a connection
        let connection_id = ConnectionId::new(id);
        let all_servers = self.state.get_all_servers().await;
        for server in all_servers {
            if server.connections.contains_key(&connection_id) {
                self.state
                    .close_connection_on_server(server.id, connection_id)
                    .await;
                ui.add_llm_message(format!(
                    "[CONNECTION] Closed connection #{} on server #{}",
                    id,
                    server.id.as_u32()
                ));
                found = true;
                break;
            }
        }

        if !found {
            ui.add_llm_message(format!(
                "No server, client, or connection found with ID #{}",
                id
            ));
        }

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
        let current_model = self
            .state
            .get_ollama_model()
            .await
            .unwrap_or_else(|| "None".to_string());

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
                    self.state.set_ollama_model(Some(model.clone())).await;
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

    /// Handle show environment command - display environment information
    async fn handle_show_environment(&mut self, ui: &mut App) -> Result<()> {
        use crate::protocol::{registry, CLIENT_REGISTRY};

        ui.add_llm_message("".to_string());
        ui.add_llm_message("=== NetGet Environment ===".to_string());
        ui.add_llm_message("".to_string());

        // System information
        ui.add_llm_message("System Information:".to_string());
        ui.add_llm_message(format!("  OS: {}", std::env::consts::OS));
        ui.add_llm_message(format!("  Architecture: {}", std::env::consts::ARCH));
        ui.add_llm_message(format!(
            "  Rust Version: {}",
            env!("CARGO_PKG_RUST_VERSION", "unknown")
        ));
        ui.add_llm_message(format!("  NetGet Version: {}", env!("CARGO_PKG_VERSION")));
        ui.add_llm_message("".to_string());

        // LLM configuration
        let model = self
            .state
            .get_ollama_model()
            .await
            .unwrap_or_else(|| "None".to_string());
        let web_search_mode = self.state.get_web_search_mode().await;
        let scripting_mode = self.state.get_selected_scripting_mode().await;

        ui.add_llm_message("LLM Configuration:".to_string());
        ui.add_llm_message(format!("  Ollama Model: {}", model));
        ui.add_llm_message(format!("  Web Search: {}", web_search_mode.as_str()));
        ui.add_llm_message(format!("  Scripting: {}", scripting_mode.as_str()));
        ui.add_llm_message("".to_string());

        // Get system capabilities
        let caps = self.state.get_system_capabilities().await;

        // Check server protocols
        let server_excluded = registry().get_excluded_protocols(&caps);
        let server_available = registry().get_available_protocols(&caps);

        // Check client protocols
        let client_excluded = CLIENT_REGISTRY.get_excluded_protocols(&caps);
        let client_available = CLIENT_REGISTRY.get_available_protocols(&caps);

        // System capabilities summary
        ui.add_llm_message("System Capabilities:".to_string());
        ui.add_llm_message(format!(
            "  Root Access: {}",
            if caps.is_root { "Yes" } else { "No" }
        ));
        ui.add_llm_message(format!(
            "  Privileged Ports (<1024): {}",
            if caps.can_bind_privileged_ports {
                "Yes"
            } else {
                "No"
            }
        ));
        ui.add_llm_message(format!(
            "  Raw Socket Access (pcap): {}",
            if caps.has_raw_socket_access {
                "Yes"
            } else {
                "No"
            }
        ));
        ui.add_llm_message("".to_string());

        // Summary
        let total_server_protocols = server_available.len() + server_excluded.len();
        let total_client_protocols = client_available.len() + client_excluded.len();

        ui.add_llm_message(format!(
            "Server Protocols: {} available, {} excluded (total {})",
            server_available.len(),
            server_excluded.len(),
            total_server_protocols
        ));
        ui.add_llm_message(format!(
            "Client Protocols: {} available, {} excluded (total {})",
            client_available.len(),
            client_excluded.len(),
            total_client_protocols
        ));
        ui.add_llm_message("".to_string());

        // Show excluded protocols if any
        if !server_excluded.is_empty() {
            ui.add_llm_message("Excluded Server Protocols:".to_string());
            let mut excluded_names: Vec<_> = server_excluded.keys().cloned().collect();
            excluded_names.sort();

            for protocol_name in excluded_names {
                if let Some(missing_deps) = server_excluded.get(&protocol_name) {
                    ui.add_llm_message(format!("  {}", protocol_name));
                    for dep in missing_deps {
                        ui.add_llm_message(format!("    ✗ {}: {}", dep.name(), dep.description()));
                        ui.add_llm_message(format!("      → {}", dep.installation_hint()));
                    }
                }
            }
            ui.add_llm_message("".to_string());
        }

        if !client_excluded.is_empty() {
            ui.add_llm_message("Excluded Client Protocols:".to_string());
            let mut excluded_names: Vec<_> = client_excluded.keys().cloned().collect();
            excluded_names.sort();

            for protocol_name in excluded_names {
                if let Some(missing_deps) = client_excluded.get(&protocol_name) {
                    ui.add_llm_message(format!("  {}", protocol_name));
                    for dep in missing_deps {
                        ui.add_llm_message(format!("    ✗ {}: {}", dep.name(), dep.description()));
                        ui.add_llm_message(format!("      → {}", dep.installation_hint()));
                    }
                }
            }
            ui.add_llm_message("".to_string());
        }

        if server_excluded.is_empty() && client_excluded.is_empty() {
            ui.add_llm_message("✓ All protocols are available!".to_string());
            ui.add_llm_message("".to_string());
        }

        Ok(())
    }

    async fn handle_save(&mut self, name: String, id: Option<u32>, ui: &mut App) -> Result<()> {
        use crate::state::client::ClientId;
        use crate::state::server::ServerId;
        use crate::utils::save_load;

        let path = if let Some(id_val) = id {
            // Save specific server or client by ID
            // Try server first
            let server_id = ServerId::new(id_val);
            if self.state.get_server(server_id).await.is_some() {
                match save_load::save_server(&self.state, server_id, &name).await {
                    Ok(path) => {
                        ui.add_llm_message(format!(
                            "[SAVE] Saved server #{} to: {}",
                            id_val,
                            path.display()
                        ));
                        path
                    }
                    Err(e) => {
                        ui.add_llm_message(format!(
                            "[ERROR] Failed to save server #{}: {}",
                            id_val, e
                        ));
                        return Ok(());
                    }
                }
            } else {
                // Try client
                let client_id = ClientId::new(id_val);
                if self.state.get_client(client_id).await.is_some() {
                    match save_load::save_client(&self.state, client_id, &name).await {
                        Ok(path) => {
                            ui.add_llm_message(format!(
                                "[SAVE] Saved client #{} to: {}",
                                id_val,
                                path.display()
                            ));
                            path
                        }
                        Err(e) => {
                            ui.add_llm_message(format!(
                                "[ERROR] Failed to save client #{}: {}",
                                id_val, e
                            ));
                            return Ok(());
                        }
                    }
                } else {
                    ui.add_llm_message(format!(
                        "[ERROR] No server or client found with ID #{}",
                        id_val
                    ));
                    return Ok(());
                }
            }
        } else {
            // Save all servers and clients
            match save_load::save_all(&self.state, &name).await {
                Ok(path) => {
                    let servers = self.state.get_all_servers().await;
                    let clients = self.state.get_all_clients().await;
                    ui.add_llm_message(format!(
                        "[SAVE] Saved {} server(s) and {} client(s) to: {}",
                        servers.len(),
                        clients.len(),
                        path.display()
                    ));
                    path
                }
                Err(e) => {
                    ui.add_llm_message(format!("[ERROR] Failed to save configuration: {}", e));
                    return Ok(());
                }
            }
        };

        ui.add_llm_message(format!(
            "[INFO] Use '/load {}' to restore this configuration",
            path.display()
        ));
        Ok(())
    }

    async fn handle_load(&mut self, name: String, ui: &mut App) -> Result<()> {
        use crate::utils::save_load;

        // Load actions from file
        let actions = match save_load::load_actions(&name).await {
            Ok(actions) => actions,
            Err(e) => {
                ui.add_llm_message(format!("[ERROR] Failed to load file '{}': {}", name, e));
                return Ok(());
            }
        };

        if actions.is_empty() {
            ui.add_llm_message(format!("[WARN] File '{}' contains no actions", name));
            return Ok(());
        }

        ui.add_llm_message(format!(
            "[LOAD] Loading {} action(s) from: {}",
            actions.len(),
            save_load::normalize_filename(&name)
        ));

        // Execute each action
        for (i, action) in actions.iter().enumerate() {
            // Try to parse as common action
            if let Ok(common_action) = crate::llm::actions::common::CommonAction::from_json(action)
            {
                use crate::llm::actions::common::CommonAction;

                match common_action {
                    CommonAction::OpenServer {
                        port,
                        base_stack,
                        send_first,
                        initial_memory,
                        instruction,
                        startup_params,
                        event_handlers,
                        scheduled_tasks,
                        feedback_instructions,
                    } => {
                        // Execute open_server action via server startup
                        match server_startup::start_server_from_action(
                            &self.state,
                            port,
                            &base_stack,
                            send_first,
                            initial_memory,
                            instruction.clone(),
                            startup_params,
                            event_handlers,
                            scheduled_tasks,
                            feedback_instructions,
                        )
                        .await
                        {
                            Ok(server_id) => {
                                ui.add_llm_message(format!(
                                    "[LOAD] Opened server #{} on port {} ({})",
                                    server_id.as_u32(),
                                    port,
                                    base_stack
                                ));
                            }
                            Err(e) => {
                                ui.add_llm_message(format!(
                                    "[ERROR] Failed to open server (action {}): {}",
                                    i + 1,
                                    e
                                ));
                            }
                        }
                    }
                    CommonAction::OpenClient {
                        protocol,
                        remote_addr,
                        instruction,
                        startup_params,
                        initial_memory,
                        event_handlers,
                        scheduled_tasks,
                        feedback_instructions,
                    } => {
                        // Execute open_client action via client startup
                        use crate::cli::client_startup;

                        match client_startup::start_client_from_action(
                            &self.state,
                            &protocol,
                            &remote_addr,
                            instruction.clone(),
                            startup_params,
                            initial_memory,
                            event_handlers,
                            scheduled_tasks,
                            feedback_instructions,
                            self.llm.clone(),
                        )
                        .await
                        {
                            Ok(client_id) => {
                                ui.add_llm_message(format!(
                                    "[LOAD] Opened client #{} to {} ({})",
                                    client_id.as_u32(),
                                    remote_addr,
                                    protocol
                                ));
                            }
                            Err(e) => {
                                ui.add_llm_message(format!(
                                    "[ERROR] Failed to open client (action {}): {}",
                                    i + 1,
                                    e
                                ));
                            }
                        }
                    }
                    _ => {
                        ui.add_llm_message(format!(
                            "[WARN] Skipping unsupported action type (action {})",
                            i + 1
                        ));
                    }
                }
            } else {
                ui.add_llm_message(format!("[WARN] Skipping invalid action (action {})", i + 1));
            }
        }

        ui.add_llm_message("[LOAD] Configuration loaded successfully".to_string());
        Ok(())
    }

    #[cfg(feature = "sqlite")]
    async fn handle_sqlite(
        &mut self,
        db_id: Option<u32>,
        query: Option<String>,
        ui: &mut App,
    ) -> Result<()> {
        match (db_id, query) {
            (None, None) => {
                // List all databases
                let databases = self.state.get_all_databases().await;
                if databases.is_empty() {
                    ui.add_llm_message("[DB] No databases".to_string());
                } else {
                    ui.add_llm_message(format!("[DB] {} database(s):", databases.len()));
                    for db in databases {
                        ui.add_llm_message(format!("  {}", db.schema_summary()));
                    }
                }
            }
            (Some(id), None) => {
                // Show schema for specific database
                let db_id_obj = crate::state::DatabaseId::new(id);
                if let Some(db) = self.state.get_database(db_id_obj).await {
                    ui.add_llm_message(format!("[DB] Database {}:", db_id_obj));
                    ui.add_llm_message(db.schema_summary());
                } else {
                    ui.add_llm_message(format!("[ERROR] Database {} not found", db_id_obj));
                }
            }
            (Some(id), Some(sql)) => {
                // Execute query on specific database
                let db_id_obj = crate::state::DatabaseId::new(id);
                match self.state.execute_sql(db_id_obj, &sql).await {
                    Ok(result) => {
                        let formatted = result.format();
                        ui.add_llm_message(format!("[DB] Query result:\n{}", formatted));
                    }
                    Err(e) => {
                        ui.add_llm_message(format!("[ERROR] SQL error: {}", e));
                    }
                }
            }
            (None, Some(sql)) => {
                // Execute query on first database
                let databases = self.state.get_all_databases().await;
                if databases.is_empty() {
                    ui.add_llm_message("[ERROR] No databases available".to_string());
                } else {
                    let db_id = databases[0].id;
                    match self.state.execute_sql(db_id, &sql).await {
                        Ok(result) => {
                            let formatted = result.format();
                            ui.add_llm_message(format!("[DB] Query result:\n{}", formatted));
                        }
                        Err(e) => {
                            ui.add_llm_message(format!("[ERROR] SQL error: {}", e));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
