//! Event type definitions

use bytes::Bytes;
use std::net::SocketAddr;

use crate::network::connection::ConnectionId;
use crate::protocol::ProtocolType;

/// Main application event enum
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// Network-related event
    Network(NetworkEvent),
    /// User command
    UserCommand(UserCommand),
    /// Tick/timeout event
    Tick,
    /// Shutdown signal
    Shutdown,
}

/// Network events
#[derive(Debug, Clone)]
pub enum NetworkEvent {
    /// Server started listening on address
    Listening {
        addr: SocketAddr,
    },
    /// New connection established
    Connected {
        connection_id: ConnectionId,
        remote_addr: SocketAddr,
    },
    /// Connection closed
    Disconnected {
        connection_id: ConnectionId,
    },
    /// Data received from connection
    DataReceived {
        connection_id: ConnectionId,
        data: Bytes,
    },
    /// Data sent on connection
    DataSent {
        connection_id: ConnectionId,
        data: Bytes,
    },
    /// Network error occurred
    Error {
        connection_id: Option<ConnectionId>,
        error: String,
    },
}

/// User commands parsed from input
#[derive(Debug, Clone)]
pub enum UserCommand {
    /// Start listening on a port
    Listen {
        port: u16,
        protocol: ProtocolType,
    },
    /// Connect to a remote server (client mode)
    Connect {
        addr: SocketAddr,
        protocol: ProtocolType,
    },
    /// Close current connections
    Close,
    /// Add a file to the protocol handler (e.g., FTP)
    AddFile {
        name: String,
        content: Vec<u8>,
    },
    /// Query current status
    Status,
    /// Change the Ollama model
    ChangeModel {
        model: String,
    },
    /// Raw user input (let LLM decide)
    Raw {
        input: String,
    },
}

impl UserCommand {
    /// Parse a user input string into a command
    /// This is a simple parser - the LLM will do more sophisticated parsing
    pub fn parse(input: &str) -> Self {
        let input_lower = input.trim().to_lowercase();

        // Simple pattern matching for common commands
        if input_lower.starts_with("listen") || input_lower.starts_with("start") {
            // Try to extract port and protocol
            if let Some(port_str) = input_lower.split_whitespace().find(|s| s.parse::<u16>().is_ok()) {
                if let Ok(port) = port_str.parse::<u16>() {
                    // Try to detect protocol from input
                    let protocol = if input_lower.contains("ftp") {
                        ProtocolType::Ftp
                    } else if input_lower.contains("http") {
                        ProtocolType::Http
                    } else {
                        ProtocolType::Custom
                    };

                    return UserCommand::Listen { port, protocol };
                }
            }
        }

        if input_lower.starts_with("close") || input_lower.starts_with("stop") {
            return UserCommand::Close;
        }

        if input_lower.starts_with("status") || input_lower == "?" {
            return UserCommand::Status;
        }

        // Check for model change command
        if input_lower.starts_with("model ") {
            let model = input.trim()[6..].trim().to_string();
            return UserCommand::ChangeModel { model };
        }

        // Default: treat as raw input for LLM
        UserCommand::Raw {
            input: input.to_string(),
        }
    }
}
