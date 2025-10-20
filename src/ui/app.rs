//! Application state and rendering logic for the TUI

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use tracing::{debug, warn};

use super::layout::AppLayout;

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
    /// LLM response messages
    pub llm_messages: Vec<String>,
    /// Status/summary messages
    pub status_messages: Vec<String>,
    /// Connection information
    pub connection_info: ConnectionInfo,
    /// Packet statistics
    pub packet_stats: PacketStats,
    /// Scroll positions for messages
    pub llm_scroll: u16,
    pub status_scroll: u16,
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
            llm_messages: Vec::new(),
            status_messages: Vec::new(),
            connection_info: ConnectionInfo::default(),
            packet_stats: PacketStats::default(),
            llm_scroll: 0,
            status_scroll: 0,
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

    /// Add a message to the LLM messages panel
    pub fn add_llm_message(&mut self, message: String) {
        self.llm_messages.push(message);
    }

    /// Add a message to the status panel
    pub fn add_status_message(&mut self, message: String) {
        self.status_messages.push(message);
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

    /// Render the UI
    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let layout = AppLayout::new(area);

        // Render user input panel
        self.render_input_panel(frame, layout.input);

        // Render LLM messages panel
        self.render_llm_panel(frame, layout.llm_output);

        // Render connection info panel
        self.render_connection_panel(frame, layout.connection_info);

        // Render status panel
        self.render_status_panel(frame, layout.status);
    }

    fn render_input_panel(&self, frame: &mut Frame, area: Rect) {
        // Build title with shortcuts and history indicator
        let title = if let Some(pos) = self.history_position {
            format!("Input [History {}/{}] (Shift+Enter=newline, Ctrl+C=quit)",
                    pos + 1, self.command_history.len())
        } else {
            "Input (Enter=submit, Shift+Enter=newline, Up/Down=history, Ctrl+C=quit)".to_string()
        };

        let input_text = Paragraph::new(self.input.as_str())
            .style(Style::default().fg(Color::White).bg(Color::Blue))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                    .style(Style::default().bg(Color::Blue)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(input_text, area);

        // Calculate cursor position accounting for newlines and wrapping
        let text_before_cursor = &self.input[..self.cursor_position];
        let lines: Vec<&str> = text_before_cursor.split('\n').collect();
        let line_count = lines.len() as u16;
        let col_in_line = lines.last().map(|l| l.len()).unwrap_or(0) as u16;

        // Position cursor (accounting for border)
        let cursor_x = area.x + col_in_line + 1;
        let cursor_y = area.y + line_count;

        // Only set cursor if it's within the visible area
        if cursor_y < area.y + area.height - 1 {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }

    fn render_llm_panel(&self, frame: &mut Frame, area: Rect) {
        let messages: Vec<ListItem> = self
            .llm_messages
            .iter()
            .map(|m| ListItem::new(m.as_str()).style(Style::default().fg(Color::White).bg(Color::Blue)))
            .collect();

        let messages_list = List::new(messages)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("LLM Responses")
                    .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                    .style(Style::default().bg(Color::Blue)),
            );

        frame.render_widget(messages_list, area);
    }

    fn render_connection_panel(&self, frame: &mut Frame, area: Rect) {
        let info_text = vec![
            Line::from(vec![
                Span::styled("Mode: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD).bg(Color::Blue)),
                Span::styled(&self.connection_info.mode, Style::default().fg(Color::White).bg(Color::Blue)),
            ]),
            Line::from(vec![
                Span::styled("Protocol: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD).bg(Color::Blue)),
                Span::styled(&self.connection_info.protocol, Style::default().fg(Color::White).bg(Color::Blue)),
            ]),
            Line::from(vec![
                Span::styled("Model: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD).bg(Color::Blue)),
                Span::styled(&self.connection_info.model, Style::default().fg(Color::LightGreen).bg(Color::Blue)),
            ]),
            Line::from(vec![
                Span::styled("Local: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD).bg(Color::Blue)),
                Span::styled(
                    self.connection_info
                        .local_addr
                        .as_deref()
                        .unwrap_or("None"),
                    Style::default().fg(Color::White).bg(Color::Blue),
                ),
            ]),
            Line::from(vec![
                Span::styled("Remote: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD).bg(Color::Blue)),
                Span::styled(
                    self.connection_info
                        .remote_addr
                        .as_deref()
                        .unwrap_or("None"),
                    Style::default().fg(Color::White).bg(Color::Blue),
                ),
            ]),
            Line::from(vec![
                Span::styled("State: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD).bg(Color::Blue)),
                Span::styled(&self.connection_info.state, Style::default().fg(Color::White).bg(Color::Blue)),
            ]),
            Line::from(Span::styled("", Style::default().bg(Color::Blue))),
            Line::from(vec![Span::styled(
                "Packet Statistics:",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD).bg(Color::Blue),
            )]),
            Line::from(vec![
                Span::styled("  Packets RX: ", Style::default().fg(Color::LightCyan).bg(Color::Blue)),
                Span::styled(self.packet_stats.packets_received.to_string(), Style::default().fg(Color::White).bg(Color::Blue)),
            ]),
            Line::from(vec![
                Span::styled("  Packets TX: ", Style::default().fg(Color::LightCyan).bg(Color::Blue)),
                Span::styled(self.packet_stats.packets_sent.to_string(), Style::default().fg(Color::White).bg(Color::Blue)),
            ]),
            Line::from(vec![
                Span::styled("  Bytes RX: ", Style::default().fg(Color::LightCyan).bg(Color::Blue)),
                Span::styled(self.packet_stats.bytes_received.to_string(), Style::default().fg(Color::White).bg(Color::Blue)),
            ]),
            Line::from(vec![
                Span::styled("  Bytes TX: ", Style::default().fg(Color::LightCyan).bg(Color::Blue)),
                Span::styled(self.packet_stats.bytes_sent.to_string(), Style::default().fg(Color::White).bg(Color::Blue)),
            ]),
        ];

        let info_paragraph = Paragraph::new(info_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Connection Info")
                    .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                    .style(Style::default().bg(Color::Blue)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(info_paragraph, area);
    }

    fn render_status_panel(&self, frame: &mut Frame, area: Rect) {
        let messages: Vec<ListItem> = self
            .status_messages
            .iter()
            .map(|m| ListItem::new(m.as_str()).style(Style::default().fg(Color::LightCyan).bg(Color::Blue)))
            .collect();

        let status_list = List::new(messages)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Status / Activity Log")
                    .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                    .style(Style::default().bg(Color::Blue)),
            );

        frame.render_widget(status_list, area);
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
