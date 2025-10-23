//! Setup utilities for logging and terminal initialization

use anyhow::Result;
use crossterm::event::{
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use std::io;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use super::Args;

/// Initialize logging based on arguments
pub fn init_logging(args: &Args, is_interactive: bool) -> Result<()> {
    if args.logging_disabled() || is_interactive {
        // No-op subscriber when logging is disabled or in interactive (TUI) mode
        tracing_subscriber::registry()
            .with(EnvFilter::new("off"))
            .init();
    } else {
        let log_level = args.effective_log_level();

        // Create environment filter
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(format!("netget={}", log_level)));

        // Log to stderr in non-interactive mode
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

    Ok(())
}

/// Initialize terminal for TUI mode
/// Returns a guard that will clean up the terminal on drop
pub fn init_terminal() -> Result<TerminalGuard> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;

    // Enable keyboard enhancement flags for better modifier detection
    // This allows terminals to properly report Shift+Enter, Alt+Enter, etc.
    let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
        | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
        | KeyboardEnhancementFlags::REPORT_EVENT_TYPES;

    // Try to enable enhanced keyboard support (not all terminals support this)
    let enhanced_supported = execute!(io::stdout(), PushKeyboardEnhancementFlags(flags)).is_ok();

    Ok(TerminalGuard { enhanced_supported })
}

/// Guard to ensure terminal cleanup happens
pub struct TerminalGuard {
    enhanced_supported: bool,
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.enhanced_supported {
            let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        }
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}
