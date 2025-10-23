//! Common actions available in all prompts
//!
//! This module defines actions that are available in both user input
//! and network event prompts (show_message, memory operations, etc.).

use super::{ActionDefinition, Parameter};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Common actions available in all contexts
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommonAction {
    /// Display a message to the user
    ShowMessage {
        message: String,
    },

    /// Open a new server
    OpenServer {
        port: u16,
        base_stack: String,
        #[serde(default)]
        send_first: bool,
        #[serde(default)]
        initial_memory: Option<String>,
        instruction: String,
    },

    /// Close a server (closes all if server_id not specified)
    CloseServer {
        #[serde(default)]
        server_id: Option<u32>,
    },

    /// Update the server instruction (combines with existing)
    UpdateInstruction {
        instruction: String,
    },

    /// Change the LLM model
    ChangeModel {
        model: String,
    },

    /// Replace global memory completely
    SetMemory {
        value: String,
    },

    /// Append to global memory
    AppendMemory {
        value: String,
    },
}

impl CommonAction {
    /// Parse from JSON value
    pub fn from_json(value: &serde_json::Value) -> Result<Self> {
        serde_json::from_value(value.clone())
            .context("Failed to parse common action")
    }
}

/// Get action definition for show_message
pub fn show_message_action() -> ActionDefinition {
    ActionDefinition {
        name: "show_message".to_string(),
        description: "Display a message to the user controlling NetGet".to_string(),
        parameters: vec![
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Message to display".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "show_message",
            "message": "Server started successfully on port 8080"
        }),
    }
}

/// Get action definition for open_server
pub fn open_server_action() -> ActionDefinition {
    ActionDefinition {
        name: "open_server".to_string(),
        description: "Start a new server".to_string(),
        parameters: vec![
            Parameter {
                name: "port".to_string(),
                type_hint: "number".to_string(),
                description: "Port number to listen on".to_string(),
                required: true,
            },
            Parameter {
                name: "base_stack".to_string(),
                type_hint: "string".to_string(),
                description: "Stack: tcp, http, udp, snmp, dns, dhcp, ntp, ssh, irc".to_string(),
                required: true,
            },
            Parameter {
                name: "send_first".to_string(),
                type_hint: "boolean".to_string(),
                description: "True if server sends data first (FTP, SMTP), false if it waits for client (HTTP)".to_string(),
                required: false,
            },
            Parameter {
                name: "initial_memory".to_string(),
                type_hint: "string".to_string(),
                description: "Optional initial global memory".to_string(),
                required: false,
            },
            Parameter {
                name: "instruction".to_string(),
                type_hint: "string".to_string(),
                description: "Detailed instructions for handling network events".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "open_server",
            "port": 21,
            "base_stack": "tcp",
            "send_first": true,
            "instruction": "You are an FTP server. Respond to FTP commands like USER, PASS, LIST, RETR, QUIT with appropriate FTP response codes."
        }),
    }
}

/// Get action definition for close_server
pub fn close_server_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_server".to_string(),
        description: "Stop the current server".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_server"
        }),
    }
}

/// Get action definition for update_instruction
pub fn update_instruction_action() -> ActionDefinition {
    ActionDefinition {
        name: "update_instruction".to_string(),
        description: "Update the current server instruction (combines with existing instruction)".to_string(),
        parameters: vec![
            Parameter {
                name: "instruction".to_string(),
                type_hint: "string".to_string(),
                description: "New instruction to add/combine".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "update_instruction",
            "instruction": "For all HTTP requests, return status 404 with 'Not Found' message."
        }),
    }
}

/// Get action definition for change_model
pub fn change_model_action() -> ActionDefinition {
    ActionDefinition {
        name: "change_model".to_string(),
        description: "Switch to a different LLM model".to_string(),
        parameters: vec![
            Parameter {
                name: "model".to_string(),
                type_hint: "string".to_string(),
                description: "Model name (e.g., 'llama3.2:latest')".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "change_model",
            "model": "llama3.2:latest"
        }),
    }
}

/// Get action definition for set_memory
pub fn set_memory_action() -> ActionDefinition {
    ActionDefinition {
        name: "set_memory".to_string(),
        description: "Replace global memory completely".to_string(),
        parameters: vec![
            Parameter {
                name: "value".to_string(),
                type_hint: "string".to_string(),
                description: "New memory value".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "set_memory",
            "value": "User requested feature X"
        }),
    }
}

/// Get action definition for append_memory
pub fn append_memory_action() -> ActionDefinition {
    ActionDefinition {
        name: "append_memory".to_string(),
        description: "Append to global memory (added with newline separator)".to_string(),
        parameters: vec![
            Parameter {
                name: "value".to_string(),
                type_hint: "string".to_string(),
                description: "Text to append".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "append_memory",
            "value": "Additional context"
        }),
    }
}

/// Get all common action definitions
pub fn get_all_common_actions() -> Vec<ActionDefinition> {
    vec![
        show_message_action(),
        open_server_action(),
        close_server_action(),
        update_instruction_action(),
        change_model_action(),
        set_memory_action(),
        append_memory_action(),
    ]
}

/// Get common actions for user input (all common actions)
pub fn get_user_input_common_actions() -> Vec<ActionDefinition> {
    get_all_common_actions()
}

/// Get common actions for network events (exclude server management actions)
pub fn get_network_event_common_actions() -> Vec<ActionDefinition> {
    vec![
        show_message_action(),
        set_memory_action(),
        append_memory_action(),
    ]
}
