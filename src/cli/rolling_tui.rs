//! Rolling terminal TUI - output flows like tail -f with sticky footer
//!
//! This module implements the interactive TUI mode using a rolling terminal
//! approach where output naturally scrolls into the terminal's scrollback buffer,
//! while input and connection info remain sticky at the bottom.

use anyhow::Result;
use crossterm::{
    cursor,
    event::{Event, EventStream, KeyCode, KeyModifiers},
    execute,
    style::{Print, ResetColor, SetForegroundColor},
    terminal,
};
use futures::StreamExt;
use std::io::{stdout, Write};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{interval, Instant};
use tracing::{debug, error, info};

use crate::events::{EventHandler, UserCommand};
use crate::llm::OllamaClient;
use crate::settings::Settings;
use crate::state::app_state::AppState;
use crate::ui::{app::LogLevel, App};

use super::input_state::InputState;
use super::sticky_footer::{ConnectionInfo, FooterContent, StickyFooter};
use super::theme::ColorPalette;

/// Format scripting mode for display in status bar
/// Returns "LLM", "Python", or "JavaScript" based on selected mode
fn format_scripting_mode(mode: crate::state::app_state::ScriptingMode) -> String {
    mode.as_str().to_string()
}

/// Run the interactive rolling TUI mode
pub async fn run_rolling_tui(
    state: AppState,
    mut app: App,
    mut event_handler: EventHandler,
    llm_client: OllamaClient,
    settings: Settings,
    args: &super::Args,
    palette: ColorPalette,
) -> Result<()> {
    info!("Starting rolling TUI mode");

    // Wrap settings in Arc<Mutex> for sharing with event handlers
    let settings = Arc::new(Mutex::new(settings));

    // Wrap palette in Arc for sharing
    let palette = Arc::new(palette);

    // Override model if specified in args, otherwise use settings
    let effective_model = if let Some(model) = &args.model {
        model.clone()
    } else {
        settings.lock().await.model.clone()
    };

    state.set_ollama_model(effective_model.clone()).await;
    app.connection_info.model = effective_model;

    // Load web search setting from settings file
    let web_search_mode = settings.lock().await.get_web_search_mode();
    state.set_web_search_mode(web_search_mode).await;

    // Setup terminal (raw mode only, no alternate screen)
    terminal::enable_raw_mode()?;

    // Get terminal size (use defaults if detection fails or returns 0, e.g., in PTY tests)
    let (width, height) = match terminal::size() {
        Ok((w, h)) if w > 0 && h > 0 => (w, h),
        _ => (80, 24), // Default to 80x24 if size detection fails or returns 0
    };

    // Create sticky footer with system capabilities
    let system_capabilities = state.get_system_capabilities().await;
    let mut footer = StickyFooter::new(width, height, system_capabilities, (*palette).clone())?;
    let scroll_height = footer.scroll_region_height();
    let footer_height = height.saturating_sub(scroll_height);

    // Create web approval channel for ASK mode
    let (web_approval_tx, mut web_approval_rx) = tokio::sync::mpsc::unbounded_channel();
    state.set_web_approval_channel(web_approval_tx).await;

    // BEFORE setting scrolling region, push any existing terminal content up
    // by printing newlines. This makes room for the footer without overwriting content.
    // Move to actual bottom of terminal using a large line number that will clamp.
    // Note: terminal::size() may return wrong values in PTY tests, so we use ESC[9999;1H
    // which moves to line 9999 (clamped to actual terminal height) instead of relying on detected height
    print!("\x1b[9999;1H"); // CSI 9999;1 H - Move to line 9999, column 1 (clamps to actual terminal bottom)
    stdout().flush()?;

    // Print footer_height newlines to push existing content up
    for _ in 0..footer_height {
        execute!(stdout(), Print("\n"))?;
    }
    stdout().flush()?;

    // Now set up scrolling region (lines 1 to scroll_region_height)
    // This tells the terminal that only these lines should scroll, keeping footer fixed
    // DECSTBM: ESC[<top>;<bottom>r - Set scrolling region
    print!("\x1b[1;{}r", scroll_height);
    stdout().flush()?;

    let scripting_mode = state.get_selected_scripting_mode().await;
    let scripting_status = format_scripting_mode(scripting_mode);
    let web_search_mode = state.get_web_search_mode().await;
    let event_handler_mode = state.get_event_handler_mode().await;

    footer.set_connection_info(ConnectionInfo {
        model: app.connection_info.model.clone(),
        scripting_env: scripting_status,
        web_search_mode,
        event_handler_mode,
    });
    footer.set_packet_stats(app.packet_stats.clone());
    footer.set_log_level(app.log_level);

    // Print welcome messages to scrolling region
    print_welcome_messages(&mut footer, &palette)?;

    // Render footer initially to position cursor correctly
    // Without this, the cursor sits at the terminal default position until first keystroke
    footer.render(&mut stdout())?;

    // Create status channel for server messages
    let (status_tx, mut status_rx) = mpsc::unbounded_channel::<String>();

    // Create keyboard event stream
    let mut event_stream = EventStream::new();

    // Create tick interval for UI updates
    let mut tick_interval = interval(Duration::from_millis(100));

    // Cleanup configuration constants
    const CLEANUP_INTERVAL_SECS: u64 = 5;
    const SERVER_CLEANUP_TIMEOUT_SECS: u64 = 30;
    const CONNECTION_CLEANUP_TIMEOUT_SECS: u64 = 10;
    const CONNECTIONLESS_CLEANUP_TIMEOUT_SECS: u64 = 10;

    // Create cleanup interval
    let mut cleanup_interval = interval(Duration::from_secs(CLEANUP_INTERVAL_SECS));

    // Create task execution interval (check every 1 second)
    let mut task_execution_interval = interval(Duration::from_secs(1));
    task_execution_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Create test interval for debugging footer behavior (disabled for stable snapshots)
    // Set to a very long duration so it doesn't fire during tests
    let mut test_interval = tokio::time::interval_at(
        Instant::now() + Duration::from_secs(3600), // Start in 1 hour
        Duration::from_secs(3600) // Tick every hour
    );
    test_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Counter for test heartbeats
    let mut _heartbeat_counter = 0u64;

    // Resize debouncing - store pending resize dimensions
    let mut pending_resize: Option<(u16, u16)> = None;
    const RESIZE_DEBOUNCE_MS: u64 = 100; // Wait 100ms after last resize before rendering

    // Main event loop
    info!("Entering main event loop");

    loop {
        // Drain status messages from spawned tasks
        let mut ui_needs_update = false;
        while let Ok(msg) = status_rx.try_recv() {
            if msg == "__UPDATE_UI__" {
                // Special signal to update UI from state
                ui_needs_update = true;
            } else {
                // Filter messages by log level
                let should_show = if msg.starts_with("[ERROR]") {
                    true
                } else if msg.starts_with("[WARN]") {
                    app.log_level >= LogLevel::Warn
                } else if msg.starts_with("[INFO]") {
                    app.log_level >= LogLevel::Info
                } else if msg.starts_with("[DEBUG]") {
                    app.log_level >= LogLevel::Debug
                } else if msg.starts_with("[TRACE]") {
                    app.log_level >= LogLevel::Trace
                } else {
                    // Unprefixed messages always show
                    true
                };

                if should_show {
                    print_output_line(&msg, &mut footer, &palette)?;
                    ui_needs_update = true;
                }
            }
        }

        // Render footer immediately if messages were printed to reposition cursor
        // This ensures cursor is in the input field before select! blocks
        if ui_needs_update {
            update_ui_from_state(&mut app, &state, &mut footer).await;
            footer.render(&mut stdout())?;
            ui_needs_update = false; // Reset flag since we just rendered
        }

        tokio::select! {
            // Debounce timer for resize events
            _ = tokio::time::sleep(Duration::from_millis(RESIZE_DEBOUNCE_MS)), if pending_resize.is_some() => {
                // Debounce period has passed, apply the resize
                if let Some((width, height)) = pending_resize {
                    footer.handle_resize(width, height);
                    update_ui_from_state(&mut app, &state, &mut footer).await;
                    footer.render(&mut stdout())?;
                    pending_resize = None;
                }
            }
            // Keyboard events
            maybe_event = event_stream.next() => {
                match maybe_event {
                    Some(Ok(Event::Resize(width, height))) => {
                        // Store the resize event but don't render yet - wait for debounce
                        pending_resize = Some((width, height));
                    }
                    Some(Ok(event)) => {
                        if handle_event(event, &mut app, &state, &mut event_handler, &status_tx, &mut footer, settings.clone(), palette.clone()).await? {
                            info!("Quit requested by user");
                            break; // Quit requested
                        }
                    }
                    Some(Err(e)) => {
                        error!("Keyboard event error: {}", e);
                    }
                    None => {
                        info!("Event stream ended unexpectedly");
                        break;
                    }
                }
            }

            // Web search approval requests
            Some(request) = web_approval_rx.recv() => {
                debug!("Received web approval request for: {}", request.url);

                // Store approval request in footer
                footer.pending_approval = Some(crate::cli::sticky_footer::PendingApproval {
                    url: request.url,
                    response_tx: request.response_tx,
                });

                // Re-render footer to show approval prompt
                footer.render(&mut stdout())?;
                ui_needs_update = false;
            }

            // Periodic tick for UI updates
            _ = tick_interval.tick() => {
                // Just triggers potential updates
            }

            // Execute due tasks
            _ = task_execution_interval.tick() => {
                execute_due_tasks(&state, &llm_client, &status_tx).await;
            }

            // Periodic cleanup of old servers and connections
            _ = cleanup_interval.tick() => {
                state.cleanup_old_servers(SERVER_CLEANUP_TIMEOUT_SECS).await;
                state.cleanup_closed_connections(CONNECTION_CLEANUP_TIMEOUT_SECS).await;
                state.cleanup_old_connections(CONNECTIONLESS_CLEANUP_TIMEOUT_SECS).await;
                state.cleanup_old_conversations().await;
                ui_needs_update = true;
            }
        }

        // Update UI after handling events
        if ui_needs_update {
            update_ui_from_state(&mut app, &state, &mut footer).await;
            footer.render(&mut stdout())?;
        }
    }

    // Cleanup terminal
    // Reset scrolling region to full terminal (DECSTBM with no args)
    print!("\x1b[r");
    // Clear the sticky footer before exiting
    clear_sticky_footer(&footer)?;
    terminal::disable_raw_mode()?;
    println!(); // Final newline

    // Save command history before exiting
    let _ = app.save_history();
    info!("Rolling TUI mode exited");

    Ok(())
}

/// Print welcome messages to the scrolling region
fn print_welcome_messages(footer: &mut StickyFooter, palette: &ColorPalette) -> Result<()> {
    let messages = vec![
        "NetGet - LLM-Controlled Server",
    ];

    for msg in messages {
        print_output_line(msg, footer, palette)?;
    }

    Ok(())
}

/// Print a line to stdout (scrolls naturally within scroll region - no flickering!)
fn print_output_line(line: &str, footer: &mut StickyFooter, palette: &ColorPalette) -> Result<()> {
    let mut stdout = stdout();

    // Move cursor to the LAST line of the scrolling region
    // The scrolling region is set to lines 1-scroll_region_height (1-indexed)
    // cursor::MoveTo uses 0-indexed coordinates, so last line is scroll_region_height - 1
    // When we print with \n, the scroll region will scroll naturally,
    // and the footer (outside the scroll region) will remain in place - no flickering!
    let scroll_height = footer.scroll_region_height();
    let last_scroll_line = scroll_height.saturating_sub(1); // 0-indexed

    // Position cursor at the last line of scroll region
    execute!(stdout, cursor::MoveTo(0, last_scroll_line))?;

    if line.starts_with("[ERROR]") {
        execute!(
            stdout,
            SetForegroundColor(palette.error),
            Print("✗ "),
            ResetColor,
            Print(line.strip_prefix("[ERROR]").unwrap()),
        )?;
    } else if line.starts_with("[WARN]") {
        execute!(
            stdout,
            SetForegroundColor(palette.warning),
            Print("⚠ "),
            ResetColor,
            Print(line.strip_prefix("[WARN]").unwrap()),
        )?;
    } else if line.starts_with("[INFO]") {
        execute!(
            stdout,
            SetForegroundColor(palette.info),
            Print("● "),
            ResetColor,
            Print(line.strip_prefix("[INFO]").unwrap()),
        )?;
    } else if line.starts_with("[DEBUG]") {
        execute!(
            stdout,
            SetForegroundColor(palette.debug),
            Print("○ "),
            ResetColor,
            Print(line.strip_prefix("[DEBUG]").unwrap()),
        )?;
    } else if line.starts_with("[TRACE]") {
        let content = line.strip_prefix("[TRACE]").unwrap();

        // Special handling for LLM request/response/prompt headers and conversation messages
        if content.trim_start().starts_with("LLM request:")
            || content.trim_start().starts_with("LLM response")
            || content.trim_start().starts_with("LLM prompt:")
            || content.trim_start().starts_with("JSON schema:")
            || content.trim_start().starts_with("Initial conversation:")
            || content.trim_start().starts_with("Conversation updated:")
        {
            // LLM headers: grey bullet, grey text
            execute!(
                stdout,
                SetForegroundColor(palette.trace),
                Print("· "),
                Print(content),
                ResetColor,
            )?;
        } else if content.trim_start().starts_with("Message ") {
            // Conversation messages: split at colon to show prefix in normal color, content in trace color
            if let Some(colon_pos) = content.find(':') {
                let prefix = &content[..=colon_pos]; // Include the colon
                let message_content = &content[colon_pos + 1..];
                execute!(
                    stdout,
                    SetForegroundColor(palette.trace),
                    Print("· "),
                    ResetColor,
                    Print(prefix),
                    SetForegroundColor(palette.trace),
                    Print(message_content),
                    ResetColor,
                )?;
            } else {
                // No colon found, just print normally
                execute!(
                    stdout,
                    SetForegroundColor(palette.trace),
                    Print("· "),
                    ResetColor,
                    Print(content),
                )?;
            }
        } else {
            // For all other TRACE content (including multi-line LLM output), keep grey
            execute!(
                stdout,
                SetForegroundColor(palette.trace),
                Print("· "),
                Print(content),
                ResetColor,
            )?;
        }

    } else if line.starts_with("[USER]") {
        execute!(
            stdout,
            SetForegroundColor(palette.user),
            Print("▶ "),
            ResetColor,
            Print(line.strip_prefix("[USER]").unwrap()),
        )?;
    } else if line.starts_with("[SERVER]") {
        execute!(
            stdout,
            SetForegroundColor(palette.server),
            Print("◆ "),
            ResetColor,
            Print(line.strip_prefix("[SERVER]").unwrap()),
        )?;
    } else if line.starts_with("[CONN]") {
        execute!(
            stdout,
            SetForegroundColor(palette.connection),
            Print("◇ "),
            ResetColor,
            Print(line.strip_prefix("[CONN]").unwrap()),
        )?;
    } else {
        execute!(stdout, Print(line))?;
    }

    // Print newline - this will scroll the terminal up by one line
    execute!(stdout, Print("\n"))?;
    stdout.flush()?;

    // IMPORTANT: After printing a line, decrement the blank lines buffer
    // This line now occupies what was previously a blank line at the top
    footer.decrement_blank_lines_buffer();

    Ok(())
}

/// Execute all tasks that are due
async fn execute_due_tasks(
    state: &AppState,
    llm_client: &OllamaClient,
    status_tx: &mpsc::UnboundedSender<String>,
) {
    use crate::state::task::TaskStatus;
    use std::time::Instant;

    let now = Instant::now();
    let tasks = state.get_all_tasks().await;

    for task in tasks {
        // Skip if not scheduled or not yet due
        if task.status != TaskStatus::Scheduled {
            continue;
        }

        if task.next_execution > now {
            continue;
        }

        // Mark as executing
        state
            .update_task_status(task.id, TaskStatus::Executing)
            .await;

        // Spawn task execution to avoid blocking
        let state_clone = state.clone();
        let llm_clone = llm_client.clone();
        let status_tx_clone = status_tx.clone();
        let task_clone = task.clone();

        tokio::spawn(async move {
            execute_single_task(state_clone, llm_clone, status_tx_clone, task_clone).await
        });
    }
}

/// Execute a single task
async fn execute_single_task(
    state: AppState,
    llm_client: OllamaClient,
    status_tx: mpsc::UnboundedSender<String>,
    task: crate::state::ScheduledTask,
) {
    use crate::llm::prompt::PromptBuilder;
    use crate::state::task::{TaskExecutionResult, TaskScope};

    let _ = status_tx.send(format!("[TASK] Executing task '{}'", task.name));

    // Get protocol actions if server, connection, or client-scoped
    let protocol_actions = match &task.scope {
        TaskScope::Server(server_id) | TaskScope::Connection(server_id, _) => {
            if let Some(protocol_name) = state.get_protocol_name(*server_id).await {
                if let Some(protocol) = crate::protocol::server_registry::registry().get(&protocol_name) {
                    protocol.get_sync_actions()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        }
        TaskScope::Client(client_id) => {
            if let Some(protocol_name) = state.get_protocol_name_for_client(*client_id).await {
                if let Some(protocol) = crate::protocol::client_registry::CLIENT_REGISTRY.get(&protocol_name) {
                    protocol.as_ref().get_sync_actions()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        }
        TaskScope::Global => Vec::new(),
    };

    // Build prompt
    let prompt = PromptBuilder::build_task_execution_prompt(&state, &task, protocol_actions).await;

    // Get current model
    let model = state.get_ollama_model().await;

    // Register task as conversation
    let conversation_source = match &task.scope {
        TaskScope::Global => crate::state::app_state::ConversationSource::Task { task_name: task.name.clone() },
        TaskScope::Server(server_id) => crate::state::app_state::ConversationSource::Task { task_name: format!("{}#{}",task.name, server_id.as_u32()) },
        TaskScope::Connection(server_id, conn_id) => crate::state::app_state::ConversationSource::Task { task_name: format!("{}#{}/{}", task.name, server_id.as_u32(), conn_id) },
        TaskScope::Client(client_id) => crate::state::app_state::ConversationSource::Task { task_name: format!("{}@{}", task.name, client_id.as_u32()) },
    };

    let truncated_instruction = if task.instruction.len() > 30 {
        format!("{}...", &task.instruction[..27])
    } else {
        task.instruction.clone()
    };

    // Create conversation handler with tracking
    let mut conversation = crate::llm::ConversationHandler::new(
        prompt.clone(),
        std::sync::Arc::new(llm_client.clone()),
        model.clone(),
    )
    .with_status_tx(status_tx.clone())
    .with_tracking(
        state.clone(),
        conversation_source,
        truncated_instruction,
    );

    // Add empty user message to trigger generation
    conversation.add_user_message("Execute the task.".to_string());

    // Generate with conversation handler (handles tracking automatically)
    let web_search_mode = state.get_web_search_mode().await;
    let actions = match conversation
        .generate_with_tools_and_retry(
            state.get_web_approval_channel().await,
            web_search_mode,
            Vec::new(), // No additional actions for tasks
        )
        .await
    {
        Ok(actions) => actions,
        Err(e) => {
            // Execution failed
            let error = format!("LLM call failed: {}", e);
            let _ = status_tx.send(format!("[ERROR] Task '{}' failed: {}", task.name, error));

            let result = TaskExecutionResult {
                success: false,
                actions: Vec::new(),
                error: Some(error),
            };

            handle_task_failure(&state, &status_tx, task, result).await;
            return;
        }
    };

    // Get protocol for execution (if server, connection, or client-scoped)
    let protocol = match &task.scope {
        TaskScope::Server(server_id) | TaskScope::Connection(server_id, _) => {
            state
                .get_protocol_name(*server_id)
                .await
                .and_then(|name| crate::protocol::server_registry::registry().get(&name))
        }
        TaskScope::Client(_client_id) => {
            // Client protocols are handled differently - they don't use the server protocol registry
            // For now, return None as task execution for clients needs client-specific implementation
            None
        }
        TaskScope::Global => None,
    };

    // Execute actions
    match crate::llm::execute_actions(actions.clone(), &state, protocol.as_deref())
        .await
    {
        Ok(_exec_result) => {
            // Success
            let _ = status_tx.send(format!(
                "[TASK] Task '{}' completed successfully",
                task.name
            ));

            let result = TaskExecutionResult {
                success: true,
                actions,
                error: None,
            };

            handle_task_success(&state, &status_tx, task, result).await;
        }
        Err(e) => {
            // Execution failed
            let error = format!("Action execution failed: {}", e);
            let _ = status_tx.send(format!("[ERROR] Task '{}' failed: {}", task.name, error));

            let result = TaskExecutionResult {
                success: false,
                actions,
                error: Some(error),
            };

            handle_task_failure(&state, &status_tx, task, result).await;
        }
    }
}

/// Handle task success
async fn handle_task_success(
    state: &AppState,
    status_tx: &mpsc::UnboundedSender<String>,
    task: crate::state::ScheduledTask,
    result: crate::state::TaskExecutionResult,
) {
    use crate::state::task::{TaskStatus, TaskType};
    use std::time::{Duration, Instant};

    // Record execution
    state.record_task_execution(task.id, &result).await;

    match &task.task_type {
        TaskType::OneShot { .. } => {
            // One-shot task completed
            state.update_task_status(task.id, TaskStatus::Completed).await;
            state.remove_task(task.id).await;
            let _ = status_tx.send(format!(
                "[TASK] One-shot task '{}' completed and removed",
                task.name
            ));
        }
        TaskType::Recurring {
            interval_secs,
            max_executions,
            executions_count,
        } => {
            // Check if max executions reached
            if let Some(max) = max_executions {
                if *executions_count >= *max {
                    state.update_task_status(task.id, TaskStatus::Completed).await;
                    state.remove_task(task.id).await;
                    let _ = status_tx.send(format!(
                        "[TASK] Recurring task '{}' reached max executions ({}) and removed",
                        task.name, max
                    ));
                    return;
                }
            }

            // Schedule next execution
            let next = Instant::now() + Duration::from_secs(*interval_secs);
            state.update_task_next_execution(task.id, next).await;
            state.update_task_status(task.id, TaskStatus::Scheduled).await;
        }
    }
}

/// Handle task failure with exponential backoff retry
async fn handle_task_failure(
    state: &AppState,
    status_tx: &mpsc::UnboundedSender<String>,
    task: crate::state::ScheduledTask,
    result: crate::state::TaskExecutionResult,
) {
    use crate::state::task::TaskStatus;
    use std::time::{Duration, Instant};

    const MAX_FAILURES: u64 = 5;
    const BACKOFF_BASE_SECS: u64 = 60; // 1 minute base backoff

    // Record execution
    state.record_task_execution(task.id, &result).await;

    let failure_count = task.failure_count + 1;

    if failure_count >= MAX_FAILURES {
        // Too many failures, disable task
        state
            .update_task_status(
                task.id,
                TaskStatus::Failed(result.error.unwrap_or_else(|| "Unknown error".to_string())),
            )
            .await;
        state.remove_task(task.id).await;
        let _ = status_tx.send(format!(
            "[ERROR] Task '{}' failed {} times, removing from schedule",
            task.name, MAX_FAILURES
        ));
    } else {
        // Retry with exponential backoff
        let backoff_secs = BACKOFF_BASE_SECS * 2u64.pow((failure_count - 1) as u32);
        let next = Instant::now() + Duration::from_secs(backoff_secs);

        state.update_task_next_execution(task.id, next).await;
        state.update_task_status(task.id, TaskStatus::Scheduled).await;

        let _ = status_tx.send(format!(
            "[WARN] Task '{}' failed (attempt {}/{}), retrying in {} seconds",
            task.name, failure_count, MAX_FAILURES, backoff_secs
        ));
    }
}

/// Clear the sticky footer area
fn clear_sticky_footer(footer: &StickyFooter) -> Result<()> {
    let mut stdout = stdout();
    let (_, height) = terminal::size()?;
    let footer_height = footer.calculate_footer_height();
    let footer_start = height.saturating_sub(footer_height);

    // Clear footer lines
    for line in footer_start..height {
        execute!(
            stdout,
            cursor::MoveTo(0, line),
            terminal::Clear(terminal::ClearType::CurrentLine),
        )?;
    }

    stdout.flush()?;
    Ok(())
}

/// Handle keyboard and other events
async fn handle_event(
    event: Event,
    app: &mut App,
    state: &AppState,
    event_handler: &mut EventHandler,
    status_tx: &mpsc::UnboundedSender<String>,
    footer: &mut StickyFooter,
    settings: Arc<Mutex<Settings>>,
    palette: Arc<ColorPalette>,
) -> Result<bool> {
    match event {
        Event::Key(key) => {
            handle_key_event(key.code, key.modifiers, app, state, event_handler, status_tx, footer, settings, palette).await
        }
        _ => Ok(false),
    }
}

/// Handle keyboard key events
async fn handle_key_event(
    key_code: KeyCode,
    modifiers: KeyModifiers,
    app: &mut App,
    state: &AppState,
    event_handler: &mut EventHandler,
    status_tx: &mpsc::UnboundedSender<String>,
    footer: &mut StickyFooter,
    settings: Arc<Mutex<Settings>>,
    palette: Arc<ColorPalette>,
) -> Result<bool> {
    // Handle web approval prompt first (if active)
    if let Some(approval) = footer.pending_approval.take() {
        use crate::state::app_state::{WebApprovalResponse, WebSearchMode};

        match (key_code, modifiers) {
            (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
                // Ctrl-C during approval - deny and quit
                debug!("User pressed Ctrl-C during approval - denying and quitting");
                let _ = approval.response_tx.send(WebApprovalResponse::Deny);
                return Ok(true); // Signal quit
            }
            (KeyCode::Char('y'), _) | (KeyCode::Char('Y'), _) => {
                debug!("User approved web search");
                let _ = approval.response_tx.send(WebApprovalResponse::Allow);
                footer.render(&mut stdout())?;
                return Ok(false);
            }
            (KeyCode::Char('n'), _) | (KeyCode::Char('N'), _) => {
                debug!("User denied web search");
                let _ = approval.response_tx.send(WebApprovalResponse::Deny);
                footer.render(&mut stdout())?;
                return Ok(false);
            }
            (KeyCode::Char('a'), _) | (KeyCode::Char('A'), _) => {
                debug!("User chose always allow - switching to ON mode");

                // Switch mode to ON
                state.set_web_search_mode(WebSearchMode::On).await;

                // Save to settings
                if let Err(e) = settings.lock().await.set_web_search_mode(WebSearchMode::On) {
                    error!("Failed to save web search mode: {}", e);
                }

                // Send response
                let _ = approval.response_tx.send(WebApprovalResponse::AlwaysAllow);

                // Update UI
                update_ui_from_state(app, state, footer).await;
                footer.render(&mut stdout())?;
                return Ok(false);
            }
            _ => {
                // Any other key - restore the approval and ignore
                footer.pending_approval = Some(approval);
                return Ok(false);
            }
        }
    }

    // Handle special keys first
    match key_code {
        // Ctrl+C to quit
        KeyCode::Char('c') | KeyCode::Char('C') if modifiers.contains(KeyModifiers::CONTROL) => {
            return Ok(true);
        }

        // Ctrl+E to toggle scripting ON/OFF
        KeyCode::Char('e') | KeyCode::Char('E') if modifiers.contains(KeyModifiers::CONTROL) => {
            let (new_mode, switched) = state.cycle_scripting_mode().await;

            if switched {
                let message = match new_mode {
                    crate::state::app_state::ScriptingMode::On => {
                        "Scripting enabled: LLM will choose runtime for each script"
                    }
                    crate::state::app_state::ScriptingMode::Off => {
                        "Scripting disabled: LLM will handle all requests directly"
                    }
                    crate::state::app_state::ScriptingMode::Python => {
                        "Scripting mode: Python (use /env to change)"
                    }
                    crate::state::app_state::ScriptingMode::JavaScript => {
                        "Scripting mode: JavaScript (use /env to change)"
                    }
                    crate::state::app_state::ScriptingMode::Go => {
                        "Scripting mode: Go (use /env to change)"
                    }
                    crate::state::app_state::ScriptingMode::Perl => {
                        "Scripting mode: Perl (use /env to change)"
                    }
                };
                print_output_line(message, footer, &palette)?;

                // Save the new scripting mode to settings
                let mode_str = new_mode.as_str().to_lowercase();
                if let Err(e) = settings.lock().await.set_scripting_mode(mode_str) {
                    error!("Failed to save scripting mode setting: {}", e);
                }

                update_ui_from_state(app, state, footer).await;
                footer.render(&mut stdout())?;
            }

            return Ok(false);
        }

        // Ctrl+L to cycle log level
        KeyCode::Char('l') | KeyCode::Char('L') if modifiers.contains(KeyModifiers::CONTROL) => {
            let new_level = app.log_level.cycle();
            app.set_log_level(new_level);
            footer.set_log_level(new_level);
            print_output_line(&format!("Log level set to: {}", new_level.as_str()), footer, &palette)?;
            footer.render(&mut stdout())?;
            return Ok(false);
        }

        // Ctrl+W to cycle web search mode (ON -> ASK -> OFF -> ON)
        KeyCode::Char('w') | KeyCode::Char('W') if modifiers.contains(KeyModifiers::CONTROL) => {
            let new_mode = state.cycle_web_search_mode().await;
            let message = match new_mode {
                crate::state::app_state::WebSearchMode::On => "Web search: ON - LLM may perform web searches",
                crate::state::app_state::WebSearchMode::Ask => "Web search: ASK - LLM will request approval before searching",
                crate::state::app_state::WebSearchMode::Off => "Web search: OFF - LLM cannot perform web searches",
            };
            print_output_line(message, footer, &palette)?;

            // Save the new web search mode to settings
            if let Err(e) = settings.lock().await.set_web_search_mode(new_mode) {
                error!("Failed to save web search setting: {}", e);
            }

            update_ui_from_state(app, state, footer).await;
            footer.render(&mut stdout())?;
            return Ok(false);
        }

        // Ctrl+H to cycle event handler mode (ANY -> SCRIPT -> STATIC -> LLM -> ANY)
        KeyCode::Char('h') | KeyCode::Char('H') if modifiers.contains(KeyModifiers::CONTROL) => {
            let new_mode = state.cycle_event_handler_mode().await;
            let message = match new_mode {
                crate::state::app_state::EventHandlerMode::Any => {
                    "Handler mode: ANY - LLM chooses handler types (script/static/llm) as appropriate"
                }
                crate::state::app_state::EventHandlerMode::Script => {
                    "Handler mode: SCRIPT - LLM must configure all events with script handlers"
                }
                crate::state::app_state::EventHandlerMode::Static => {
                    "Handler mode: STATIC - LLM must configure all events with static response handlers"
                }
                crate::state::app_state::EventHandlerMode::Llm => {
                    "Handler mode: LLM - LLM must configure all events to be handled by LLM (no scripts/static)"
                }
            };
            print_output_line(message, footer, &palette)?;

            update_ui_from_state(app, state, footer).await;
            footer.render(&mut stdout())?;
            return Ok(false);
        }

        // Ctrl+N or Alt+N to insert newline
        KeyCode::Char('n') | KeyCode::Char('N') if modifiers.contains(KeyModifiers::CONTROL) || modifiers.contains(KeyModifiers::ALT) => {
            footer.input_mut().insert_newline();
            update_slash_suggestions_and_render(app, footer, &mut stdout())?;
            return Ok(false);
        }

        // Enter to submit (plain enter only, not with modifiers)
        KeyCode::Enter if !modifiers.contains(KeyModifiers::SHIFT) && !modifiers.contains(KeyModifiers::CONTROL) && !modifiers.contains(KeyModifiers::ALT) => {
            let text = footer.input().text();
            if !text.is_empty() {
                // Add to history
                app.add_to_history(text.clone());

                // Parse command
                let command = UserCommand::parse(&text);

                // CRITICAL: Clear input and slash suggestions BEFORE executing command
                // This ensures the footer shrinks and scroll region is correct before we print output
                footer.input_mut().clear();
                app.update_slash_suggestions(&footer.input().text());

                // Update footer content (switch back to Normal mode since input is cleared)
                if app.slash_suggestions.is_empty() {
                    footer.set_content(FooterContent::Normal {
                        servers: app.servers.clone(),
                        clients: app.clients.clone(),
                        connections: app.connections.clone(),
                        expand_all: app.expand_all_connections,
                        conversations: app.conversations.clone(),
                    });
                }

                // Render footer now so scroll region is updated before command execution
                footer.render(&mut stdout())?;

                // IMPORTANT: For SetFooterStatus and TestOutput, we DON'T print the command echo
                // - SetFooterStatus: Avoids positioning issues during footer expansion/shrinking
                // - TestOutput: Direct scroll region manipulation makes the echo unnecessary
                let print_echo_before = !matches!(
                    command,
                    UserCommand::SetFooterStatus { .. } | UserCommand::TestOutput { .. }
                );

                if print_echo_before {
                    print_output_line(&format!("[USER] {}", text), footer, &palette)?;
                }

                // Handle command
                match command {
                    UserCommand::Status | UserCommand::ShowModel | UserCommand::ShowLogLevel | UserCommand::ShowScriptingEnv | UserCommand::ShowWebSearch | UserCommand::ShowEnvironment => {
                        // Handle status/info commands
                        handle_status_command(&command, app, state, event_handler, footer, &palette).await?;
                    }
                    UserCommand::ChangeModel { model } => {
                        state.set_ollama_model(model.clone()).await;
                        app.connection_info.model = model.clone();
                        print_output_line(&format!("Model changed to: {}", model), footer, &palette)?;
                        update_ui_from_state(app, state, footer).await;
                        footer.render(&mut stdout())?;
                    }
                    UserCommand::ChangeLogLevel { level } => {
                        if let Some(log_level) = crate::ui::app::LogLevel::from_str(&level) {
                            app.set_log_level(log_level);
                            footer.set_log_level(log_level);
                            print_output_line(&format!("Log level set to: {}", log_level.as_str()), footer, &palette)?;
                            footer.render(&mut stdout())?;
                        } else {
                            print_output_line(&format!("Unknown log level: {}", level), footer, &palette)?;
                        }
                    }
                    UserCommand::TestOutput { count } => {
                        // Generate test output lines using print_output_line (scrolling mechanism)
                        // This ensures content is properly preserved during footer expansion/shrinking
                        for i in 1..=count {
                            print_output_line(&format!("Test line {} of {}", i, count), footer, &palette)?;
                        }

                        // Re-render footer
                        footer.render(&mut stdout())?;
                    }
                    UserCommand::TestAsk => {
                        // Test web search approval by triggering a search
                        use crate::llm::actions::tools::{execute_tool, ToolAction};

                        print_output_line("[INFO] Testing web search approval with DuckDuckGo...", footer, &palette)?;

                        // Get web search mode and approval channel
                        let web_search_mode = state.get_web_search_mode().await;
                        let approval_tx = state.get_web_approval_channel().await;

                        // Create a web search action for DuckDuckGo with a long path to test truncation
                        let action = ToolAction::WebSearch {
                            query: "https://duckduckgo.com/?q=test+search+query+with+very+long+parameters&ia=web&category=general&filters=none".to_string(),
                        };

                        // Execute the tool asynchronously (this will trigger approval prompt if in ASK mode)
                        let status_tx_clone = status_tx.clone();
                        let state_clone = state.clone();
                        tokio::spawn(async move {
                            let result = execute_tool(&action, approval_tx.as_ref(), web_search_mode, Some(&state_clone)).await;

                            // Send result to status channel
                            if result.success {
                                let _ = status_tx_clone.send("[INFO] Web search completed successfully".to_string());
                                // Truncate result if too long
                                let result_preview = if result.result.len() > 500 {
                                    format!("{}... (truncated)", &result.result[..500])
                                } else {
                                    result.result.clone()
                                };
                                let _ = status_tx_clone.send(format!("[DEBUG] Result preview: {}", result_preview));
                            } else {
                                let _ = status_tx_clone.send(format!("[ERROR] Web search failed: {}", result.result));
                            }
                        });
                    }
                    UserCommand::SetFooterStatus { message } => {
                        use std::fs::OpenOptions;
                        use std::io::Write as IoWrite;

                        // Write debug info to file
                        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open("/tmp/netget_debug.log") {
                            let _ = writeln!(file, "[DEBUG] SetFooterStatus handler called with message: {:?}", message);
                        }

                        // Get current terminal dimensions from footer (terminal::size() returns 0 in PTY)
                        let term_width = footer.terminal_width();
                        let term_height = footer.terminal_height();

                        // Calculate old and new footer heights
                        let old_scroll_height = footer.scroll_region_height();
                        let old_footer_height = term_height.saturating_sub(old_scroll_height);
                        let old_footer_start = term_height.saturating_sub(old_footer_height);

                        // Set custom footer status message (this recalculates footer height)
                        footer.set_custom_status(message.clone());

                        let new_scroll_height = footer.scroll_region_height();
                        let new_footer_height = term_height.saturating_sub(new_scroll_height);

                        // Write footer height info to file
                        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open("/tmp/netget_debug.log") {
                            let _ = writeln!(file, "[DEBUG] Footer heights: old={}, new={}, term_height={}",
                                old_footer_height, new_footer_height, term_height);
                        }

                        // Handle footer size changes
                        if new_footer_height > old_footer_height {
                            // Footer is EXPANDING (e.g., 5 lines → 7 lines, increase by 2)
                            let lines_to_add = new_footer_height - old_footer_height;

                            // Try to consume from blank lines buffer first
                            let consumed = footer.consume_blank_lines_buffer(lines_to_add);
                            let lines_to_push = lines_to_add - consumed;

                            // Write debug info to file
                            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open("/tmp/netget_debug.log") {
                                let _ = writeln!(file, "[DEBUG-EXPAND] Footer expanding: old_height={}, new_height={}, lines_to_add={}, consumed={}, lines_to_push={}",
                                    old_footer_height, new_footer_height, lines_to_add, consumed, lines_to_push);
                            }

                            // If buffer didn't have enough space, push content up BEFORE changing scroll region
                            if lines_to_push > 0 {
                                // Move cursor to bottom of the OLD scroll region (0-indexed)
                                let last_old_scroll_line = old_scroll_height.saturating_sub(1);
                                execute!(stdout(), cursor::MoveTo(0, last_old_scroll_line))?;

                                // Print newlines to scroll content up within the OLD scroll region
                                // This preserves all content by scrolling it up before we shrink the region
                                for _ in 0..lines_to_push {
                                    execute!(stdout(), Print("\n"))?;
                                }
                                stdout().flush()?;
                            }

                            // NOW set the new (smaller) scrolling region
                            print!("\x1b[1;{}r", new_scroll_height);
                            stdout().flush()?;

                            // Footer.render() will clear and draw the footer area
                        } else if new_footer_height < old_footer_height {
                            // Footer is SHRINKING (e.g., 7 lines → 5 lines, decrease by 2)
                            let lines_to_remove = old_footer_height - new_footer_height;

                            // Add shrunk lines to blank lines buffer - they become available blank lines at top
                            footer.add_to_blank_lines_buffer(lines_to_remove);

                            // Write debug info to file
                            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open("/tmp/netget_debug.log") {
                                let _ = writeln!(file, "[DEBUG-SHRINK] Footer shrinking: lines_to_remove={}, buffer now={}",
                                    lines_to_remove, footer.blank_lines_buffer());
                            }

                            // Step 1: Clear the top N lines of the old footer (where N = lines_to_remove)
                            let blank_line = " ".repeat(term_width as usize);
                            for line_offset in 0..lines_to_remove {
                                execute!(
                                    stdout(),
                                    cursor::MoveTo(0, old_footer_start + line_offset),
                                    Print(&blank_line),
                                )?;
                            }
                            stdout().flush()?;

                            // Step 2: Update scrolling region to new height
                            print!("\x1b[1;{}r", new_scroll_height);
                            stdout().flush()?;
                        } else {
                            // Footer size UNCHANGED - no buffer manipulation needed
                            // Just log for debugging
                            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open("/tmp/netget_debug.log") {
                                let _ = writeln!(file, "[DEBUG-UNCHANGED] Footer size unchanged: height={}, buffer={}",
                                    new_footer_height, footer.blank_lines_buffer());
                            }
                        }

                        // Step 4 (all cases): Redraw the footer at the new position
                        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open("/tmp/netget_debug.log") {
                            let final_scroll_height = footer.scroll_region_height();
                            let final_footer_height = footer.terminal_height().saturating_sub(final_scroll_height);
                            let final_footer_start = footer.terminal_height().saturating_sub(final_footer_height);
                            let _ = writeln!(file, "[DEBUG] Before footer.render(): scroll_height={}, footer_height={}, footer_start={}",
                                final_scroll_height, final_footer_height, final_footer_start);
                        }
                        footer.render(&mut stdout())?;

                        // Command echo is suppressed for SetFooterStatus (see print_echo_before logic above)
                    }
                    UserCommand::ShowDocs { protocol } => {
                        use crate::docs;

                        if let Some(protocol_name) = protocol {
                            // Show detailed docs for specific protocol
                            match docs::show_protocol_docs(&protocol_name) {
                                Ok(docs_text) => {
                                    for line in docs_text.lines() {
                                        print_output_line(line, footer, &palette)?;
                                    }
                                }
                                Err(err_msg) => {
                                    print_output_line(&err_msg, footer, &palette)?;
                                }
                            }
                        } else {
                            // List all protocols
                            let docs_text = docs::list_all_protocols();
                            for line in docs_text.lines() {
                                print_output_line(line, footer, &palette)?;
                            }
                        }

                        footer.render(&mut stdout())?;
                    }
                    UserCommand::StopAll => {
                        // Stop all servers and clients
                        handle_stop_all(state, footer, &palette).await?;
                        update_ui_from_state(app, state, footer).await;
                        footer.render(&mut stdout())?;
                    }
                    UserCommand::StopById { id } => {
                        // Stop specific server, client, or connection by ID
                        handle_stop_by_id(id, state, footer, &palette).await?;
                        update_ui_from_state(app, state, footer).await;
                        footer.render(&mut stdout())?;
                    }
                    UserCommand::Quit => {
                        return Ok(true);
                    }
                    UserCommand::UnknownSlashCommand { command } => {
                        print_output_line(&format!("Unknown command: {}", command), footer, &palette)?;
                    }
                    UserCommand::Interpret { input: llm_input } => {
                        // Spawn async task to process with LLM
                        let mut handler_clone = event_handler.clone();
                        let status_tx_clone = status_tx.clone();
                        tokio::spawn(async move {
                            let _ = handler_clone.handle_interpret_with_actions(llm_input, status_tx_clone, None).await;
                        });
                    }
                    UserCommand::ChangeScriptingEnv { env } => {
                        // Parse the scripting environment
                        let mode = match env.to_lowercase().as_str() {
                            "on" | "auto" => Some(crate::state::app_state::ScriptingMode::On),
                            "off" | "llm" => Some(crate::state::app_state::ScriptingMode::Off),
                            "python" | "py" => Some(crate::state::app_state::ScriptingMode::Python),
                            "javascript" | "js" | "node" => Some(crate::state::app_state::ScriptingMode::JavaScript),
                            "go" | "golang" => Some(crate::state::app_state::ScriptingMode::Go),
                            "perl" => Some(crate::state::app_state::ScriptingMode::Perl),
                            _ => None,
                        };

                        if let Some(new_mode) = mode {
                            // Check if the environment is available
                            let scripting_env = state.get_scripting_env().await;
                            let available = match new_mode {
                                crate::state::app_state::ScriptingMode::On => true,
                                crate::state::app_state::ScriptingMode::Off => true,
                                crate::state::app_state::ScriptingMode::Python => scripting_env.python.is_some(),
                                crate::state::app_state::ScriptingMode::JavaScript => scripting_env.javascript.is_some(),
                                crate::state::app_state::ScriptingMode::Go => scripting_env.go.is_some(),
                                crate::state::app_state::ScriptingMode::Perl => scripting_env.perl.is_some(),
                            };

                            if available {
                                state.set_selected_scripting_mode(new_mode).await;
                                let message = match new_mode {
                                    crate::state::app_state::ScriptingMode::On => {
                                        "Scripting environment set to: ON (LLM will choose runtime for each script)"
                                    }
                                    crate::state::app_state::ScriptingMode::Off => {
                                        "Scripting environment set to: OFF (LLM will handle all requests directly)"
                                    }
                                    crate::state::app_state::ScriptingMode::Python => {
                                        "Scripting environment set to: Python (LLM will produce Python code)"
                                    }
                                    crate::state::app_state::ScriptingMode::JavaScript => {
                                        "Scripting environment set to: JavaScript (LLM will produce JavaScript code)"
                                    }
                                    crate::state::app_state::ScriptingMode::Go => {
                                        "Scripting environment set to: Go (LLM will produce Go code)"
                                    }
                                    crate::state::app_state::ScriptingMode::Perl => {
                                        "Scripting environment set to: Perl (LLM will produce Perl code)"
                                    }
                                };
                                print_output_line(message, footer, &palette)?;

                                // Save to settings
                                let mode_str = new_mode.as_str().to_lowercase();
                                if let Err(e) = settings.lock().await.set_scripting_mode(mode_str) {
                                    error!("Failed to save scripting mode setting: {}", e);
                                }

                                update_ui_from_state(app, state, footer).await;
                                footer.render(&mut stdout())?;
                            } else {
                                print_output_line(&format!("{} environment is not available on this system", new_mode), footer, &palette)?;
                            }
                        } else {
                            print_output_line(&format!("Unknown scripting environment: {}. Valid options: on (auto), off (llm), python, javascript, go, perl", env), footer, &palette)?;
                        }
                    }
                    UserCommand::SetWebSearch { mode } => {
                        state.set_web_search_mode(mode).await;
                        let message = match mode {
                            crate::state::app_state::WebSearchMode::On => "Web search: ON",
                            crate::state::app_state::WebSearchMode::Ask => "Web search: ASK - will request approval",
                            crate::state::app_state::WebSearchMode::Off => "Web search: OFF",
                        };
                        print_output_line(message, footer, &palette)?;

                        // Save the new web search mode to settings
                        if let Err(e) = settings.lock().await.set_web_search_mode(mode) {
                            error!("Failed to save web search setting: {}", e);
                        }

                        update_ui_from_state(app, state, footer).await;
                        footer.render(&mut stdout())?;
                    }
                }
            }

            // Re-render footer after command execution (content may have changed)
            footer.render(&mut stdout())?;
            return Ok(false);
        }

        // Up arrow - command history navigation
        KeyCode::Up if footer.input().is_on_first_line() => {
            navigate_history_previous(app, footer);
            update_slash_suggestions_and_render(app, footer, &mut stdout())?;
            return Ok(false);
        }

        // Down arrow - command history navigation
        KeyCode::Down if footer.input().is_on_last_line() => {
            navigate_history_next(app, footer);
            update_slash_suggestions_and_render(app, footer, &mut stdout())?;
            return Ok(false);
        }

        // Ctrl+A - move to start of line
        KeyCode::Char('a') | KeyCode::Char('A') if modifiers.contains(KeyModifiers::CONTROL) => {
            footer.input_mut().move_to_start_of_line();
            footer.render_input_only(&mut stdout())?;
            return Ok(false);
        }

        // Ctrl+E - move to end of line
        KeyCode::Char('e') | KeyCode::Char('E') if modifiers.contains(KeyModifiers::CONTROL) => {
            footer.input_mut().move_to_end_of_line();
            footer.render_input_only(&mut stdout())?;
            return Ok(false);
        }

        // Ctrl+K - delete to end of line
        KeyCode::Char('k') | KeyCode::Char('K') if modifiers.contains(KeyModifiers::CONTROL) => {
            footer.input_mut().delete_to_end_of_line();
            update_slash_suggestions_and_render(app, footer, &mut stdout())?;
            return Ok(false);
        }

        // Ctrl+U - delete entire line
        KeyCode::Char('u') | KeyCode::Char('U') if modifiers.contains(KeyModifiers::CONTROL) => {
            footer.input_mut().delete_line();
            update_slash_suggestions_and_render(app, footer, &mut stdout())?;
            return Ok(false);
        }

        // Ctrl+W - delete word
        KeyCode::Char('w') | KeyCode::Char('W') if modifiers.contains(KeyModifiers::CONTROL) => {
            footer.input_mut().delete_word();
            update_slash_suggestions_and_render(app, footer, &mut stdout())?;
            return Ok(false);
        }

        // E key - toggle expand all (if not typing)
        KeyCode::Char('e') | KeyCode::Char('E') if !modifiers.contains(KeyModifiers::CONTROL) && footer.input().text().is_empty() => {
            app.toggle_expand_all();
            update_ui_from_state(app, state, footer).await;
            footer.render(&mut stdout())?;
            return Ok(false);
        }

        _ => {}
    }

    // Try to handle with InputState
    if footer.input_mut().handle_key(key_code, modifiers) {
        update_slash_suggestions_and_render(app, footer, &mut stdout())?;
        return Ok(false);
    }

    Ok(false)
}

/// Navigate to previous command in history
fn navigate_history_previous(app: &mut App, footer: &mut StickyFooter) {
    if app.command_history.is_empty() {
        return;
    }

    let input = footer.input_mut();
    match app.history_position {
        None => {
            // Starting history navigation - save current input
            let current = input.text();
            if !current.is_empty() {
                app.history_temp_input = Some(current);
            }
            // Go to most recent command
            let pos = app.command_history.len() - 1;
            app.history_position = Some(pos);
            *input = InputState::from_lines(
                app.command_history[pos]
                    .lines()
                    .map(|s| s.to_string())
                    .collect(),
            );
            input.move_to_top();
        }
        Some(pos) if pos > 0 => {
            // Go to older command
            let new_pos = pos - 1;
            app.history_position = Some(new_pos);
            *input = InputState::from_lines(
                app.command_history[new_pos]
                    .lines()
                    .map(|s| s.to_string())
                    .collect(),
            );
            input.move_to_top();
        }
        _ => {
            // Already at oldest command, do nothing
        }
    }
}

/// Navigate to next command in history
fn navigate_history_next(app: &mut App, footer: &mut StickyFooter) {
    let input = footer.input_mut();
    match app.history_position {
        Some(pos) if pos < app.command_history.len() - 1 => {
            // Go to newer command
            let new_pos = pos + 1;
            app.history_position = Some(new_pos);
            *input = InputState::from_lines(
                app.command_history[new_pos]
                    .lines()
                    .map(|s| s.to_string())
                    .collect(),
            );
            input.move_to_bottom();
        }
        Some(_) => {
            // At newest command, restore temp input or clear
            app.history_position = None;
            let temp = app.history_temp_input.take().unwrap_or_default();
            *input = InputState::from_lines(temp.lines().map(|s| s.to_string()).collect());
            input.move_to_bottom();
        }
        None => {
            // Not in history mode, do nothing
        }
    }
}

/// Update slash suggestions and render footer intelligently
/// Only re-renders full footer if suggestions actually changed
fn update_slash_suggestions_and_render(
    app: &mut App,
    footer: &mut StickyFooter,
    stdout: &mut impl Write,
) -> Result<()> {
    // Store old suggestions before updating
    let old_suggestions = app.slash_suggestions.clone();

    // Update suggestions based on current input
    app.update_slash_suggestions(&footer.input().text());

    // Check if suggestions actually changed
    if old_suggestions != app.slash_suggestions {
        // Update footer content based on new suggestions
        if app.slash_suggestions.is_empty() {
            footer.set_content(FooterContent::Normal {
                servers: app.servers.clone(),
                clients: app.clients.clone(),
                connections: app.connections.clone(),
                expand_all: app.expand_all_connections,
                conversations: app.conversations.clone(),
            });
        } else {
            footer.set_content(FooterContent::SlashCommands {
                suggestions: app.slash_suggestions.clone(),
            });
        }
        // Re-render entire footer (content changed)
        footer.render(stdout)?;
    } else {
        // Only re-render input line (suggestions unchanged)
        footer.render_input_only(stdout)?;
    }

    Ok(())
}

/// Update UI with current application state
async fn update_ui_from_state(app: &mut App, state: &AppState, footer: &mut StickyFooter) {
    use crate::ui::app::{ClientDisplayInfo, ConnectionDisplayInfo, ServerDisplayInfo};

    // Track old footer height BEFORE updating content
    let old_scroll_height = footer.scroll_region_height();
    let term_height = footer.terminal_height();
    let old_footer_height = term_height.saturating_sub(old_scroll_height);

    app.connection_info.mode = state.get_mode().await.to_string();
    app.connection_info.model = state.get_ollama_model().await;

    // Update server list
    let servers = state.get_all_servers().await;
    app.servers = servers
        .iter()
        .map(|s| ServerDisplayInfo {
            id: format!("#{}", s.id.as_u32()),
            protocol: s.protocol_name.clone(),
            port: s.port,
            status: s.status.to_string(),
            connections: s.connections.len(),
        })
        .collect();

    // Update client list
    let clients = state.get_all_clients().await;
    app.clients = clients
        .iter()
        .map(|c| ClientDisplayInfo {
            id: format!("#{}", c.id.as_u32()),
            protocol: c.protocol_name.clone(),
            remote_addr: c.remote_addr.clone(),
            status: c.status.to_string(),
        })
        .collect();

    // Update connection list - collect into a temporary vec to avoid borrow issues
    let mut connections = Vec::new();
    for s in &servers {
        for conn in s.connections.values() {
            let network_conn_id = conn.id.to_string();
            let global_id = app.get_or_allocate_connection_id(network_conn_id);
            connections.push(ConnectionDisplayInfo {
                id: global_id,
                server_id: format!("#{}", s.id.as_u32()),
                address: conn.remote_addr.to_string(),
                state: match conn.status {
                    crate::state::server::ConnectionStatus::Active => "Active".to_string(),
                    crate::state::server::ConnectionStatus::Closed => "Closed".to_string(),
                },
            });
        }
    }
    app.connections = connections;

    // Fetch active conversations from state
    app.conversations = state.get_active_conversations().await;

    // Update footer content (this recalculates scroll region)
    if app.slash_suggestions.is_empty() {
        footer.set_content(FooterContent::Normal {
            servers: app.servers.clone(),
            clients: app.clients.clone(),
            connections: app.connections.clone(),
            expand_all: app.expand_all_connections,
            conversations: app.conversations.clone(),
        });
    } else {
        footer.set_content(FooterContent::SlashCommands {
            suggestions: app.slash_suggestions.clone(),
        });
    }

    // Update connection info
    if let Some(first_server) = servers.first() {
        app.connection_info.protocol = first_server.protocol_name.clone();
        if let Some(addr) = first_server.local_addr {
            app.connection_info.local_addr = Some(addr.to_string());
        }
    }

    let scripting_mode = state.get_selected_scripting_mode().await;
    let scripting_status = format_scripting_mode(scripting_mode);
    let web_search_mode = state.get_web_search_mode().await;
    let event_handler_mode = state.get_event_handler_mode().await;

    footer.set_connection_info(ConnectionInfo {
        model: app.connection_info.model.clone(),
        scripting_env: scripting_status,
        web_search_mode,
        event_handler_mode,
    });

    // CRITICAL: Handle footer size changes (expansion/shrinking)
    let new_scroll_height = footer.scroll_region_height();
    let new_footer_height = term_height.saturating_sub(new_scroll_height);

    if new_footer_height != old_footer_height {
        let term_width = footer.terminal_width();
        let old_footer_start = term_height.saturating_sub(old_footer_height);

        if new_footer_height > old_footer_height {
            // Footer is EXPANDING (e.g., connection added, causing footer to grow)
            let lines_to_add = new_footer_height - old_footer_height;

            // Try to consume from blank lines buffer first
            let consumed = footer.consume_blank_lines_buffer(lines_to_add);
            let lines_to_push = lines_to_add - consumed;

            // If buffer didn't have enough space, push content up BEFORE changing scroll region
            if lines_to_push > 0 {
                // Move cursor to bottom of the OLD scroll region (0-indexed)
                let last_old_scroll_line = old_scroll_height.saturating_sub(1);
                execute!(stdout(), cursor::MoveTo(0, last_old_scroll_line)).ok();

                // Print newlines to scroll content up within the OLD scroll region
                // This preserves all content by scrolling it up before we shrink the region
                for _ in 0..lines_to_push {
                    execute!(stdout(), Print("\n")).ok();
                }
                stdout().flush().ok();
            }

            // NOW set the new (smaller) scrolling region
            print!("\x1b[1;{}r", new_scroll_height);
            stdout().flush().ok();

            // Footer.render() will clear and draw the footer area

        } else if new_footer_height < old_footer_height {
            // Footer is SHRINKING (e.g., connection removed, causing footer to shrink)
            let lines_to_remove = old_footer_height - new_footer_height;

            // Add shrunk lines to blank lines buffer
            footer.add_to_blank_lines_buffer(lines_to_remove);

            // Clear the top N lines of the old footer
            let blank_line = " ".repeat(term_width as usize);
            for line_offset in 0..lines_to_remove {
                execute!(
                    stdout(),
                    cursor::MoveTo(0, old_footer_start + line_offset),
                    Print(&blank_line),
                ).ok();
            }
            stdout().flush().ok();

            // Update scrolling region to new height
            print!("\x1b[1;{}r", new_scroll_height);
            stdout().flush().ok();
        }
    }

    // NOTE: Callers are responsible for rendering the footer after this function
    // All call sites already do: update_ui_from_state() then footer.render()
}

/// Handle status/info commands
async fn handle_status_command(
    command: &UserCommand,
    app: &App,
    state: &AppState,
    event_handler: &mut EventHandler,
    footer: &mut StickyFooter,
    palette: &ColorPalette,
) -> Result<()> {
    match command {
        UserCommand::Status => {
            print_output_line("=== Server Status ===", footer, palette)?;
            if app.servers.is_empty() {
                print_output_line("No servers running", footer, palette)?;
            } else {
                for server in &app.servers {
                    print_output_line(
                        &format!(
                            "Server {}: {} on port {} - {}",
                            server.id, server.protocol, server.port, server.status
                        ),
                        footer,
                        palette,
                    )?;
                }
            }
        }
        UserCommand::ShowModel => {
            let current_model = state.get_ollama_model().await;
            print_output_line(&format!("Current model: {}", current_model), footer, palette)?;
            print_output_line("", footer, palette)?;
            print_output_line("Fetching available models...", footer, palette)?;

            // Fetch model list from Ollama via event handler's LLM client
            match event_handler.list_models().await {
                Ok(models) => {
                    if models.is_empty() {
                        print_output_line("No models found. Please pull a model first.", footer, palette)?;
                        print_output_line("Example: ollama pull llama3.2", footer, palette)?;
                    } else {
                        print_output_line(&format!("Available models ({}):", models.len()), footer, palette)?;
                        for model in &models {
                            if model == &current_model {
                                print_output_line(&format!("  * {} (current)", model), footer, palette)?;
                            } else {
                                print_output_line(&format!("    {}", model), footer, palette)?;
                            }
                        }
                        print_output_line("", footer, palette)?;
                        print_output_line("To change model, use: /model <name>", footer, palette)?;
                    }
                }
                Err(e) => {
                    print_output_line(&format!("Failed to fetch models: {}", e), footer, palette)?;
                    print_output_line("Make sure Ollama is running.", footer, palette)?;
                }
            }
        }
        UserCommand::ShowLogLevel => {
            print_output_line(
                &format!("Current log level: {}", app.log_level.as_str()),
                footer,
                palette,
            )?;
        }
        UserCommand::ShowWebSearch => {
            let mode = state.get_web_search_mode().await;
            let status = match mode {
                crate::state::app_state::WebSearchMode::On => "ON (always allowed)",
                crate::state::app_state::WebSearchMode::Ask => "ASK (requires approval)",
                crate::state::app_state::WebSearchMode::Off => "OFF (disabled)",
            };
            print_output_line(&format!("Web search mode: {}", status), footer, palette)?;
            print_output_line("", footer, palette)?;
            print_output_line("To change, use: /web on, /web ask, or /web off", footer, palette)?;
            print_output_line("Or press Ctrl+W to cycle through modes", footer, palette)?;
        }
        UserCommand::ShowScriptingEnv => {
            let mode = state.get_selected_scripting_mode().await;
            print_output_line(&format!("Current scripting mode: {}", mode), footer, palette)?;
            print_output_line("", footer, palette)?;
            print_output_line("To change, use: /script <env>", footer, palette)?;
        }
        UserCommand::ShowEnvironment => {
            print_output_line("=== Environment Information ===", footer, palette)?;
            print_output_line(&format!("Platform: {}", std::env::consts::OS), footer, palette)?;
            print_output_line(&format!("Architecture: {}", std::env::consts::ARCH), footer, palette)?;
            if let Ok(cwd) = std::env::current_dir() {
                print_output_line(&format!("Working directory: {}", cwd.display()), footer, palette)?;
            }
            print_output_line(&format!("Model: {}", state.get_ollama_model().await), footer, palette)?;
        }
        _ => {}
    }
    Ok(())
}

async fn handle_stop_all(
    state: &AppState,
    footer: &mut StickyFooter,
    palette: &ColorPalette,
) -> Result<()> {
    use crate::state::server::ServerStatus;
    use crate::state::client::ClientStatus;

    print_output_line("Stopping all servers, connections, and clients...", footer, palette)?;

    // Stop all servers
    let server_ids: Vec<_> = state.get_all_server_ids().await;
    for server_id in server_ids {
        state.update_server_status(server_id, ServerStatus::Stopped).await;
        state.cleanup_server_tasks(server_id).await;
        print_output_line(&format!("[SERVER] Stopped server #{}", server_id.as_u32()), footer, palette)?;
    }

    // Stop all clients
    let client_ids: Vec<_> = state.get_all_client_ids().await;
    for client_id in client_ids {
        state.update_client_status(client_id, ClientStatus::Disconnected).await;
        state.cleanup_client_tasks(client_id).await;
        print_output_line(&format!("[CLIENT] Stopped client #{}", client_id.as_u32()), footer, palette)?;
    }

    print_output_line("All servers and clients stopped.", footer, palette)?;
    Ok(())
}

async fn handle_stop_by_id(
    id: u32,
    state: &AppState,
    footer: &mut StickyFooter,
    palette: &ColorPalette,
) -> Result<()> {
    use crate::state::server::{ServerId, ServerStatus};
    use crate::state::client::{ClientId, ClientStatus};
    use crate::server::connection::ConnectionId;

    // Try to find what type of entity this ID corresponds to
    let mut found = false;

    // Check if it's a server
    let server_id = ServerId::new(id);
    if state.get_server(server_id).await.is_some() {
        state.update_server_status(server_id, ServerStatus::Stopped).await;
        state.cleanup_server_tasks(server_id).await;
        print_output_line(&format!("[SERVER] Stopped server #{}", id), footer, palette)?;
        found = true;
    }

    // Check if it's a client
    let client_id = ClientId::new(id);
    if state.get_client(client_id).await.is_some() {
        state.update_client_status(client_id, ClientStatus::Disconnected).await;
        state.cleanup_client_tasks(client_id).await;
        print_output_line(&format!("[CLIENT] Stopped client #{}", id), footer, palette)?;
        found = true;
    }

    // Check if it's a connection
    let connection_id = ConnectionId::new(id);
    let all_servers = state.get_all_servers().await;
    for server in all_servers {
        if server.connections.contains_key(&connection_id) {
            state.close_connection_on_server(server.id, connection_id).await;
            print_output_line(&format!("[CONNECTION] Closed connection #{} on server #{}", id, server.id.as_u32()), footer, palette)?;
            found = true;
            break;
        }
    }

    if !found {
        print_output_line(&format!("No server, client, or connection found with ID #{}", id), footer, palette)?;
    }

    Ok(())
}
