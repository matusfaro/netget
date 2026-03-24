//! CLI module - handles command-line interface and application startup

mod args;
mod banner;
pub mod client_startup;
pub mod easy_startup;
mod input_state;
mod non_interactive;
mod rolling_tui;
pub mod server_startup;
mod setup;
mod sticky_footer;
mod terminal_cleanup;
mod theme;

use anyhow::Result;
pub use args::Args;
use clap::Parser;
use tracing::debug;

use crate::events::EventHandler;
use crate::llm::OllamaClient;
use crate::settings::Settings;
use crate::state::app_state::AppState;
use crate::ui::App;

/// Create the LLM client from CLI args, branching on --openai-url vs --ollama-url
fn create_llm_client(args: &Args, lock_enabled: bool) -> Result<OllamaClient> {
    if let Some(ref openai_url) = args.openai_url {
        let api_key = args.resolve_api_key().ok_or_else(|| {
            anyhow::anyhow!(
                "API key required for OpenAI-compatible endpoint.\n   Use --api-key, NETGET_API_KEY, or OPENAI_API_KEY env var."
            )
        })?;
        if args.model.is_none() {
            anyhow::bail!(
                "--model is required when using --openai-url.\n   Example: --openai-url {} --model gpt-4o",
                openai_url
            );
        }
        Ok(OllamaClient::new_openai(openai_url, api_key))
    } else {
        let ollama_url = args.ollama_url.as_deref().unwrap_or("http://localhost:11434");
        Ok(OllamaClient::new_with_options(ollama_url, lock_enabled))
    }
}

/// Main CLI entry point
pub async fn run() -> Result<()> {
    let args = Args::parse();

    // Handle --simple-list flag (list available simple protocols and exit)
    if args.simple_list {
        use crate::protocol::EASY_REGISTRY;
        println!("Available simple protocols:");
        println!();
        let protocols = EASY_REGISTRY.get_all_names();
        if protocols.is_empty() {
            println!("  No simple protocols available (check compiled features)");
        } else {
            for name in protocols {
                println!("  - {}", name);
            }
        }
        println!();
        println!("Usage: netget --simple <protocol>");
        println!("Example: netget --simple http");
        return Ok(());
    }

    // Handle --simple <protocol> flag (start simple protocol in non-interactive mode)
    if let Some(ref protocol) = args.simple_protocol {
        return run_simple_protocol(protocol, &args).await;
    }

    // Check for actions JSON first (--load flag or JSON input)
    let actions_json = args.get_actions_json()?;

    // Try to get prompt (this reads stdin if needed)
    let prompt = args.get_prompt()?;

    // Determine if we're in interactive mode
    let is_interactive = prompt.is_none() && actions_json.is_none() && args.is_interactive();

    // Setup logging based on mode
    setup::init_logging(&args, is_interactive)?;

    // Load settings
    let settings = Settings::load();

    // Decide on mode based on input type
    if let Some(actions) = actions_json {
        // Non-interactive mode - we have actions JSON to execute
        non_interactive::run_with_actions(actions, &args, settings).await
    } else if let Some(prompt) = prompt {
        // Non-interactive mode - we have a prompt
        non_interactive::run_non_interactive(prompt, &args, settings).await
    } else if args.is_interactive() {
        // Interactive TUI mode - no prompt and terminal is available
        debug!("Entering interactive TUI mode");
        debug!("Creating AppState...");
        let base_url = args.openai_url.clone()
            .or_else(|| args.ollama_url.clone())
            .unwrap_or_else(|| "http://localhost:11434".to_string());
        let state = AppState::new_with_options(args.include_disabled_protocols, args.ollama_lock, base_url);
        debug!("AppState created");

        // Configure rate limiter from CLI args
        debug!("Configuring rate limiter...");
        let rate_limiter_config = args.build_rate_limiter_config();
        state.configure_rate_limiter(rate_limiter_config).await?;
        debug!("Rate limiter configured");

        // Determine scripting mode with priority: CLI arg > saved setting > auto-detected
        debug!("Parsing scripting mode...");
        let mode_to_set = if let Some(mode) = args.parse_scripting_mode()? {
            Some(mode)
        } else {
            settings.parse_scripting_mode()
        };
        debug!("Scripting mode to set: {:?}", mode_to_set);

        if let Some(mode) = mode_to_set {
            // Validate that the requested environment is available
            debug!("Getting scripting environment for validation...");
            let scripting_env = state.get_scripting_env().await;
            debug!("Scripting environment retrieved");
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
        }

        // Determine theme: CLI arg > auto-detect > neutral fallback
        debug!("Parsing theme argument: {}", args.theme);
        let theme_option = theme::parse_theme(&args.theme)?;
        debug!("Theme option parsed: {:?}", theme_option);
        let theme = if let Some(t) = theme_option {
            debug!("Using explicit theme: {:?}", t);
            t
        } else {
            // Auto-detect
            debug!("Auto-detecting theme...");
            let detected = theme::detect_theme().unwrap_or(theme::Theme::Neutral);
            debug!("Theme detected: {:?}", detected);
            detected
        };
        debug!("Creating color palette from theme: {:?}", theme);
        let color_palette = theme::ColorPalette::from_theme(theme);
        debug!("Color palette created");

        // Get system capabilities for UI display
        debug!("Getting system capabilities...");
        let system_capabilities = state.get_system_capabilities().await;
        debug!("Creating App...");
        let app = App::new(system_capabilities);
        debug!("Getting ollama lock status...");
        let lock_enabled = state.get_ollama_lock_enabled().await;

        // Initialize LLM backend (OpenAI, Hybrid, or Ollama-only)
        #[cfg(feature = "embedded-llm")]
        let llm = {
            if args.openai_url.is_some() {
                debug!("Creating OpenAI-compatible client...");
                create_llm_client(&args, lock_enabled)?
                    .with_mock_config_file(args.mock_config_file.clone())
                    .with_app_state(state.clone())
            } else {
                // Check if user wants embedded LLM
                let use_hybrid = args.use_embedded || args.embedded_model.is_some();

                if use_hybrid {
                    debug!("Creating HybridLLMManager...");
                    let embedded_path = args.embedded_model.as_ref().map(|p| p.display().to_string());
                    let hybrid = crate::llm::HybridLLMManager::new(args.use_embedded, embedded_path).await?;

                    if let Some(client) = hybrid.ollama_client().await {
                        debug!("Using Ollama backend from HybridLLMManager");
                        client
                            .with_mock_config_file(args.mock_config_file.clone())
                            .with_app_state(state.clone())
                    } else {
                        debug!("Using embedded backend - creating fallback OllamaClient");
                        let ollama_url = args.ollama_url.as_deref().unwrap_or("http://localhost:11434");
                        OllamaClient::new_with_options(ollama_url, lock_enabled)
                            .with_mock_config_file(args.mock_config_file.clone())
                            .with_app_state(state.clone())
                    }
                } else {
                    debug!("Creating OllamaClient...");
                    create_llm_client(&args, lock_enabled)?
                        .with_mock_config_file(args.mock_config_file.clone())
                        .with_app_state(state.clone())
                }
            }
        };

        #[cfg(not(feature = "embedded-llm"))]
        let llm = {
            debug!("Creating LLM client...");
            create_llm_client(&args, lock_enabled)?
                .with_mock_config_file(args.mock_config_file.clone())
                .with_app_state(state.clone())
        };

        // Store the configured LLM client in state so spawned servers can use it
        state.set_llm_client(llm.clone()).await;

        debug!("Creating EventHandler...");
        let event_handler = EventHandler::new(state.clone(), llm.clone());

        // Note: init_terminal not needed for rolling TUI (manages terminal itself)
        debug!("Entering rolling TUI...");
        rolling_tui::run_rolling_tui(
            state,
            app,
            event_handler,
            llm,
            settings,
            &args,
            color_palette,
        )
        .await
    } else {
        // No prompt and no terminal available
        anyhow::bail!(
            "Cannot start in interactive mode without a terminal.\n\
             Please provide a prompt via arguments or stdin."
        )
    }
}

/// Run a simple protocol in non-interactive mode
async fn run_simple_protocol(protocol: &str, args: &Args) -> Result<()> {
    use crate::protocol::EASY_REGISTRY;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex;

    // Setup logging (non-interactive mode)
    setup::init_logging(args, false)?;

    // Check if protocol exists
    if EASY_REGISTRY.get_by_name(protocol).is_none() {
        eprintln!("Error: Unknown simple protocol: {}", protocol);
        eprintln!("Use --simple-list to see available protocols");
        std::process::exit(1);
    }

    println!("[SIMPLE] Starting simple protocol: {}", protocol);

    // Load settings
    let settings = Settings::load();

    // Create application state
    let base_url = args.openai_url.clone()
        .or_else(|| args.ollama_url.clone())
        .unwrap_or_else(|| "http://localhost:11434".to_string());
    let state = AppState::new_with_options(args.include_disabled_protocols, args.ollama_lock, base_url);

    // Configure rate limiter from CLI args
    let rate_limiter_config = args.build_rate_limiter_config();
    state.configure_rate_limiter(rate_limiter_config).await?;

    // Determine configured model: args override settings
    let configured_model = args.model.clone().or(settings.model.clone());

    // Select or validate model
    let selected_model = if args.openai_url.is_some() {
        configured_model.ok_or_else(|| anyhow::anyhow!(
            "--model is required when using --openai-url"
        ))?
    } else {
        let ollama_url_for_model = args.ollama_url.as_deref().unwrap_or("http://localhost:11434");
        crate::llm::select_or_validate_model(configured_model, false, ollama_url_for_model)
            .await?
            .ok_or_else(|| anyhow::anyhow!("No model available"))?
    };

    println!("[SIMPLE] Using model: {}", selected_model);
    state.set_ollama_model(Some(selected_model)).await;

    // Create LLM client
    let lock_enabled = state.get_ollama_lock_enabled().await;
    let llm = create_llm_client(args, lock_enabled)?
        .with_mock_config_file(args.mock_config_file.clone())
        .with_app_state(state.clone());

    // Store the configured LLM client in state so spawned servers can use it
    state.set_llm_client(llm.clone()).await;

    // Start the easy protocol
    let easy_id = easy_startup::start_easy_protocol(
        protocol,
        None, // user_instruction - could be extended via CLI later
        None, // port - could be extended via CLI later
        Arc::new(state.clone()),
        Arc::new(llm.clone()),
    )
    .await?;

    println!(
        "[SIMPLE] Started {} (easy instance #{})",
        protocol,
        easy_id.as_u32()
    );

    // Get underlying server info
    if let Some(server_id) = state.get_first_server_id().await {
        if let Some(server) = state.get_server(server_id).await {
            println!("[SIMPLE] Listening on port {}", server.port);
        }
        println!("[SIMPLE] Server #{} is running. Press Ctrl+C to stop.", server_id.as_u32());
    }

    // Set up Ctrl+C handler
    let shutdown = Arc::new(Mutex::new(false));
    let shutdown_clone = shutdown.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        let mut shutdown = shutdown_clone.lock().await;
        *shutdown = true;
    });

    // Main event loop - just wait for shutdown
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if *shutdown.lock().await {
            println!("\n[SIMPLE] Shutting down...");
            break;
        }
    }

    println!("[SIMPLE] Server stopped.");
    Ok(())
}
