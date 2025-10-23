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

use super::server_startup;

/// Run NetGet in non-interactive mode with the given prompt
pub async fn run_non_interactive(
    prompt: String,
    args: &super::Args,
    settings: Settings,
) -> Result<()> {
    info!("Starting NetGet in non-interactive mode");
    debug!("Prompt: {}", prompt);

    // Create application state
    let state = AppState::new();

    // Override model if specified in args
    if let Some(model) = &args.model {
        state.set_ollama_model(model.clone()).await;
        debug!("Using model: {}", model);
    } else if !settings.model.is_empty() {
        state.set_ollama_model(settings.model.clone()).await;
    }

    // Create event handler and LLM client
    let llm = OllamaClient::default();
    let mut event_handler = EventHandler::new(state.clone(), llm.clone());

    // Create status channel for messages
    let (status_tx, mut status_rx) = mpsc::unbounded_channel::<String>();

    // Spawn a task to handle the interpretation
    let status_tx_clone = status_tx.clone();
    let mut interpret_handle = tokio::spawn(async move {
        event_handler.handle_interpret(prompt, status_tx_clone).await
    });

    // Process status messages from the handler
    let mut handle_completed = false;
    loop {
        tokio::select! {
            msg = status_rx.recv() => {
                match msg {
                    Some(msg) => {
                        if msg == "__CHECK_SERVER_STARTUP__" {
                            // Server should be started - break to main server loop
                            break;
                        } else if msg == "__UPDATE_UI__" {
                            // Ignore in non-interactive mode
                        } else if msg.starts_with("__STATS_SENT__") {
                            // Ignore in non-interactive mode
                        } else {
                            // Print status message (strip log level prefix for cleaner output)
                            let clean_msg = msg
                                .strip_prefix("[INFO] ").unwrap_or(&msg)
                                .strip_prefix("[ERROR] ").unwrap_or(&msg)
                                .strip_prefix("[WARN] ").unwrap_or(&msg)
                                .strip_prefix("[DEBUG] ").unwrap_or(&msg);
                            println!("{}", clean_msg);
                        }
                    }
                    None => {
                        // Channel closed - handler finished
                        break;
                    }
                }
            }
            result = &mut interpret_handle => {
                // Handler completed - save result and mark as completed
                result??;
                handle_completed = true;
                break;
            }
        }
    }

    // Wait for handler to finish (only if it hasn't completed yet)
    if !handle_completed {
        interpret_handle.await??;
    }

    // Check if we're in server mode
    if state.get_mode().await == Mode::Server {
        // Run the server
        return run_server(&state, llm, status_rx).await;
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

    // Start the server
    server_startup::check_and_start_server(state, &llm, &status_tx).await?;

    println!("Server is running. Press Ctrl+C to stop.");
    println!("Waiting for connections...\n");

    // Set up Ctrl+C handler
    let shutdown = Arc::new(Mutex::new(false));
    let shutdown_clone = shutdown.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        let mut shutdown = shutdown_clone.lock().await;
        *shutdown = true;
    });

    // Main event loop
    loop {
        // Check for shutdown
        if *shutdown.lock().await {
            println!("\nShutting down server...");
            break;
        }

        // Process status messages from handler (drain remaining)
        while let Ok(msg) = status_rx.try_recv() {
            if !msg.starts_with("__") {
                println!("[STATUS] {}", msg);
            }
        }

        // Process server status messages
        while let Ok(msg) = server_status_rx.try_recv() {
            println!("[STATUS] {}", msg);
        }

        // Sleep briefly to avoid busy waiting
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    println!("Server stopped.");
    Ok(())
}
