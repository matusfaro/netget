//! CLI module - handles command-line interface and application startup

mod args;
mod non_interactive;
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
        let state = AppState::new();
        let app = App::new();
        let llm = OllamaClient::default();
        let event_handler = EventHandler::new(state.clone(), llm);

        let _terminal_guard = setup::init_terminal()?;
        tui::run_tui(state, app, event_handler, settings).await
    } else {
        // No prompt and no terminal available
        anyhow::bail!(
            "Cannot start in interactive mode without a terminal.\n\
             Please provide a prompt via arguments or stdin."
        )
    }
}
