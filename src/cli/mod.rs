//! CLI module - handles command-line interface and application startup

mod args;
mod server_startup;
mod setup;
mod terminal_cleanup;
mod tui;

pub use args::Args;
use anyhow::Result;
use clap::Parser;

use crate::events::EventHandler;
use crate::llm::OllamaClient;
use crate::settings::Settings;
use crate::state::app_state::AppState;
use crate::ui::App;

/// Main CLI entry point
pub async fn run() -> Result<()> {
    let args = Args::parse();

    // Setup logging
    setup::init_logging(&args)?;

    // Load settings
    let settings = Settings::load();

    // Create application state
    let state = AppState::new();
    let app = App::new();

    // Create LLM client
    let llm = OllamaClient::default();
    let event_handler = EventHandler::new(state.clone(), llm);

    // Process initial command if provided
    if let Some(cmd) = args.command {
        eprintln!("Executing command: {}", cmd);

        // Parse and execute the command
        use crate::events::types::UserCommand;

        let command = UserCommand::parse(&cmd);

        // Check if it's a slash command or needs LLM interpretation
        match command {
            UserCommand::Interpret { input } => {
                // Use LLM to interpret and execute
                use crate::llm::{CommandInterpretation, OllamaClient, PromptBuilder};

                let llm = OllamaClient::default();
                let model = state.get_ollama_model().await;
                let prompt = PromptBuilder::build_command_interpretation_prompt(&state, &input).await;

                match llm.generate(&model, &prompt).await {
                    Ok(response) => {
                        match CommandInterpretation::from_str(&response) {
                            Ok(interpretation) => {
                                // Display message
                                if let Some(msg) = &interpretation.message {
                                    eprintln!("LLM: {}", msg);
                                }

                                // Execute actions
                                use crate::llm::Action;
                                use crate::state::app_state::Mode;

                                for action in interpretation.actions {
                                    match action {
                                        Action::UpdateInstruction { instruction } => {
                                            eprintln!("Instruction: {}", instruction);
                                            state.set_instruction(instruction).await;
                                        }
                                        Action::OpenServer { port, base_stack: stack_str, protocol: _, send_banner, initial_memory } => {
                                            let stack = crate::protocol::BaseStack::from_str(&stack_str)
                                                .unwrap_or(crate::protocol::BaseStack::TcpRaw);
                                            state.set_mode(Mode::Server).await;
                                            state.set_base_stack(stack).await;
                                            state.set_port(port).await;
                                            state.set_send_banner(send_banner).await;

                                            // Set initial memory if provided
                                            if let Some(mem) = initial_memory {
                                                state.set_memory(mem).await;
                                            }

                                            eprintln!("Server will start on port {} with stack {}", port, stack);
                                        }
                                        Action::ShowMessage { message } => {
                                            eprintln!("{}", message);
                                        }
                                        _ => {
                                            eprintln!("Action not supported in CLI mode: {:?}", action);
                                        }
                                    }
                                }

                                // Check if we need to start a server and run it
                                if state.get_mode().await == crate::state::app_state::Mode::Server {
                                    eprintln!("Starting server (press Ctrl+C to stop)...");

                                    // Create event channels
                                    let (network_tx, mut network_rx) = tokio::sync::mpsc::unbounded_channel();
                                    let (status_tx, mut _status_rx) = tokio::sync::mpsc::unbounded_channel();

                                    // Start server
                                    use std::collections::HashMap;
                                    use std::sync::Arc;
                                    use tokio::sync::Mutex;
                                    let connections = Arc::new(Mutex::new(HashMap::new()));
                                    let cancellation_tokens = Arc::new(Mutex::new(HashMap::new()));

                                    if let Err(e) = server_startup::check_and_start_server(
                                        &state,
                                        &network_tx,
                                        &connections,
                                        &cancellation_tokens,
                                        &status_tx,
                                    ).await {
                                        eprintln!("Failed to start server: {}", e);
                                        return Err(e);
                                    }

                                    // Simple event processing loop
                                    eprintln!("Server running. Waiting for connections...");
                                    while let Some(event) = network_rx.recv().await {
                                        eprintln!("Event: {:?}", event);
                                        // TODO: Handle events with LLM
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("Failed to parse LLM response: {}", e);
                                return Err(e.into());
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("LLM error: {}", e);
                        return Err(e);
                    }
                }
            }
            _ => {
                eprintln!("Slash commands not supported in CLI mode");
            }
        }

        return Ok(());
    }

    // No command provided - enter interactive TUI mode
    let _terminal_guard = setup::init_terminal()?;

    tui::run_tui(state, app, event_handler, settings).await
}
