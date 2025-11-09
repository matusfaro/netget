//! CLI module - handles command-line interface and application startup

mod args;
pub mod client_startup;
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

use crate::events::EventHandler;
use crate::llm::OllamaClient;
use crate::settings::Settings;
use crate::state::app_state::AppState;
use crate::ui::App;

/// Main CLI entry point
pub async fn run() -> Result<()> {
    let args = Args::parse();

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
        let state = AppState::new_with_options(args.include_disabled_protocols, args.ollama_lock);

        // Determine scripting mode with priority: CLI arg > saved setting > auto-detected
        let mode_to_set = if let Some(mode) = args.parse_scripting_mode()? {
            Some(mode)
        } else { settings.parse_scripting_mode() };

        if let Some(mode) = mode_to_set {
            // Validate that the requested environment is available
            let scripting_env = state.get_scripting_env().await;
            let available = match mode {
                crate::state::app_state::ScriptingMode::On => true, // LLM chooses runtime
                crate::state::app_state::ScriptingMode::Off => true, // Always available
                crate::state::app_state::ScriptingMode::Python => scripting_env.python.is_some(),
                crate::state::app_state::ScriptingMode::JavaScript => scripting_env.javascript.is_some(),
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
        let theme_option = theme::parse_theme(&args.theme)?;
        let theme = if let Some(t) = theme_option {
            t
        } else {
            // Auto-detect
            theme::detect_theme().unwrap_or(theme::Theme::Neutral)
        };
        let color_palette = theme::ColorPalette::from_theme(theme);

        // Get system capabilities for UI display
        let system_capabilities = state.get_system_capabilities().await;
        let app = App::new(system_capabilities);
        let lock_enabled = state.get_ollama_lock_enabled().await;
        let llm = OllamaClient::new_with_options("http://localhost:11434", lock_enabled);
        let event_handler = EventHandler::new(state.clone(), llm.clone());

        // Note: init_terminal not needed for rolling TUI (manages terminal itself)
        rolling_tui::run_rolling_tui(state, app, event_handler, llm, settings, &args, color_palette).await
    } else {
        // No prompt and no terminal available
        anyhow::bail!(
            "Cannot start in interactive mode without a terminal.\n\
             Please provide a prompt via arguments or stdin."
        )
    }
}
