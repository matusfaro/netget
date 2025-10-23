//! Event type definitions

use bytes::Bytes;
use std::collections::HashMap;

/// HTTP response to be sent back to the client
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Bytes,
}

/// Main application event enum
#[derive(Debug)]
pub enum AppEvent {
    /// User command
    UserCommand(UserCommand),
    /// Tick/timeout event
    Tick,
    /// Shutdown signal
    Shutdown,
}

/// UDP response to be sent back
#[derive(Debug, Clone)]
pub struct UdpResponse {
    pub data: Vec<u8>,
}

/// User commands parsed from input
/// These are ONLY slash commands - all other input goes to LLM for interpretation
#[derive(Debug, Clone)]
pub enum UserCommand {
    /// Query current status (slash command: /status)
    Status,
    /// Show current model (slash command: /model)
    ShowModel,
    /// Change the Ollama model (slash command: /model <name>)
    ChangeModel {
        model: String,
    },
    /// Show current log level (slash command: /log)
    ShowLogLevel,
    /// Change log level (slash command: /log <level>)
    ChangeLogLevel {
        level: String,
    },
    /// Quit the application (slash command: /quit)
    Quit,
    /// Unknown slash command (error case)
    UnknownSlashCommand {
        command: String,
    },
    /// Regular user input (not a slash command) - send to LLM for interpretation
    Interpret {
        input: String,
    },
}

impl UserCommand {
    /// Parse a user input string into a command
    /// Only handles slash commands - everything else goes to LLM for interpretation
    pub fn parse(input: &str) -> Self {
        let trimmed = input.trim();

        // Check if it's a slash command
        if !trimmed.starts_with('/') {
            // Not a slash command - send to LLM for interpretation
            return UserCommand::Interpret {
                input: trimmed.to_string(),
            };
        }

        // Parse slash commands
        let input_lower = trimmed.to_lowercase();

        if input_lower == "/status" || input_lower == "/?" {
            return UserCommand::Status;
        }

        if input_lower == "/quit" || input_lower == "/exit" || input_lower == "/q" {
            return UserCommand::Quit;
        }

        // /model command
        if input_lower.starts_with("/model") {
            let rest = trimmed[6..].trim();
            if rest.is_empty() {
                // Show current model
                return UserCommand::ShowModel;
            }
            return UserCommand::ChangeModel { model: rest.to_string() };
        }

        // /log command
        if input_lower.starts_with("/log") {
            let rest = trimmed[4..].trim();
            if rest.is_empty() {
                // Show current log level
                return UserCommand::ShowLogLevel;
            }
            return UserCommand::ChangeLogLevel { level: rest.to_string() };
        }

        // Unknown slash command - return error, don't send to LLM
        // This prevents accidental LLM calls from typos like "/modle"
        UserCommand::UnknownSlashCommand {
            command: trimmed.to_string(),
        }
    }
}
