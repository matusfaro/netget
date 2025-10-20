//! Layout management for the TUI
//!
//! Defines the shell-like layout with scrollable output and pinned input

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Shell-like application layout
pub struct AppLayout {
    /// Scrollable output area (main content)
    pub output: Rect,
    /// Status bar (connection info)
    pub status: Rect,
    /// Input prompt area (pinned to bottom)
    pub input: Rect,
}

impl AppLayout {
    /// Create a new layout for the given terminal area
    pub fn new(area: Rect) -> Self {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),      // Output area (takes most space)
                Constraint::Length(1),   // Status bar (1 line)
                Constraint::Length(3),   // Input area (3 lines for border + text)
            ])
            .split(area);

        Self {
            output: chunks[0],
            status: chunks[1],
            input: chunks[2],
        }
    }
}
