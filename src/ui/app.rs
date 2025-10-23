//! Application state for the shell interface

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use tracing::{debug, warn};
use tui_textarea::TextArea;

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

/// Log level for output verbosity
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    /// ERROR: Critical errors only
    Error,
    /// WARN: Warnings and errors
    Warn,
    /// INFO: One line per request/response (default)
    Info,
    /// DEBUG: Detailed LLM responses, memory updates, actions
    Debug,
    /// TRACE: Full protocol and LLM content
    Trace,
}

impl Default for LogLevel {
    fn default() -> Self {
        LogLevel::Info
    }
}

impl LogLevel {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "error" => Some(LogLevel::Error),
            "warn" => Some(LogLevel::Warn),
            "info" => Some(LogLevel::Info),
            "debug" => Some(LogLevel::Debug),
            "trace" => Some(LogLevel::Trace),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Error => "ERROR",
            LogLevel::Warn => "WARN",
            LogLevel::Info => "INFO",
            LogLevel::Debug => "DEBUG",
            LogLevel::Trace => "TRACE",
        }
    }
}

/// Main application state for the TUI
pub struct App {
    /// User input buffer (using tui-textarea for better editing)
    pub textarea: TextArea<'static>,
    /// Command history
    pub command_history: Vec<String>,
    /// Current position in history (None = not browsing/comp history)
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
    /// Which panel currently has focus
    pub focus: Focus,
    /// Current log level
    pub log_level: LogLevel,
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
            textarea: TextArea::default(),
            command_history: Vec::new(),
            history_position: None,
            history_temp_input: None,
            output_messages: Vec::new(),
            connection_info: ConnectionInfo::default(),
            packet_stats: PacketStats::default(),
            scroll_offset: 0,
            focus: Focus::default(),
            log_level: LogLevel::default(),
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

    /// Escape newlines and other special characters for storage
    fn escape_for_storage(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    }

    /// Unescape newlines and other special characters from storage
    fn unescape_from_storage(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        let mut chars = s.chars();

        while let Some(ch) = chars.next() {
            if ch == '\\' {
                match chars.next() {
                    Some('n') => result.push('\n'),
                    Some('r') => result.push('\r'),
                    Some('t') => result.push('\t'),
                    Some('\\') => result.push('\\'),
                    Some(c) => {
                        result.push('\\');
                        result.push(c);
                    }
                    None => result.push('\\'),
                }
            } else {
                result.push(ch);
            }
        }

        result
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
                    .map(|line| Self::unescape_from_storage(&line))
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
            writeln!(file, "{}", Self::escape_for_storage(command))?;
        }

        debug!("Saved {} commands to history", self.command_history.len());
        Ok(())
    }

    /// Create a new App instance (loads history from ~/.netget_history)
    pub fn new() -> Self {
        let mut app = Self::default();
        app.command_history = Self::load_history();
        // Interactive mode defaults to TRACE logging (logged to netget.log)
        app.log_level = LogLevel::Trace;
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
            // Auto-scroll: if we're at the bottom, keep us at the bottom
            let was_at_bottom = self.scroll_offset == 0;

            self.output_messages.push(message);

            // Stay at bottom if we were already there
            if was_at_bottom {
                self.scroll_offset = 0;
            }
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

    /// Log at ERROR level (always shown)
    pub fn log_error(&mut self, message: String) {
        self.add_message(format!("[ERROR] {}", message));
    }

    /// Log at WARN level (shown if level >= WARN)
    pub fn log_warn(&mut self, message: String) {
        if self.log_level >= LogLevel::Warn {
            self.add_message(format!("[WARN] {}", message));
        }
    }

    /// Log at INFO level (shown if level >= INFO)
    pub fn log_info(&mut self, message: String) {
        if self.log_level >= LogLevel::Info {
            self.add_message(format!("[INFO] {}", message));
        }
    }

    /// Log at DEBUG level (shown if level >= DEBUG)
    pub fn log_debug(&mut self, message: String) {
        if self.log_level >= LogLevel::Debug {
            self.add_message(format!("[DEBUG] {}", message));
        }
    }

    /// Log at TRACE level (shown if level >= TRACE)
    pub fn log_trace(&mut self, message: String) {
        if self.log_level >= LogLevel::Trace {
            self.add_message(format!("[TRACE] {}", message));
        }
    }

    /// Set log level
    pub fn set_log_level(&mut self, level: LogLevel) {
        self.log_level = level;
        self.add_message(format!("Log level set to {}", level.as_str()));
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

    /// Get the current input text
    pub fn get_input(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// Insert a newline at the current cursor position
    pub fn insert_newline(&mut self) {
        self.textarea.insert_newline();
        self.update_slash_suggestions();
    }

    /// Check if cursor is on the first line of the textarea
    pub fn is_cursor_on_first_line(&self) -> bool {
        let (row, _) = self.textarea.cursor();  // cursor() returns (row, col)
        row == 0
    }

    /// Check if cursor is on the last line of the textarea
    pub fn is_cursor_on_last_line(&self) -> bool {
        let (row, _) = self.textarea.cursor();  // cursor() returns (row, col)
        let total_lines = self.textarea.lines().len();
        row >= total_lines.saturating_sub(1)
    }

    /// Move cursor up within the textarea
    pub fn move_cursor_up(&mut self) {
        self.textarea.move_cursor(tui_textarea::CursorMove::Up);
    }

    /// Move cursor down within the textarea
    pub fn move_cursor_down(&mut self) {
        self.textarea.move_cursor(tui_textarea::CursorMove::Down);
    }

    /// Submit current input and return it
    pub fn submit_input(&mut self) -> String {
        let input = self.get_input();

        // Add to history if not empty and different from last entry
        if !input.trim().is_empty() &&
           (self.command_history.is_empty() ||
            self.command_history.last() != Some(&input)) {
            self.command_history.push(input.clone());
        }

        // Reset input and history navigation
        self.textarea = TextArea::default();
        self.history_position = None;
        self.history_temp_input = None;
        self.update_slash_suggestions();

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
                let current = self.get_input();
                if !current.is_empty() {
                    self.history_temp_input = Some(current);
                }
                // Go to most recent command
                let pos = self.command_history.len() - 1;
                self.history_position = Some(pos);
                self.textarea = TextArea::from(self.command_history[pos].lines().map(|s| s.to_string()).collect::<Vec<_>>());
                // Move cursor to beginning of first line when going back in history
                self.textarea.move_cursor(tui_textarea::CursorMove::Top);
            }
            Some(pos) if pos > 0 => {
                // Go to older command
                let new_pos = pos - 1;
                self.history_position = Some(new_pos);
                self.textarea = TextArea::from(self.command_history[new_pos].lines().map(|s| s.to_string()).collect::<Vec<_>>());
                // Move cursor to beginning of first line when going back in history
                self.textarea.move_cursor(tui_textarea::CursorMove::Top);
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
                self.textarea = TextArea::from(self.command_history[new_pos].lines().map(|s| s.to_string()).collect::<Vec<_>>());
                // Move cursor to end of last line when going forward in history
                self.textarea.move_cursor(tui_textarea::CursorMove::Bottom);
                self.textarea.move_cursor(tui_textarea::CursorMove::End);
            }
            Some(_) => {
                // At newest command, restore temp input or clear
                self.history_position = None;
                let temp = self.history_temp_input.take().unwrap_or_default();
                self.textarea = TextArea::from(temp.lines().map(|s| s.to_string()).collect::<Vec<_>>());
                // Move cursor to end of last line
                self.textarea.move_cursor(tui_textarea::CursorMove::Bottom);
                self.textarea.move_cursor(tui_textarea::CursorMove::End);
            }
            None => {
                // Not in history mode, do nothing
            }
        }
    }

    // Cursor movement methods removed - TextArea handles these internally

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

    /// Calculate the number of visual lines the input will take
    pub fn calculate_input_height(&self, _width: usize) -> u16 {
        self.textarea.lines().len().max(1) as u16
    }

    /// Update slash command suggestions based on current input
    pub fn update_slash_suggestions(&mut self) {
        let input = self.get_input();
        // Only show suggestions if input starts with "/"
        if !input.starts_with('/') {
            self.slash_suggestions.clear();
            return;
        }

        // Define all available slash commands
        let all_commands = vec![
            "/exit - Exit the application",
            "/model - List available models",
            "/model <name> - Select a model",
            "/log - Show current log level",
            "/log <level> - Set log level (error, warn, info, debug, trace)",
        ];

        // Filter commands based on current input
        let input_lower = input.to_lowercase();
        self.slash_suggestions = all_commands
            .into_iter()
            .filter(|cmd| cmd.to_lowercase().starts_with(&input_lower))
            .map(|s| s.to_string())
            .collect();
    }

    /// Check if we should show slash suggestions
    pub fn should_show_slash_suggestions(&self) -> bool {
        let input = self.get_input();
        input.starts_with('/') && !self.slash_suggestions.is_empty()
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
    fn test_escape_unescape_newlines() {
        // Test simple newline
        let original = "line 1\nline 2";
        let escaped = App::escape_for_storage(original);
        assert_eq!(escaped, "line 1\\nline 2");
        let unescaped = App::unescape_from_storage(&escaped);
        assert_eq!(unescaped, original);

        // Test multiple special characters
        let original = "line 1\nline 2\r\nline 3\ttab\\backslash";
        let escaped = App::escape_for_storage(original);
        let unescaped = App::unescape_from_storage(&escaped);
        assert_eq!(unescaped, original);

        // Test empty string
        let original = "";
        let escaped = App::escape_for_storage(original);
        let unescaped = App::unescape_from_storage(&escaped);
        assert_eq!(unescaped, original);

        // Test no special characters
        let original = "just plain text";
        let escaped = App::escape_for_storage(original);
        assert_eq!(escaped, original);
        let unescaped = App::unescape_from_storage(&escaped);
        assert_eq!(unescaped, original);
    }

    #[test]
    fn test_multiline_command_in_history() {
        let mut app = App::default();

        // Submit a multi-line command
        app.textarea = TextArea::from(vec!["line 1".to_string(), "line 2".to_string(), "line 3".to_string()]);
        let result = app.submit_input();

        assert_eq!(result, "line 1\nline 2\nline 3");
        assert_eq!(app.command_history.len(), 1);
        assert_eq!(app.command_history[0], "line 1\nline 2\nline 3");
    }

    #[test]
    fn test_submit_input_adds_to_history() {
        let mut app = App::default();

        // Submit first command
        app.textarea = TextArea::from(vec!["listen on port 21".to_string()]);
        let result = app.submit_input();

        assert_eq!(result, "listen on port 21");
        assert_eq!(app.command_history.len(), 1);
        assert_eq!(app.get_input(), "");

        // Submit second command
        app.textarea = TextArea::from(vec!["status".to_string()]);
        app.submit_input();

        assert_eq!(app.command_history.len(), 2);
    }

    #[test]
    fn test_submit_duplicate_not_added() {
        let mut app = App::default();

        app.textarea = TextArea::from(vec!["listen on port 21".to_string()]);
        app.submit_input();

        // Same command again
        app.textarea = TextArea::from(vec!["listen on port 21".to_string()]);
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
        assert_eq!(app.get_input(), "command3");
        assert_eq!(app.history_position, Some(2));

        // Navigate up again - should show command2
        app.history_previous();
        assert_eq!(app.get_input(), "command2");
        assert_eq!(app.history_position, Some(1));

        // Navigate down - should show command3
        app.history_next();
        assert_eq!(app.get_input(), "command3");
        assert_eq!(app.history_position, Some(2));

        // Navigate down again - should clear
        app.history_next();
        assert_eq!(app.get_input(), "");
        assert_eq!(app.history_position, None);
    }

    #[test]
    fn test_history_temp_buffer() {
        let mut app = App::default();
        app.command_history = vec!["old_command".to_string()];

        // Type something
        app.textarea = TextArea::from(vec!["new text".to_string()]);

        // Navigate up - should save current input
        app.history_previous();
        assert_eq!(app.get_input(), "old_command");

        // Navigate down - should restore saved input
        app.history_next();
        assert_eq!(app.get_input(), "new text");
    }
}
