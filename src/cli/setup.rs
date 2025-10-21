//! Setup utilities for logging and terminal initialization

use anyhow::Result;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use std::fs::OpenOptions;
use std::io;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use super::Args;

/// Initialize logging based on arguments
pub fn init_logging(args: &Args) -> Result<()> {
    if args.logging_disabled() {
        // No-op subscriber when logging is disabled
        tracing_subscriber::registry()
            .with(EnvFilter::new("off"))
            .init();
    } else {
        let log_level = args.effective_log_level();

        // Determine output destination
        let is_interactive = args.is_interactive();

        if is_interactive || !args.log_level.eq_ignore_ascii_case("off") {
            // Log to file when in interactive mode or when logging is explicitly enabled
            let log_file = OpenOptions::new()
                .create(true)
                .append(true)
                .open("netget.log")?;

            let filter = EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(format!("netget={}", log_level)));

            tracing_subscriber::registry()
                .with(fmt::layer()
                    .with_writer(log_file)
                    .with_ansi(false)
                    .with_target(true)
                    .with_thread_ids(true)
                    .with_line_number(true))
                .with(filter)
                .init();
        } else {
            // Log to stderr in non-interactive mode (unless logging is off)
            let filter = EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(format!("netget={}", log_level)));

            tracing_subscriber::registry()
                .with(fmt::layer()
                    .with_writer(io::stderr)
                    .with_ansi(true)
                    .with_target(false)
                    .with_thread_ids(false)
                    .with_line_number(false))
                .with(filter)
                .init();
        }
    }

    Ok(())
}

/// Initialize terminal for TUI mode
/// Returns a guard that will clean up the terminal on drop
pub fn init_terminal() -> Result<TerminalGuard> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    Ok(TerminalGuard)
}

/// Guard to ensure terminal cleanup happens
pub struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}
