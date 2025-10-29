//! Simplified application state for rolling terminal
//!
//! This module provides the minimal state needed for the rolling terminal interface.
//! Most rendering logic has moved to sticky_footer.rs.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write as _};
use std::path::PathBuf;
use tracing::{debug, warn};

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

/// Server information for display
#[derive(Debug, Clone)]
pub struct ServerDisplayInfo {
    pub id: String,
    pub protocol: String,
    pub port: u16,
    pub status: String,
    pub connections: usize,
}

/// Connection information for display
#[derive(Debug, Clone)]
pub struct ConnectionDisplayInfo {
    pub id: u32,
    pub server_id: String,
    pub address: String,
    pub state: String,
}

/// Connection information for status bar
#[derive(Default, Clone)]
pub struct ConnectionInfo {
    pub mode: String,
    pub protocol: String,
    pub model: String,
    pub local_addr: Option<String>,
    pub remote_addr: Option<String>,
    pub state: String,
}

/// Packet statistics for status bar
#[derive(Default, Clone)]
pub struct PacketStats {
    pub packets_received: u64,
    pub packets_sent: u64,
    pub bytes_received: u64,
    pub bytes_sent: u64,
}

/// Simplified application state for rolling terminal
pub struct App {
    /// Command history
    pub command_history: Vec<String>,
    /// Current position in history (None = not browsing history)
    pub history_position: Option<usize>,
    /// Temporary buffer when browsing history
    pub history_temp_input: Option<String>,
    /// Connection information
    pub connection_info: ConnectionInfo,
    /// Packet statistics
    pub packet_stats: PacketStats,
    /// Current log level
    pub log_level: LogLevel,
    /// Slash command suggestions
    pub slash_suggestions: Vec<String>,
    /// Server list for display
    pub servers: Vec<ServerDisplayInfo>,
    /// Connection list for display
    pub connections: Vec<ConnectionDisplayInfo>,
    /// Whether to expand all connections (E key toggle)
    pub expand_all_connections: bool,
    /// Next global connection ID to assign
    pub next_global_connection_id: u32,
    /// Mapping from network ConnectionId to global UI ID
    pub connection_id_map: std::collections::HashMap<String, u32>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            command_history: Vec::new(),
            history_position: None,
            history_temp_input: None,
            connection_info: ConnectionInfo::default(),
            packet_stats: PacketStats::default(),
            log_level: LogLevel::Trace, // Interactive mode defaults to TRACE
            slash_suggestions: Vec::new(),
            servers: Vec::new(),
            connections: Vec::new(),
            expand_all_connections: false,
            next_global_connection_id: 1,
            connection_id_map: std::collections::HashMap::new(),
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
        app
    }

    /// Add command to history (deduplicates)
    pub fn add_to_history(&mut self, command: String) {
        if !command.trim().is_empty()
            && (self.command_history.is_empty()
                || self.command_history.last() != Some(&command))
        {
            self.command_history.push(command);
        }
    }

    /// Get or allocate a global connection ID for a network connection
    pub fn get_or_allocate_connection_id(&mut self, network_conn_id: String) -> u32 {
        if let Some(&id) = self.connection_id_map.get(&network_conn_id) {
            id
        } else {
            let id = self.next_global_connection_id;
            self.next_global_connection_id += 1;
            self.connection_id_map.insert(network_conn_id, id);
            id
        }
    }

    /// Remove a connection from the ID map (when connection closes)
    pub fn remove_connection_id(&mut self, network_conn_id: &str) {
        self.connection_id_map.remove(network_conn_id);
    }

    /// Update slash command suggestions based on input
    pub fn update_slash_suggestions(&mut self, input: &str) {
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

    /// Set log level
    pub fn set_log_level(&mut self, level: LogLevel) {
        self.log_level = level;
    }

    /// Toggle expand all connections
    pub fn toggle_expand_all(&mut self) {
        self.expand_all_connections = !self.expand_all_connections;
    }

    /// Legacy compatibility: add_llm_message (no-op in rolling terminal, output goes to stdout)
    pub fn add_llm_message(&mut self, _message: String) {
        // In rolling terminal mode, messages are printed directly to stdout
        // This method exists for compatibility with event handler
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
        let original = "line 1\nline 2";
        let escaped = App::escape_for_storage(original);
        assert_eq!(escaped, "line 1\\nline 2");
        let unescaped = App::unescape_from_storage(&escaped);
        assert_eq!(unescaped, original);
    }

    #[test]
    fn test_add_to_history_deduplicates() {
        // Use default() to avoid loading existing history from ~/.netget_history
        let mut app = App::default();
        app.add_to_history("command1".to_string());
        app.add_to_history("command1".to_string()); // Duplicate
        assert_eq!(app.command_history.len(), 1);

        app.add_to_history("command2".to_string());
        assert_eq!(app.command_history.len(), 2);
    }

    #[test]
    fn test_slash_suggestions_all_commands() {
        let mut app = App::default();

        // Single "/" should show all 5 commands
        app.update_slash_suggestions("/");
        assert_eq!(app.slash_suggestions.len(), 5);
        assert!(app.slash_suggestions.iter().any(|s| s.contains("/exit")));
        assert!(app.slash_suggestions.iter().any(|s| s.contains("/model")));
        assert!(app.slash_suggestions.iter().any(|s| s.contains("/log")));
    }

    #[test]
    fn test_slash_suggestions_filter_model() {
        let mut app = App::default();

        // "/mo" should filter to only /model commands
        app.update_slash_suggestions("/mo");
        assert_eq!(app.slash_suggestions.len(), 2);
        assert!(app.slash_suggestions.iter().all(|s| s.contains("/model")));

        // "/mod" should have same result as "/mo"
        app.update_slash_suggestions("/mod");
        assert_eq!(app.slash_suggestions.len(), 2);
        assert!(app.slash_suggestions.iter().all(|s| s.contains("/model")));

        // "/model" should match exactly /model commands
        app.update_slash_suggestions("/model");
        assert_eq!(app.slash_suggestions.len(), 2);
    }

    #[test]
    fn test_slash_suggestions_filter_log() {
        let mut app = App::default();

        // "/l" should filter to only /log commands
        app.update_slash_suggestions("/l");
        assert_eq!(app.slash_suggestions.len(), 2);
        assert!(app.slash_suggestions.iter().all(|s| s.contains("/log")));
    }

    #[test]
    fn test_slash_suggestions_filter_exit() {
        let mut app = App::default();

        // "/e" should filter to only /exit command
        app.update_slash_suggestions("/e");
        assert_eq!(app.slash_suggestions.len(), 1);
        assert!(app.slash_suggestions[0].contains("/exit"));
    }

    #[test]
    fn test_slash_suggestions_no_match() {
        let mut app = App::default();

        // "/xyz" should match nothing
        app.update_slash_suggestions("/xyz");
        assert_eq!(app.slash_suggestions.len(), 0);
    }

    #[test]
    fn test_slash_suggestions_clear_on_non_slash() {
        let mut app = App::default();

        // Start with suggestions
        app.update_slash_suggestions("/");
        assert!(!app.slash_suggestions.is_empty());

        // Clear when input doesn't start with "/"
        app.update_slash_suggestions("hello");
        assert_eq!(app.slash_suggestions.len(), 0);

        // Empty input also clears
        app.update_slash_suggestions("/");
        app.update_slash_suggestions("");
        assert_eq!(app.slash_suggestions.len(), 0);
    }

    #[test]
    fn test_slash_suggestions_case_insensitive() {
        let mut app = App::default();

        // Upper case "/MO" should still match /model
        app.update_slash_suggestions("/MO");
        assert_eq!(app.slash_suggestions.len(), 2);
        assert!(app.slash_suggestions.iter().all(|s| s.contains("/model")));

        // Mixed case "/Log" should match /log
        app.update_slash_suggestions("/Log");
        assert_eq!(app.slash_suggestions.len(), 2);
        assert!(app.slash_suggestions.iter().all(|s| s.contains("/log")));
    }
}
