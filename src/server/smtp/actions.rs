//! SMTP protocol actions implementation

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

/// SMTP protocol action handler
pub struct SmtpProtocol;

impl SmtpProtocol {
    pub fn new() -> Self {
        Self
    }

    fn execute_send_smtp_greeting(&self, action: serde_json::Value) -> Result<ActionResult> {
        let hostname = action
            .get("hostname")
            .and_then(|v| v.as_str())
            .unwrap_or("localhost");

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("ESMTP Service Ready");

        let response = format!("220 {} {}\r\n", hostname, message);

        debug!("SMTP sending greeting: {}", response.trim());
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_smtp_ok(&self, action: serde_json::Value) -> Result<ActionResult> {
        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("OK");

        let response = format!("250 {}\r\n", message);

        debug!("SMTP sending OK: {}", message);
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_smtp_ehlo(&self, action: serde_json::Value) -> Result<ActionResult> {
        let hostname = action
            .get("hostname")
            .and_then(|v| v.as_str())
            .unwrap_or("localhost");

        let extensions = action
            .get("extensions")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_else(|| vec!["8BITMIME", "SIZE 10240000"]);

        let mut response = format!("250-{}\r\n", hostname);
        for (i, ext) in extensions.iter().enumerate() {
            if i == extensions.len() - 1 {
                response.push_str(&format!("250 {}\r\n", ext));
            } else {
                response.push_str(&format!("250-{}\r\n", ext));
            }
        }

        debug!("SMTP sending EHLO response");
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_smtp_start_data(&self, _action: serde_json::Value) -> Result<ActionResult> {
        let response = "354 Start mail input; end with <CRLF>.<CRLF>\r\n";

        debug!("SMTP sending start data");
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_smtp_error(&self, action: serde_json::Value) -> Result<ActionResult> {
        let code = action.get("code").and_then(|v| v.as_u64()).unwrap_or(500);

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .context("Missing 'message' parameter")?;

        let response = format!("{} {}\r\n", code, message);

        debug!("SMTP sending error {}: {}", code, message);
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_smtp_quit(&self, action: serde_json::Value) -> Result<ActionResult> {
        let hostname = action
            .get("hostname")
            .and_then(|v| v.as_str())
            .unwrap_or("localhost");

        let response = format!("221 {} closing connection\r\n", hostname);

        debug!("SMTP sending QUIT response");
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_send_smtp_message(&self, action: serde_json::Value) -> Result<ActionResult> {
        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .context("Missing 'message' parameter")?;

        // Ensure message ends with \r\n
        let formatted = if message.ends_with("\r\n") {
            message.to_string()
        } else if message.ends_with('\n') {
            format!("{}\r", message.trim_end_matches('\n'))
        } else {
            format!("{}\r\n", message)
        };

        debug!("SMTP sending custom message: {}", formatted.trim());
        Ok(ActionResult::Output(formatted.as_bytes().to_vec()))
    }
}

impl Server for SmtpProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::smtp::SmtpServer;
            SmtpServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            ).await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // SMTP doesn't need async actions for now
        Vec::new()
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_smtp_greeting_action(),
            send_smtp_ok_action(),
            send_smtp_ehlo_action(),
            send_smtp_start_data_action(),
            send_smtp_error_action(),
            send_smtp_quit_action(),
            send_smtp_message_action(),
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
            "send_smtp_greeting" => self.execute_send_smtp_greeting(action),
            "send_smtp_ok" => self.execute_send_smtp_ok(action),
            "send_smtp_ehlo" => self.execute_send_smtp_ehlo(action),
            "send_smtp_start_data" => self.execute_send_smtp_start_data(action),
            "send_smtp_error" => self.execute_send_smtp_error(action),
            "send_smtp_quit" => self.execute_send_smtp_quit(action),
            "send_smtp_message" => self.execute_send_smtp_message(action),
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            "close_connection" => Ok(ActionResult::CloseConnection),
            _ => Err(anyhow::anyhow!("Unknown SMTP action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "SMTP"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_smtp_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>SMTP"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["smtp", "mail", "email"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadata {
        crate::protocol::metadata::ProtocolMetadata::new(
            crate::protocol::metadata::DevelopmentState::Alpha
        )
    }

    fn description(&self) -> &'static str {
        "SMTP mail server"
    }

    fn example_prompt(&self) -> &'static str {
        "Start an SMTP mail server on port 25"
    }

    fn group_name(&self) -> &'static str {
        "Application Protocols"
    }
}

// Action definitions

fn send_smtp_greeting_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_smtp_greeting".to_string(),
        description: "Send SMTP greeting banner (220 response)".to_string(),
        parameters: vec![
            Parameter {
                name: "hostname".to_string(),
                type_hint: "string".to_string(),
                description: "Server hostname (default: localhost)".to_string(),
                required: false,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Greeting message (default: 'ESMTP Service Ready')".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_smtp_greeting",
            "hostname": "mail.example.com",
            "message": "ESMTP Service Ready"
        }),
    }
}

fn send_smtp_ok_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_smtp_ok".to_string(),
        description: "Send SMTP OK response (250)".to_string(),
        parameters: vec![Parameter {
            name: "message".to_string(),
            type_hint: "string".to_string(),
            description: "OK message (default: 'OK')".to_string(),
            required: false,
        }],
        example: json!({
            "type": "send_smtp_ok",
            "message": "Requested mail action okay, completed"
        }),
    }
}

fn send_smtp_ehlo_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_smtp_ehlo".to_string(),
        description: "Send SMTP EHLO response with extensions".to_string(),
        parameters: vec![
            Parameter {
                name: "hostname".to_string(),
                type_hint: "string".to_string(),
                description: "Server hostname (default: localhost)".to_string(),
                required: false,
            },
            Parameter {
                name: "extensions".to_string(),
                type_hint: "array".to_string(),
                description: "SMTP extensions (default: ['8BITMIME', 'SIZE 10240000'])".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_smtp_ehlo",
            "hostname": "mail.example.com",
            "extensions": ["8BITMIME", "SIZE 10240000", "STARTTLS"]
        }),
    }
}

fn send_smtp_start_data_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_smtp_start_data".to_string(),
        description: "Send SMTP start data response (354)".to_string(),
        parameters: vec![],
        example: json!({
            "type": "send_smtp_start_data"
        }),
    }
}

fn send_smtp_error_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_smtp_error".to_string(),
        description: "Send SMTP error response".to_string(),
        parameters: vec![
            Parameter {
                name: "code".to_string(),
                type_hint: "number".to_string(),
                description: "SMTP error code (e.g., 550, 500) (default: 500)".to_string(),
                required: false,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Error message".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_smtp_error",
            "code": 550,
            "message": "Mailbox unavailable"
        }),
    }
}

fn send_smtp_quit_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_smtp_quit".to_string(),
        description: "Send SMTP QUIT response (221) and prepare to close".to_string(),
        parameters: vec![Parameter {
            name: "hostname".to_string(),
            type_hint: "string".to_string(),
            description: "Server hostname (default: localhost)".to_string(),
            required: false,
        }],
        example: json!({
            "type": "send_smtp_quit",
            "hostname": "mail.example.com"
        }),
    }
}

fn send_smtp_message_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_smtp_message".to_string(),
        description: "Send a custom SMTP message (raw)".to_string(),
        parameters: vec![Parameter {
            name: "message".to_string(),
            type_hint: "string".to_string(),
            description: "SMTP message (will auto-add \\r\\n if not present)".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_smtp_message",
            "message": "250 2.1.0 Sender OK"
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
        description: "Close the SMTP connection".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_connection"
        }),
    }
}

// ============================================================================
// SMTP Action Constants
// ============================================================================

pub static SEND_SMTP_GREETING_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| send_smtp_greeting_action());
pub static SEND_SMTP_OK_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| send_smtp_ok_action());
pub static SEND_SMTP_EHLO_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| send_smtp_ehlo_action());
pub static SEND_SMTP_START_DATA_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| send_smtp_start_data_action());
pub static SEND_SMTP_ERROR_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| send_smtp_error_action());
pub static SEND_SMTP_QUIT_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| send_smtp_quit_action());
pub static SEND_SMTP_MESSAGE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| send_smtp_message_action());
pub static WAIT_FOR_MORE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| wait_for_more_action());
pub static CLOSE_CONNECTION_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| close_connection_action());

// ============================================================================
// SMTP Event Type Constants
// ============================================================================

/// SMTP command event - triggered when client sends an SMTP command
pub static SMTP_COMMAND_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "smtp_command",
        "SMTP command received from client"
    )
    .with_parameters(vec![
        Parameter {
            name: "command".to_string(),
            type_hint: "string".to_string(),
            description: "The SMTP command received (e.g., 'EHLO example.com', 'MAIL FROM:<sender@example.com>')".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        SEND_SMTP_GREETING_ACTION.clone(),
        SEND_SMTP_OK_ACTION.clone(),
        SEND_SMTP_EHLO_ACTION.clone(),
        SEND_SMTP_START_DATA_ACTION.clone(),
        SEND_SMTP_ERROR_ACTION.clone(),
        SEND_SMTP_QUIT_ACTION.clone(),
        SEND_SMTP_MESSAGE_ACTION.clone(),
        WAIT_FOR_MORE_ACTION.clone(),
        CLOSE_CONNECTION_ACTION.clone(),
    ])
});

/// Get SMTP event types
pub fn get_smtp_event_types() -> Vec<EventType> {
    vec![
        SMTP_COMMAND_EVENT.clone(),
    ]
}
