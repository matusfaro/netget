//! Application state for the shell interface

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use tracing::{debug, warn};

/// Which panel has focus
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// Input panel is focused (default)
    Input,
    /// Output panel is focused (for scrolling)
    Output,
}

impl Default for Focus {
    fn default() -> Self {
        Focus::Input
    }
}

/// Main application state for the TUI
pub struct App {
    /// User input buffer
    pub input: String,
    /// Cursor position in input
    pub cursor_position: usize,
    /// Command history
    pub command_history: Vec<String>,
    /// Current position in history (None = not browsing history)
    pub history_position: Option<usize>,
    /// Temporary buffer when browsing history
    pub history_temp_input: Option<String>,
    /// All output messages (combined log)
    pub output_messages: Vec<String>,
    /// Connection information
    pub connection_info: ConnectionInfo,
    /// Packet statistics
    pub packet_stats: PacketStats,
    /// Scroll offset for output (0 = bottom, higher = scrolled up)
    pub scroll_offset: usize,
    /// Scroll offset for input (lines scrolled from top)
    pub input_scroll: u16,
    /// Which panel currently has focus
    pub focus: Focus,
    /// Slash command suggestions (shown when typing "/")
    pub slash_suggestions: Vec<String>,
}

#[derive(Default, Clone)]
pub struct ConnectionInfo {
    pub mode: String,
    pub protocol: String,
    pub model: String,
    pub local_addr: Option<String>,
    pub remote_addr: Option<String>,
    pub state: String,
}

#[derive(Default, Clone)]
pub struct PacketStats {
    pub packets_received: u64,
    pub packets_sent: u64,
    pub bytes_received: u64,
    pub bytes_sent: u64,
}

impl Default for App {
    fn default() -> Self {
        Self {
            input: String::new(),
            cursor_position: 0,
            command_history: Vec::new(),
            history_position: None,
            history_temp_input: None,
            output_messages: Vec::new(),
            connection_info: ConnectionInfo::default(),
            packet_stats: PacketStats::default(),
            scroll_offset: 0,
            input_scroll: 0,
            focus: Focus::default(),
            slash_suggestions: Vec::new(),
        }
    }
}

impl App {
    /// Get the path to the history file
    fn history_file_path() -> Option<PathBuf> {
        dirs::home_dir().map(|mut path| {
            path.push(".netget_history");
            path
        })
    }

    /// Load command history from file
    fn load_history() -> Vec<String> {
        let Some(path) = Self::history_file_path() else {
            warn!("Could not determine home directory for history file");
            return Vec::new();
        };

        if !path.exists() {
            debug!("History file does not exist yet: {:?}", path);
            return Vec::new();
        }

        match File::open(&path) {
            Ok(file) => {
                let reader = BufReader::new(file);
                let history: Vec<String> = reader
                    .lines()
                    .filter_map(|line| line.ok())
                    .filter(|line| !line.trim().is_empty())
                    .collect();
                debug!("Loaded {} commands from history", history.len());
                history
            }
            Err(e) => {
                warn!("Failed to open history file: {}", e);
                Vec::new()
            }
        }
    }

    /// Save command history to file
    pub fn save_history(&self) -> std::io::Result<()> {
        let Some(path) = Self::history_file_path() else {
            return Ok(()); // Silently skip if can't determine home dir
        };

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;

        for command in &self.command_history {
            writeln!(file, "{}", command)?;
        }

        debug!("Saved {} commands to history", self.command_history.len());
        Ok(())
    }

    /// Create a new App instance (loads history from ~/.netget_history)
    pub fn new() -> Self {
        let mut app = Self::default();
        app.command_history = Self::load_history();
        app
    }

    /// Get the number of commands in history
    pub fn history_count(&self) -> usize {
        self.command_history.len()
    }

    /// Add a message to the output log and return true if it's new
    pub fn add_message(&mut self, message: String) -> bool {
        // Check if this is actually a new message
        let is_new = self.output_messages.last() != Some(&message);
        if is_new {
            self.output_messages.push(message);
        }
        is_new
    }

    /// Get the last N messages
    pub fn get_last_messages(&self, n: usize) -> &[String] {
        let start = self.output_messages.len().saturating_sub(n);
        &self.output_messages[start..]
    }

    /// Get count of output messages
    pub fn output_count(&self) -> usize {
        self.output_messages.len()
    }

    /// Legacy methods for compatibility
    pub fn add_llm_message(&mut self, message: String) {
        self.add_message(message);
    }

    pub fn add_status_message(&mut self, message: String) {
        self.add_message(message);
    }

    /// Scroll up in the output
    pub fn scroll_up(&mut self, lines: usize) {
        let max_scroll = self.output_messages.len().saturating_sub(1);
        self.scroll_offset = (self.scroll_offset + lines).min(max_scroll);
    }

    /// Scroll down in the output
    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    /// Scroll to bottom
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Scroll to top
    pub fn scroll_to_top(&mut self) {
        let max_scroll = self.output_messages.len().saturating_sub(1);
        self.scroll_offset = max_scroll;
    }

    /// Handle character input
    pub fn enter_char(&mut self, c: char) {
        // Exit history mode when typing
        if self.history_position.is_some() {
            self.history_position = None;
            self.history_temp_input = None;
        }

        self.input.insert(self.cursor_position, c);
        self.cursor_position += 1;
        self.update_slash_suggestions();
    }

    /// Handle backspace
    pub fn delete_char(&mut self) {
        // Exit history mode when editing
        if self.history_position.is_some() {
            self.history_position = None;
            self.history_temp_input = None;
        }

        if self.cursor_position > 0 {
            self.input.remove(self.cursor_position - 1);
            self.cursor_position -= 1;
            self.update_slash_suggestions();
        }
    }

    /// Move cursor left
    pub fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    /// Move cursor right
    pub fn move_cursor_right(&mut self) {
        if self.cursor_position < self.input.len() {
            self.cursor_position += 1;
        }
    }

    /// Submit current input and return it
    pub fn submit_input(&mut self) -> String {
        let input = self.input.clone();

        // Add to history if not empty and different from last entry
        if !input.trim().is_empty() &&
           (self.command_history.is_empty() ||
            self.command_history.last() != Some(&input)) {
            self.command_history.push(input.clone());
        }

        // Reset input and history navigation
        self.input.clear();
        self.cursor_position = 0;
        self.history_position = None;
        self.history_temp_input = None;

        input
    }

    /// Navigate up in command history
    pub fn history_previous(&mut self) {
        if self.command_history.is_empty() {
            return;
        }

        match self.history_position {
            None => {
                // Starting history navigation - save current input
                if !self.input.is_empty() {
                    self.history_temp_input = Some(self.input.clone());
                }
                // Go to most recent command
                let pos = self.command_history.len() - 1;
                self.history_position = Some(pos);
                self.input = self.command_history[pos].clone();
                self.cursor_position = self.input.len();
            }
            Some(pos) if pos > 0 => {
                // Go to older command
                let new_pos = pos - 1;
                self.history_position = Some(new_pos);
                self.input = self.command_history[new_pos].clone();
                self.cursor_position = self.input.len();
            }
            _ => {
                // Already at oldest command, do nothing
            }
        }
    }

    /// Navigate down in command history
    pub fn history_next(&mut self) {
        match self.history_position {
            Some(pos) if pos < self.command_history.len() - 1 => {
                // Go to newer command
                let new_pos = pos + 1;
                self.history_position = Some(new_pos);
                self.input = self.command_history[new_pos].clone();
                self.cursor_position = self.input.len();
            }
            Some(_) => {
                // At newest command, restore temp input or clear
                self.history_position = None;
                self.input = self.history_temp_input.take().unwrap_or_default();
                self.cursor_position = self.input.len();
            }
            None => {
                // Not in history mode, do nothing
            }
        }
    }

    /// Move cursor to start of line
    pub fn move_cursor_start(&mut self) {
        self.cursor_position = 0;
    }

    /// Move cursor to end of line
    pub fn move_cursor_end(&mut self) {
        self.cursor_position = self.input.len();
    }

    /// Delete from cursor to end of line
    pub fn delete_to_end(&mut self) {
        self.input.truncate(self.cursor_position);
    }

    /// Delete word before cursor
    pub fn delete_word(&mut self) {
        if self.cursor_position == 0 {
            return;
        }

        let before = &self.input[..self.cursor_position];
        let trimmed = before.trim_end();

        if trimmed.is_empty() {
            self.cursor_position = 0;
            self.input = self.input[self.cursor_position..].to_string();
            return;
        }

        let last_space = trimmed.rfind(char::is_whitespace);
        let new_pos = last_space.map(|p| p + 1).unwrap_or(0);

        let after = &self.input[self.cursor_position..];
        self.input = format!("{}{}", &before[..new_pos], after);
        self.cursor_position = new_pos;
    }

    /// Clear the entire input
    pub fn clear_input(&mut self) {
        self.input.clear();
        self.cursor_position = 0;
    }

    /// Toggle focus between Input and Output panels
    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Input => Focus::Output,
            Focus::Output => Focus::Input,
        };
    }

    /// Check if output panel has focus
    pub fn is_output_focused(&self) -> bool {
        self.focus == Focus::Output
    }

    /// Check if input panel has focus
    pub fn is_input_focused(&self) -> bool {
        self.focus == Focus::Input
    }

    /// Calculate the number of visual lines the input will take with wrapping
    pub fn calculate_input_height(&self, width: usize) -> u16 {
        if self.input.is_empty() {
            return 1;
        }

        if width == 0 {
            return 1;
        }

        let mut lines = 1u16;
        let mut col = 0;

        for ch in self.input.chars() {
            if ch == '\n' {
                lines += 1;
                col = 0;
            } else {
                if col >= width {
                    lines += 1;
                    col = 0;
                }
                col += 1;
            }
        }

        lines
    }

    /// Update slash command suggestions based on current input
    pub fn update_slash_suggestions(&mut self) {
        // Only show suggestions if input starts with "/"
        if !self.input.starts_with('/') {
            self.slash_suggestions.clear();
            return;
        }

        // Define all available slash commands
        let all_commands = vec![
            "/exit - Exit the application",
            "/model - List available models",
            "/model <name> - Select a model",
        ];

        // Filter commands based on current input
        let input_lower = self.input.to_lowercase();
        self.slash_suggestions = all_commands
            .into_iter()
            .filter(|cmd| cmd.to_lowercase().starts_with(&input_lower))
            .map(|s| s.to_string())
            .collect();
    }

    /// Check if we should show slash suggestions
    pub fn should_show_slash_suggestions(&self) -> bool {
        self.input.starts_with('/') && !self.slash_suggestions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_history_path() {
        let path = App::history_file_path();
        assert!(path.is_some());
        let path = path.unwrap();
        assert!(path.to_string_lossy().contains(".netget_history"));
    }

    #[test]
    fn test_submit_input_adds_to_history() {
        let mut app = App::default();

        // Submit first command
        app.input = "listen on port 21".to_string();
        app.cursor_position = app.input.len();
        let result = app.submit_input();

        assert_eq!(result, "listen on port 21");
        assert_eq!(app.command_history.len(), 1);
        assert_eq!(app.input, "");
        assert_eq!(app.cursor_position, 0);

        // Submit second command
        app.input = "status".to_string();
        app.cursor_position = app.input.len();
        app.submit_input();

        assert_eq!(app.command_history.len(), 2);
    }

    #[test]
    fn test_submit_duplicate_not_added() {
        let mut app = App::default();

        app.input = "listen on port 21".to_string();
        app.submit_input();

        // Same command again
        app.input = "listen on port 21".to_string();
        app.submit_input();

        // Should only have one entry
        assert_eq!(app.command_history.len(), 1);
    }

    #[test]
    fn test_history_navigation() {
        let mut app = App::default();
        app.command_history = vec![
            "command1".to_string(),
            "command2".to_string(),
            "command3".to_string(),
        ];

        // Navigate up - should show command3
        app.history_previous();
        assert_eq!(app.input, "command3");
        assert_eq!(app.history_position, Some(2));

        // Navigate up again - should show command2
        app.history_previous();
        assert_eq!(app.input, "command2");
        assert_eq!(app.history_position, Some(1));

        // Navigate down - should show command3
        app.history_next();
        assert_eq!(app.input, "command3");
        assert_eq!(app.history_position, Some(2));

        // Navigate down again - should clear
        app.history_next();
        assert_eq!(app.input, "");
        assert_eq!(app.history_position, None);
    }

    #[test]
    fn test_history_temp_buffer() {
        let mut app = App::default();
        app.command_history = vec!["old_command".to_string()];

        // Type something
        app.input = "new text".to_string();
        app.cursor_position = app.input.len();

        // Navigate up - should save current input
        app.history_previous();
        assert_eq!(app.input, "old_command");

        // Navigate down - should restore saved input
        app.history_next();
        assert_eq!(app.input, "new text");
    }
}
