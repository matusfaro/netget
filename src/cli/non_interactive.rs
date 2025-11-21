//! Non-interactive mode execution
//!
//! This module handles execution when NetGet runs without the TUI,
//! processing a single prompt and outputting results to stdout/stderr.

use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tracing::{debug, info};

use crate::events::EventHandler;
use crate::llm::OllamaClient;
use crate::settings::Settings;
use crate::state::app_state::{AppState, Mode};

/// Run NetGet in non-interactive mode with the given prompt
pub async fn run_non_interactive(
    prompt: String,
    args: &super::Args,
    settings: Settings,
) -> Result<()> {
    info!("Starting NetGet in non-interactive mode");
    debug!("Prompt: {}", prompt);

    // Create application state
    let ollama_url = args.ollama_url.clone().unwrap_or_else(|| "http://localhost:11434".to_string());
    let state = AppState::new_with_options(args.include_disabled_protocols, args.ollama_lock, ollama_url);

    // Configure rate limiter from CLI args
    let rate_limiter_config = args.build_rate_limiter_config();
    state.configure_rate_limiter(rate_limiter_config).await?;

    // Determine configured model: args override settings
    let configured_model = args.model.clone().or(settings.model.clone());

    // Select or validate model from Ollama (non-interactive = exit on error)
    let ollama_url_for_model = args.ollama_url.as_deref().unwrap_or("http://localhost:11434");
    let selected_model = crate::llm::select_or_validate_model(configured_model, false, ollama_url_for_model)
        .await?
        .ok_or_else(|| anyhow::anyhow!("No model available"))?;

    info!("✓  Using model: {}", selected_model);
    state.set_ollama_model(Some(selected_model)).await;

    // Determine scripting mode with priority: CLI arg > saved setting > auto-detected
    let mode_to_set = if let Some(mode) = args.parse_scripting_mode()? {
        Some(mode)
    } else {
        settings.parse_scripting_mode()
    };

    if let Some(mode) = mode_to_set {
        // Validate that the requested environment is available
        let scripting_env = state.get_scripting_env().await;
        let available = match mode {
            crate::state::app_state::ScriptingMode::On => true, // LLM chooses runtime
            crate::state::app_state::ScriptingMode::Off => true, // Always available
            crate::state::app_state::ScriptingMode::Python => scripting_env.python.is_some(),
            crate::state::app_state::ScriptingMode::JavaScript => {
                scripting_env.javascript.is_some()
            }
            crate::state::app_state::ScriptingMode::Go => scripting_env.go.is_some(),
            crate::state::app_state::ScriptingMode::Perl => scripting_env.perl.is_some(),
        };

        if !available {
            anyhow::bail!(
                "{} environment is not available on this system. Please install it or choose a different environment.",
                mode
            );
        }

        state.set_selected_scripting_mode(mode).await;
        debug!("Using scripting mode: {}", mode);
    }

    // Apply event handler mode from CLI if provided
    if let Some(handler_mode) = args.parse_event_handler_mode()? {
        state.set_event_handler_mode(handler_mode).await;
        debug!("Using event handler mode: {}", handler_mode);
    }

    // Load web search setting from settings file
    // In non-interactive mode, ASK mode is not supported (no way to prompt user)
    // so we convert ASK to OFF
    let mut web_search_mode = settings.get_web_search_mode();
    if web_search_mode == crate::state::app_state::WebSearchMode::Ask {
        debug!("Web search mode ASK is not supported in non-interactive mode, using OFF instead");
        web_search_mode = crate::state::app_state::WebSearchMode::Off;
    }
    state.set_web_search_mode(web_search_mode).await;
    debug!("Web search mode: {:?}", web_search_mode);

    // Create event handler and LLM client
    let lock_enabled = state.get_ollama_lock_enabled().await;
    let ollama_url = args.ollama_url.as_deref().unwrap_or("http://localhost:11434");
    let llm = OllamaClient::new_with_options(ollama_url, lock_enabled)
        .with_mock_config_file(args.mock_config_file.clone());

    // Store the configured LLM client in state so spawned servers can use it
    state.set_llm_client(llm.clone()).await;

    let mut event_handler = EventHandler::new(state.clone(), llm.clone());

    // Create status channel for messages from spawned servers
    let (status_tx, mut status_rx) = mpsc::unbounded_channel::<String>();

    // Spawn a background task to forward status messages to stdout in real-time
    // This ensures the test helper can see server startup messages as they happen
    let _status_forwarder = tokio::spawn(async move {
        use std::io::{self, Write};
        while let Some(msg) = status_rx.recv().await {
            // Skip internal control messages
            if !msg.starts_with("__") {
                let clean_msg = msg
                    .strip_prefix("[INFO] ")
                    .unwrap_or(&msg)
                    .strip_prefix("[ERROR] ")
                    .unwrap_or(&msg)
                    .strip_prefix("[WARN] ")
                    .unwrap_or(&msg)
                    .strip_prefix("[DEBUG] ")
                    .unwrap_or(&msg);
                println!("{clean_msg}");
                // Explicitly flush stdout to ensure message is visible immediately
                let _ = io::stdout().flush();
            }
        }
    });

    // Yield to allow the forwarder task to start
    tokio::task::yield_now().await;

    // Call handler directly - no need for separate task!
    // The handler will spawn servers directly now
    event_handler
        .handle_interpret_with_actions(prompt, status_tx.clone(), None)
        .await?;

    // Give spawned servers a moment to finish sending their startup messages
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Flush stdout to ensure all messages are visible to test helper
    {
        use std::io::Write;
        std::io::stdout().flush().ok();
    }

    // Check if we're in server mode
    if state.get_mode().await == Mode::Server {
        // Create a new status channel for the server
        // (the original status_rx was consumed by the forwarder task above)
        let (_new_status_tx, new_status_rx) = mpsc::unbounded_channel::<String>();
        return run_server(&state, llm, new_status_rx).await;
    }

    Ok(())
}

/// Run a server in non-interactive mode
async fn run_server(
    state: &AppState,
    llm: OllamaClient,
    mut status_rx: mpsc::UnboundedReceiver<String>,
) -> Result<()> {
    // Create status channel for server messages
    let (status_tx, mut server_status_rx) = mpsc::unbounded_channel::<String>();

    // Server should already be started by the interpret loop above
    // Just verify it exists and print status
    if let Some(server_id) = state.get_first_server_id().await {
        println!(
            "Server #{} is running. Press Ctrl+C to stop.",
            server_id.as_u32()
        );
        println!("Waiting for connections...\n");
    } else {
        return Err(anyhow::anyhow!(
            "No server configured. Use a command like 'listen on port 8080 via http'"
        ));
    }

    // Set up Ctrl+C handler
    let shutdown = Arc::new(Mutex::new(false));
    let shutdown_clone = shutdown.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        let mut shutdown = shutdown_clone.lock().await;
        *shutdown = true;
    });

    // Set up task execution ticker (execute tasks every 1 second, same as TUI mode)
    use tokio::time::{interval, Duration};
    let mut task_execution_interval = interval(Duration::from_secs(1));

    // Main event loop
    loop {
        tokio::select! {
            // Check for shutdown
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                if *shutdown.lock().await {
                    println!("\nShutting down server...");
                    break;
                }

                // Process status messages from handler (drain remaining)
                while let Ok(msg) = status_rx.try_recv() {
                    if !msg.starts_with("__") {
                        println!("[STATUS] {msg}");
                    }
                }

                // Sleep briefly to avoid busy waiting
                tokio::time::sleep(Duration::from_millis(100)).await;

                // Process server status messages
                while let Ok(msg) = server_status_rx.try_recv() {
                    println!("[STATUS] {msg}");
                }
            }

            // Execute due tasks every 1 second
            _ = task_execution_interval.tick() => {
                crate::cli::rolling_tui::execute_due_tasks_public(state, &llm, &status_tx).await;
            }
        }
    }

    println!("Server stopped.");
    Ok(())
}

/// Run NetGet in non-interactive mode with actions JSON (--load or piped JSON)
pub async fn run_with_actions(
    actions: Vec<serde_json::Value>,
    args: &super::Args,
    settings: Settings,
) -> Result<()> {
    info!("Starting NetGet in non-interactive mode (actions JSON)");
    debug!("Loading {} actions", actions.len());

    // Create application state
    let ollama_url = args.ollama_url.clone().unwrap_or_else(|| "http://localhost:11434".to_string());
    let state = AppState::new_with_options(args.include_disabled_protocols, args.ollama_lock, ollama_url);

    // Configure rate limiter from CLI args
    let rate_limiter_config = args.build_rate_limiter_config();
    state.configure_rate_limiter(rate_limiter_config).await?;

    // Determine scripting mode
    let mode_to_set = if let Some(mode) = args.parse_scripting_mode()? {
        Some(mode)
    } else {
        settings.parse_scripting_mode()
    };

    if let Some(mode) = mode_to_set {
        state.set_selected_scripting_mode(mode).await;
    }

    // Apply event handler mode from CLI if provided
    if let Some(handler_mode) = args.parse_event_handler_mode()? {
        state.set_event_handler_mode(handler_mode).await;
    }

    // Setup web search mode
    let mut web_search_mode = settings.get_web_search_mode();
    if web_search_mode == crate::state::app_state::WebSearchMode::Ask {
        web_search_mode = crate::state::app_state::WebSearchMode::Off;
    }
    state.set_web_search_mode(web_search_mode).await;

    // Create LLM client
    let lock_enabled = state.get_ollama_lock_enabled().await;
    let ollama_url = args.ollama_url.as_deref().unwrap_or("http://localhost:11434");
    let llm = OllamaClient::new_with_options(ollama_url, lock_enabled)
        .with_mock_config_file(args.mock_config_file.clone());

    // Store the configured LLM client in state so spawned servers can use it
    state.set_llm_client(llm.clone()).await;

    // Create status channel
    let (status_tx, mut status_rx) = mpsc::unbounded_channel::<String>();

    // Spawn background task to print status messages in real-time
    let status_printer = tokio::spawn(async move {
        while let Some(msg) = status_rx.recv().await {
            if !msg.starts_with("__") {
                // Print status messages immediately for real-time output
                println!("{}", msg);
            }
        }
    });

    println!("Loading {} action(s)...\n", actions.len());

    // Execute each action
    for (i, action) in actions.iter().enumerate() {
        // Try to parse as common action
        if let Ok(common_action) = crate::llm::actions::common::CommonAction::from_json(action) {
            use crate::cli::{client_startup, server_startup};
            use crate::llm::actions::common::CommonAction;

            match common_action {
                CommonAction::OpenServer {
                    mac_address,
                    interface,
                    host,
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
                    // Execute open_server action
                    match server_startup::start_server_from_action(
                        &state,
                        mac_address,
                        interface.clone(),
                        host,
                        port,
                        &base_stack,
                        send_first,
                        initial_memory,
                        instruction.clone(),
                        startup_params,
                        event_handlers,
                        scheduled_tasks,
                        feedback_instructions,
                        status_tx.clone(),
                    )
                    .await
                    {
                        Ok(server_id) => {
                            let binding_desc = if let Some(iface) = &interface {
                                format!("interface {} ({})", iface, base_stack)
                            } else if let Some(p) = port {
                                format!("port {} ({})", p, base_stack)
                            } else {
                                format!("({})", base_stack)
                            };
                            println!(
                                "[{}] Opened server #{} on {}",
                                i + 1,
                                server_id.as_u32(),
                                binding_desc
                            );
                        }
                        Err(e) => {
                            eprintln!("[{}] Failed to open server: {}", i + 1, e);
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
                    // Execute open_client action
                    match client_startup::start_client_from_action(
                        &state,
                        &protocol,
                        &remote_addr,
                        instruction.clone(),
                        startup_params,
                        initial_memory,
                        event_handlers,
                        scheduled_tasks,
                        feedback_instructions,
                        llm.clone(),
                    )
                    .await
                    {
                        Ok(client_id) => {
                            println!(
                                "[{}] Opened client #{} to {} ({})",
                                i + 1,
                                client_id.as_u32(),
                                remote_addr,
                                protocol
                            );
                        }
                        Err(e) => {
                            eprintln!("[{}] Failed to open client: {}", i + 1, e);
                        }
                    }
                }
                CommonAction::ShowMessage { message } => {
                    println!("[{}] {}", i + 1, message);
                }
                _ => {
                    println!("[{}] Skipping unsupported action type", i + 1);
                }
            }
        } else {
            eprintln!("[{}] Skipping invalid action", i + 1);
        }
    }

    // Drop status_tx to close the channel and signal the background task to finish
    drop(status_tx);

    // Wait for the background task to print all remaining messages
    let _ = status_printer.await;

    println!("\nConfiguration loaded successfully.");

    // Check if we're in server mode
    if state.get_mode().await == Mode::Server {
        // Create a new status channel for run_server
        let (_status_tx, status_rx) = mpsc::unbounded_channel::<String>();
        // Run the server
        return run_server(&state, llm, status_rx).await;
    }

    Ok(())
}
