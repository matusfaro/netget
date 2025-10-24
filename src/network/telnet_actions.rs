//! Telnet protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, ProtocolActions},
    ActionDefinition, Parameter,
};
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use tracing::debug;

/// Telnet protocol action handler
pub struct TelnetProtocol;

impl TelnetProtocol {
    pub fn new() -> Self {
        Self
    }

    fn execute_send_telnet_message(&self, action: serde_json::Value) -> Result<ActionResult> {
        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .context("Missing 'message' parameter")?;

        debug!("Telnet sending message: {}", message.trim());
        Ok(ActionResult::Output(message.as_bytes().to_vec()))
    }

    fn execute_send_telnet_line(&self, action: serde_json::Value) -> Result<ActionResult> {
        let line = action
            .get("line")
            .and_then(|v| v.as_str())
            .context("Missing 'line' parameter")?;

        // Add newline if not present
        let formatted = if line.ends_with("\r\n") {
            line.to_string()
        } else if line.ends_with('\n') {
            format!("{}\r", line.trim_end_matches('\n'))
        } else {
            format!("{}\r\n", line)
        };

        debug!("Telnet sending line: {}", formatted.trim());
        Ok(ActionResult::Output(formatted.as_bytes().to_vec()))
    }

    fn execute_send_telnet_prompt(&self, action: serde_json::Value) -> Result<ActionResult> {
        let prompt = action
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("> ");

        debug!("Telnet sending prompt: {:?}", prompt);
        Ok(ActionResult::Output(prompt.as_bytes().to_vec()))
    }
}

impl ProtocolActions for TelnetProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // Telnet doesn't need async actions for now
        Vec::new()
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_telnet_message_action(),
            send_telnet_line_action(),
            send_telnet_prompt_action(),
            wait_for_more_action(),
            close_connection_action(),
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "send_telnet_message" => self.execute_send_telnet_message(action),
            "send_telnet_line" => self.execute_send_telnet_line(action),
            "send_telnet_prompt" => self.execute_send_telnet_prompt(action),
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            "close_connection" => Ok(ActionResult::CloseConnection),
            _ => Err(anyhow::anyhow!("Unknown Telnet action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "Telnet"
    }
}

// Action definitions

fn send_telnet_message_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_telnet_message".to_string(),
        description: "Send a raw Telnet message (exact bytes, no modification)".to_string(),
        parameters: vec![Parameter {
            name: "message".to_string(),
            type_hint: "string".to_string(),
            description: "Message to send (sent as-is, no newline added)".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_telnet_message",
            "message": "Hello\r\n"
        }),
    }
}

fn send_telnet_line_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_telnet_line".to_string(),
        description: "Send a line of text (automatically adds \\r\\n if not present)".to_string(),
        parameters: vec![Parameter {
            name: "line".to_string(),
            type_hint: "string".to_string(),
            description: "Line of text to send".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_telnet_line",
            "line": "Welcome to the Telnet server!"
        }),
    }
}

fn send_telnet_prompt_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_telnet_prompt".to_string(),
        description: "Send a command prompt (e.g., '> ' or '$ ')".to_string(),
        parameters: vec![Parameter {
            name: "prompt".to_string(),
            type_hint: "string".to_string(),
            description: "Prompt text (default: '> ')".to_string(),
            required: false,
        }],
        example: json!({
            "type": "send_telnet_prompt",
            "prompt": "$ "
        }),
    }
}

fn wait_for_more_action() -> ActionDefinition {
    ActionDefinition {
        name: "wait_for_more".to_string(),
        description: "Wait for more data before responding".to_string(),
        parameters: vec![],
        example: json!({
            "type": "wait_for_more"
        }),
    }
}

fn close_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_connection".to_string(),
        description: "Close the Telnet connection".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_connection"
        }),
    }
}
