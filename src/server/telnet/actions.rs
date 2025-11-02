//! Telnet protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;
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

impl Server for TelnetProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::telnet::TelnetServer;
            let _send_first = ctx.startup_params
                .as_ref()
                .and_then(|p| p.get_optional_bool("send_first"))
                .unwrap_or(false);

            TelnetServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                                ctx.server_id,
            ).await
        })
    }


    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        vec![
            crate::llm::actions::ParameterDefinition {
                name: "send_first".to_string(),
                type_hint: "boolean".to_string(),
                description: "Whether the server should send the first message after connection (not typically needed for this protocol)".to_string(),
                required: false,
                example: serde_json::json!(false),
            },
        ]
    }
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

    fn get_event_types(&self) -> Vec<EventType> {
        get_telnet_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>Telnet"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["telnet"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, ProtocolState, PrivilegeRequirement};

        ProtocolMetadataV2::builder()
            .state(ProtocolState::Experimental)
            .privilege_requirement(PrivilegeRequirement::PrivilegedPort(23))
            .implementation("Simplified line-based (no IAC negotiation)")
            .llm_control("Terminal responses")
            .e2e_testing("telnet CLI / raw TCP")
            .notes("Telnet-lite, no option negotiation")
            .build()
    }

    fn description(&self) -> &'static str {
        "Telnet terminal server"
    }

    fn example_prompt(&self) -> &'static str {
        "Start a telnet server on port 23 that echoes commands"
    }

    fn group_name(&self) -> &'static str {
        "Application"
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

// ============================================================================
// Telnet Event Type Constants
// ============================================================================

pub static TELNET_MESSAGE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "telnet_message_received",
        "Telnet message received from a client"
    )
    .with_parameters(vec![
        Parameter {
            name: "message".to_string(),
            type_hint: "string".to_string(),
            description: "The Telnet message line received".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        send_telnet_message_action(),
        send_telnet_line_action(),
        send_telnet_prompt_action(),
        wait_for_more_action(),
        close_connection_action(),
    ])
});

pub fn get_telnet_event_types() -> Vec<EventType> {
    vec![
        TELNET_MESSAGE_RECEIVED_EVENT.clone(),
    ]
}
