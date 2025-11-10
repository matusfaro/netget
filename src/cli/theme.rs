use crossterm::style::Color;
use std::time::Duration;

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
            normal: Color::Reset,  // Use terminal default
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
pub fn detect_theme() -> Option<Theme> {
    // Use a short timeout to avoid blocking startup
    let timeout = Duration::from_millis(100);

    match termbg::theme(timeout) {
        Ok(termbg::Theme::Light) => Some(Theme::Light),
        Ok(termbg::Theme::Dark) => Some(Theme::Dark),
        Err(_) => None,  // Detection failed or timed out
    }
}

/// Parse theme from string (for CLI flag)
pub fn parse_theme(s: &str) -> anyhow::Result<Option<Theme>> {
    match s.to_lowercase().as_str() {
        "auto" => Ok(None),  // None means auto-detect
        "light" => Ok(Some(Theme::Light)),
        "dark" => Ok(Some(Theme::Dark)),
        "neutral" => Ok(Some(Theme::Neutral)),
        _ => anyhow::bail!("Invalid theme '{}'. Valid options: auto, light, dark, neutral", s),
    }
}

