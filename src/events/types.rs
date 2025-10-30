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
    ChangeModel { model: String },
    /// Show current log level (slash command: /log)
    ShowLogLevel,
    /// Change log level (slash command: /log <level>)
    ChangeLogLevel {
        level: String,
    },
    /// Show current scripting environment (slash command: /script)
    ShowScriptingEnv,
    /// Change scripting environment (slash command: /script <env>)
    ChangeScriptingEnv {
        env: String,
    },
    /// Show current web search status (slash command: /web)
    ShowWebSearch,
    /// Set web search on/off (slash command: /web on|off)
    SetWebSearch { enabled: bool },
    /// Generate test output lines (slash command: /test <count>)
    TestOutput {
        count: usize,
    },
    /// Set custom footer status message (slash command: /footer_status <message>)
    SetFooterStatus {
        message: Option<String>,
    },
    /// Quit the application (slash command: /quit)
    Quit,
    /// Unknown slash command (error case)
    UnknownSlashCommand { command: String },
    /// Regular user input (not a slash command) - send to LLM for interpretation
    Interpret { input: String },
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
            return UserCommand::ChangeModel {
                model: rest.to_string(),
            };
        }

        // /log command
        if input_lower.starts_with("/log") {
            let rest = trimmed[4..].trim();
            if rest.is_empty() {
                // Show current log level
                return UserCommand::ShowLogLevel;
            }
            return UserCommand::ChangeLogLevel {
                level: rest.to_string(),
            };
        }

        // /script command
        if input_lower.starts_with("/script") {
            let rest = trimmed[7..].trim();
            if rest.is_empty() {
                // Show current scripting environment
                return UserCommand::ShowScriptingEnv;
            }
            return UserCommand::ChangeScriptingEnv {
                env: rest.to_string(),
            };
        }

        // /web command
        if input_lower.starts_with("/web") {
            let rest = trimmed[4..].trim();
            if rest.is_empty() {
                // Show current web search status
                return UserCommand::ShowWebSearch;
            }
            // Parse on/off argument
            let rest_lower = rest.to_lowercase();
            if rest_lower == "on" || rest_lower == "enable" || rest_lower == "enabled" || rest_lower == "true" {
                return UserCommand::SetWebSearch { enabled: true };
            } else if rest_lower == "off" || rest_lower == "disable" || rest_lower == "disabled" || rest_lower == "false" {
                return UserCommand::SetWebSearch { enabled: false };
            }
            // Unknown argument - treat as unknown command
            return UserCommand::UnknownSlashCommand {
                command: trimmed.to_string(),
            };
        }

        // /test command - generate test output lines
        if input_lower.starts_with("/test") {
            let rest = trimmed[5..].trim();
            if let Ok(count) = rest.parse::<usize>() {
                return UserCommand::TestOutput { count };
            }
            // Invalid count - treat as unknown command
        }

        // /footer_status command - set custom footer status message
        if input_lower.starts_with("/footer_status") {
            let rest = trimmed[14..].trim();
            let message = if rest.is_empty() {
                None
            } else {
                // Replace literal \n with actual newlines
                Some(rest.replace("\\n", "\n"))
            };
            return UserCommand::SetFooterStatus { message };
        }

        // Unknown slash command - return error, don't send to LLM
        // This prevents accidental LLM calls from typos like "/modle"
        UserCommand::UnknownSlashCommand {
            command: trimmed.to_string(),
        }
    }
}
