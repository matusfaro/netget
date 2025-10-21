//! Setup utilities for logging and terminal initialization

use anyhow::Result;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use std::fs::OpenOptions;
use std::io;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use super::Args;

/// Initialize logging based on debug flag
pub fn init_logging(args: &Args) -> Result<()> {
    if args.debug {
        // Log to file when debug is enabled
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open("netget.log")?;

        tracing_subscriber::registry()
            .with(fmt::layer()
                .with_writer(log_file)
                .with_ansi(false)
                .with_target(true)
                .with_thread_ids(true)
                .with_line_number(true))
            .with(EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("netget=debug")))
            .init();
    } else {
        // No-op subscriber when debug is disabled
        tracing_subscriber::registry()
            .with(EnvFilter::new("off"))
            .init();
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
