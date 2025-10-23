//! LLM-generated actions for command interpretation

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

use crate::network::ConnectionId;
use crate::protocol::BaseStack;

/// Response from LLM when interpreting a user command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandInterpretation {
    /// List of actions to execute
    #[serde(default)]
    pub actions: Vec<Action>,

    /// Optional message to display to the user
    pub message: Option<String>,
}

/// Actions that the LLM can request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// Update the current instruction
    UpdateInstruction {
        instruction: String,
    },

    /// Open a server connection
    OpenServer {
        port: u16,
        base_stack: String,  // Will be parsed to BaseStack
        #[serde(default)]
        protocol: Option<String>,  // For TcpRaw stack
        #[serde(default)]
        send_first: bool,  // True if server sends data first on connect (e.g., FTP, SMTP), false if server waits for client request (e.g., HTTP)
        #[serde(default)]
        initial_memory: Option<String>,  // Initial memory to store for this server
    },

    /// Open a client connection
    OpenClient {
        address: String,  // Will be parsed to SocketAddr
        base_stack: String,
        #[serde(default)]
        protocol: Option<String>,
    },

    /// Close a specific connection
    CloseConnection {
        #[serde(default)]
        connection_id: Option<String>,  // If None, close all
    },

    /// Display a message to the user
    ShowMessage {
        message: String,
    },

    /// Change the Ollama model
    ChangeModel {
        model: String,
    },
}

impl CommandInterpretation {
    /// Parse from LLM JSON response
    pub fn from_str(s: &str) -> anyhow::Result<Self> {
        serde_json::from_str(s).map_err(|e| anyhow::anyhow!("Failed to parse command interpretation: {}", e))
    }
}

impl Action {
    /// Parse base stack string to BaseStack enum
    pub fn parse_base_stack(s: &str) -> Option<BaseStack> {
        BaseStack::from_str(s)
    }

    /// Parse address string to SocketAddr
    pub fn parse_socket_addr(s: &str) -> Option<SocketAddr> {
        s.parse().ok()
    }

    /// Parse connection ID string
    pub fn parse_connection_id(s: &str) -> Option<ConnectionId> {
        ConnectionId::from_string(s)
    }
}
