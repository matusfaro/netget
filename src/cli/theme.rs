use crossterm::style::Color;
use std::time::Duration;
use tracing::debug;

/// Theme variants based on terminal background
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
    Neutral,
}

/// Color palette for TUI with semantic color names
#[derive(Debug, Clone)]
pub struct ColorPalette {
    // Log level colors
    pub error: Color,
    pub warning: Color,
    pub info: Color,
    pub debug: Color,
    pub trace: Color,
    pub user: Color,

    // Connection/server indicators
    pub server: Color,
    pub connection: Color,

    // UI elements
    pub separator: Color,
    pub dimmed: Color,
    pub normal: Color,

    // Status indicators
    pub success: Color,
    pub failure: Color,
    pub ask: Color,
}

impl ColorPalette {
    /// Create a color palette for dark terminals (current NetGet default)
    pub fn dark() -> Self {
        Self {
            error: Color::Red,
            warning: Color::Yellow,
            info: Color::Blue,
            debug: Color::Cyan,
            trace: Color::DarkGrey,
            user: Color::Green,
            server: Color::Cyan,
            connection: Color::Cyan,
            separator: Color::DarkGreen,
            dimmed: Color::DarkGrey,
            normal: Color::White,
            success: Color::Green,
            failure: Color::Red,
            ask: Color::Yellow,
        }
    }

    /// Create a color palette for light terminals
    pub fn light() -> Self {
        Self {
            error: Color::DarkRed,
            warning: Color::DarkYellow,
            info: Color::DarkBlue,
            debug: Color::DarkCyan,
            trace: Color::DarkGrey,
            user: Color::DarkGreen,
            server: Color::DarkCyan,
            connection: Color::DarkCyan,
            separator: Color::DarkGreen,
            dimmed: Color::Grey,
            normal: Color::Black,
            success: Color::DarkGreen,
            failure: Color::DarkRed,
            ask: Color::DarkYellow,
        }
    }

    /// Create a neutral color palette that works on both light and dark backgrounds
    /// Uses medium contrast colors that are readable in most situations
    pub fn neutral() -> Self {
        Self {
            error: Color::Red,
            warning: Color::DarkYellow,
            info: Color::Blue,
            debug: Color::DarkCyan,
            trace: Color::Grey,
            user: Color::DarkGreen,
            server: Color::DarkCyan,
            connection: Color::DarkCyan,
            separator: Color::DarkGreen,
            dimmed: Color::Grey,
            normal: Color::Reset, // Use terminal default
            success: Color::DarkGreen,
            failure: Color::Red,
            ask: Color::DarkYellow,
        }
    }

    /// Get the appropriate color palette based on theme
    pub fn from_theme(theme: Theme) -> Self {
        match theme {
            Theme::Dark => Self::dark(),
            Theme::Light => Self::light(),
            Theme::Neutral => Self::neutral(),
        }
    }
}

/// Detect terminal background and determine appropriate theme
/// Returns None if detection fails or times out
///
/// Note: termbg can leave the terminal in a bad state on some terminals
/// (especially macOS Terminal.app). We wrap detection in catch_unwind and
/// flush any stale input afterwards.
pub fn detect_theme() -> Option<Theme> {
    use std::panic::catch_unwind;

    // Check for known-problematic terminal environments
    // macOS Terminal.app and some other terminals don't handle OSC queries well
    if let Ok(term_program) = std::env::var("TERM_PROGRAM") {
        // Apple Terminal is known to have issues with termbg
        if term_program == "Apple_Terminal" {
            debug!("Skipping theme detection on Apple Terminal (known issues)");
            return None;
        }
    }

    // Use a short timeout to avoid blocking startup
    let timeout = Duration::from_millis(100);

    // Wrap in catch_unwind to handle any panics from termbg
    let result = catch_unwind(|| termbg::theme(timeout));

    // Flush any leftover input that termbg might have left in the buffer
    // This is critical - termbg sends OSC sequences and if the terminal
    // doesn't respond or responds incorrectly, garbage can be left in stdin
    flush_stdin_nonblocking();

    match result {
        Ok(Ok(termbg::Theme::Light)) => Some(Theme::Light),
        Ok(Ok(termbg::Theme::Dark)) => Some(Theme::Dark),
        Ok(Err(e)) => {
            debug!("Theme detection failed: {:?}", e);
            None
        }
        Err(_) => {
            debug!("Theme detection panicked");
            None
        }
    }
}

/// Flush any pending input from stdin without blocking
/// This cleans up any stale escape sequences that termbg may have left
fn flush_stdin_nonblocking() {
    use std::io::Read;

    // On Unix, we can set stdin to non-blocking temporarily
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;

        let stdin = std::io::stdin();
        let fd = stdin.as_raw_fd();

        // Get current flags
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 {
            return;
        }

        // Set non-blocking
        if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return;
        }

        // Read and discard any pending data
        let mut buf = [0u8; 256];
        let mut stdin_lock = stdin.lock();
        while stdin_lock.read(&mut buf).unwrap_or(0) > 0 {}

        // Restore original flags
        unsafe { libc::fcntl(fd, libc::F_SETFL, flags) };
    }

    // On non-Unix platforms, we just do nothing - they typically don't have
    // the same issues with termbg
    #[cfg(not(unix))]
    {}
}

/// Parse theme from string (for CLI flag)
pub fn parse_theme(s: &str) -> anyhow::Result<Option<Theme>> {
    match s.to_lowercase().as_str() {
        "auto" => Ok(None), // None means auto-detect
        "light" => Ok(Some(Theme::Light)),
        "dark" => Ok(Some(Theme::Dark)),
        "neutral" => Ok(Some(Theme::Neutral)),
        _ => anyhow::bail!(
            "Invalid theme '{}'. Valid options: auto, light, dark, neutral",
            s
        ),
    }
}
