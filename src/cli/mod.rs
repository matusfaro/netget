//! CLI module - handles command-line interface and application startup

mod args;
mod input_state;
mod non_interactive;
mod rolling_tui;
pub mod server_startup;
mod setup;
mod sticky_footer;
mod terminal_cleanup;

use anyhow::Result;
pub use args::Args;
use clap::Parser;

use crate::events::EventHandler;
use crate::llm::OllamaClient;
use crate::settings::Settings;
use crate::state::app_state::AppState;
use crate::ui::App;

/// Main CLI entry point
pub async fn run() -> Result<()> {
    let args = Args::parse();

    // Try to get prompt first (this reads stdin if needed)
    let prompt = args.get_prompt()?;

    // Determine if we're in interactive mode
    let is_interactive = prompt.is_none() && args.is_interactive();

    // Setup logging based on mode
    setup::init_logging(&args, is_interactive)?;

    // Load settings
    let settings = Settings::load();

    // Decide on mode based on whether we have a prompt
    if let Some(prompt) = prompt {
        // Non-interactive mode - we have a prompt
        non_interactive::run_non_interactive(prompt, &args, settings).await
    } else if args.is_interactive() {
        // Interactive TUI mode - no prompt and terminal is available
        let state = AppState::new_with_options(args.include_disabled_protocols);

        // Determine scripting mode with priority: CLI arg > saved setting > auto-detected
        let mode_to_set = if let Some(mode) = args.parse_scripting_mode()? {
            Some(mode)
        } else if let Some(mode) = settings.parse_scripting_mode() {
            Some(mode)
        } else {
            None
        };

        if let Some(mode) = mode_to_set {
            // Validate that the requested environment is available
            let scripting_env = state.get_scripting_env().await;
            let available = match mode {
                crate::state::app_state::ScriptingMode::Llm => true, // Always available
                crate::state::app_state::ScriptingMode::Python => scripting_env.python.is_some(),
                crate::state::app_state::ScriptingMode::JavaScript => scripting_env.javascript.is_some(),
                crate::state::app_state::ScriptingMode::Go => scripting_env.go.is_some(),
            };

            if !available {
                anyhow::bail!(
                    "{} environment is not available on this system. Please install it or choose a different environment.",
                    mode
                );
            }

            state.set_selected_scripting_mode(mode).await;
        }

        let app = App::new();
        let llm = OllamaClient::default();
        let event_handler = EventHandler::new(state.clone(), llm.clone());

        // Note: init_terminal not needed for rolling TUI (manages terminal itself)
        rolling_tui::run_rolling_tui(state, app, event_handler, llm, settings, &args).await
    } else {
        // No prompt and no terminal available
        anyhow::bail!(
            "Cannot start in interactive mode without a terminal.\n\
             Please provide a prompt via arguments or stdin."
        )
    }
}
