//! Setup utilities for logging and terminal initialization

use anyhow::Result;
use crossterm::event::PopKeyboardEnhancementFlags;
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, LeaveAlternateScreen};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use tracing::Level;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use super::Args;

/// Custom writer that applies bright cyan color to TRACE level logs
struct ColoredLogWriter {
    inner: Arc<Mutex<File>>,
}

impl ColoredLogWriter {
    fn new(file: File) -> Self {
        Self {
            inner: Arc::new(Mutex::new(file)),
        }
    }
}

impl Write for ColoredLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Convert to string to check for TRACE level
        if let Ok(s) = std::str::from_utf8(buf) {
            // Replace any ANSI color code before " TRACE" with bright cyan
            // Look for the pattern: ESC[<numbers>m TRACE
            let mut modified = String::with_capacity(s.len());
            let mut chars = s.chars().peekable();

            while let Some(ch) = chars.next() {
                if ch == '\x1b' {
                    // Start of ANSI sequence
                    let mut seq = String::from("\x1b");

                    // Collect the ANSI sequence
                    while let Some(&next_ch) = chars.peek() {
                        seq.push(next_ch);
                        chars.next();
                        if next_ch == 'm' {
                            break;
                        }
                    }

                    // Check if this is followed by " TRACE"
                    let remaining: String = chars.clone().collect();
                    if remaining.starts_with(" TRACE") {
                        // Replace with bright cyan
                        modified.push_str("\x1b[96m");
                    } else {
                        // Keep original sequence
                        modified.push_str(&seq);
                    }
                } else {
                    modified.push(ch);
                }
            }

            self.inner.lock().unwrap().write_all(modified.as_bytes())?;
            Ok(buf.len())
        } else {
            self.inner.lock().unwrap().write(buf)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.lock().unwrap().flush()
    }
}

impl Clone for ColoredLogWriter {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// MakeWriter implementation for ColoredLogWriter
struct ColoredLogWriterMaker {
    writer: ColoredLogWriter,
}

impl ColoredLogWriterMaker {
    fn new(file: File) -> Self {
        Self {
            writer: ColoredLogWriter::new(file),
        }
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for ColoredLogWriterMaker {
    type Writer = ColoredLogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        self.writer.clone()
    }
}

/// Initialize logging based on arguments
pub fn init_logging(args: &Args, is_interactive: bool) -> Result<()> {
    if args.logging_disabled() {
        // No-op subscriber when logging is explicitly disabled
        tracing_subscriber::registry()
            .with(EnvFilter::new("off"))
            .init();
    } else if is_interactive {
        // Interactive (TUI) mode: log to netget.log file with bright cyan color
        // Development builds default to TRACE, release builds default to INFO
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open("netget.log")?;

        let colored_writer = ColoredLogWriterMaker::new(log_file);

        let default_level = if cfg!(debug_assertions) {
            Level::TRACE
        } else {
            Level::INFO
        };

        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(format!("netget={}", default_level)));

        tracing_subscriber::registry()
            .with(
                fmt::layer()
                    .with_writer(colored_writer)
                    .with_ansi(true)
                    .with_target(true)
                    .with_thread_ids(false)
                    .with_line_number(true),
            )
            .with(filter)
            .init();
    } else {
        // Non-interactive mode: log to stderr with configured level
        let log_level = args.effective_log_level();

        // Create environment filter
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(format!("netget={log_level}")));

        // Log to stderr in non-interactive mode
        tracing_subscriber::registry()
            .with(
                fmt::layer()
                    .with_writer(io::stderr)
                    .with_ansi(true)
                    .with_target(false)
                    .with_thread_ids(false)
                    .with_line_number(false),
            )
            .with(filter)
            .init();
    }

    Ok(())
}

/// Guard to reset terminal state on drop
pub struct TerminalGuard {
    enhanced_supported: bool,
}

impl TerminalGuard {
    #[allow(dead_code)]
    pub fn new(enhanced_supported: bool) -> Self {
        Self { enhanced_supported }
    }
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
