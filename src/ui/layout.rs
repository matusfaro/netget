//! Layout management for the TUI
//!
//! Defines the 4-panel layout structure:
//! - User input (bottom)
//! - LLM responses (top left)
//! - Connection info (top right)
//! - Status/activity log (middle)

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Layout structure for the application
pub struct AppLayout {
    /// User input area
    pub input: Rect,
    /// LLM output area
    pub llm_output: Rect,
    /// Connection information area
    pub connection_info: Rect,
    /// Status/activity area
    pub status: Rect,
}

impl AppLayout {
    /// Create a new layout from the given area
    pub fn new(area: Rect) -> Self {
        // Split vertically: top (70%) and bottom (30%)
        let vertical_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(area);

        // Split the top area vertically again: main content (70%) and status (30%)
        let top_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(vertical_chunks[0]);

        // Split the main content area horizontally: LLM output (60%) and connection info (40%)
        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(top_chunks[0]);

        Self {
            input: vertical_chunks[1],
            llm_output: main_chunks[0],
            connection_info: main_chunks[1],
            status: top_chunks[1],
        }
    }
}
