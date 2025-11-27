//! Syslog protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// Syslog protocol action handler
pub struct SyslogProtocol;

impl SyslogProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for SyslogProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // Syslog has async actions for forwarding logs
        vec![forward_syslog_action()]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            store_syslog_message_action(),
            ignore_syslog_message_action(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "Syslog"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_syslog_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>SYSLOG"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["syslog"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("syslog_loose v0.22 for parsing RFC 3164/5424 messages")
            .llm_control("Message filtering, storage, forwarding, alerting")
            .e2e_testing("logger command (Linux/macOS built-in)")
            .notes("RFC 3164 and RFC 5424 support, UDP transport")
            .build()
    }
    fn description(&self) -> &'static str {
        "Syslog server for log aggregation and analysis"
    }
    fn example_prompt(&self) -> &'static str {
        "Syslog Port 514 collect system logs and alert on critical errors"
    }
    fn group_name(&self) -> &'static str {
        "Core"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for SyslogProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::syslog::SyslogServer;
            SyslogServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            )
            .await
        })
    }
    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "forward_syslog" => self.execute_forward_syslog(action),
            "store_syslog_message" => self.execute_store_syslog_message(action),
            "ignore_syslog_message" => Ok(ActionResult::NoAction),
            _ => Err(anyhow::anyhow!("Unknown Syslog action: {}", action_type)),
        }
    }
}

impl SyslogProtocol {
    /// Execute forward_syslog async action
    fn execute_forward_syslog(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _target = action
            .get("target")
            .and_then(|v| v.as_str())
            .context("Missing 'target' parameter")?;

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .context("Missing 'message' parameter")?;

        // Return the message to be forwarded
        // The caller will handle the actual UDP send
        Ok(ActionResult::Output(message.as_bytes().to_vec()))
    }

    /// Execute store_syslog_message sync action
    fn execute_store_syslog_message(&self, action: serde_json::Value) -> Result<ActionResult> {
        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .context("Missing 'message' parameter")?;

        // Return confirmation
        let response = json!({
            "stored": true,
            "message": message
        });

        Ok(ActionResult::Output(
            serde_json::to_vec(&response).context("Failed to serialize response")?,
        ))
    }
}

/// Action definition for forward_syslog (async)
fn forward_syslog_action() -> ActionDefinition {
    ActionDefinition {
        name: "forward_syslog".to_string(),
        description: "Forward syslog message to another syslog server (async action)".to_string(),
        parameters: vec![
            Parameter {
                name: "target".to_string(),
                type_hint: "string".to_string(),
                description: "Target syslog server in format 'IP:port'".to_string(),
                required: true,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Syslog message to forward (raw format)".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "forward_syslog",
            "target": "192.168.1.100:514",
            "message": "<34>Oct 11 22:14:15 mymachine su: 'su root' failed for user on /dev/pts/8"
        }),
    }
}

/// Action definition for store_syslog_message (sync)
fn store_syslog_message_action() -> ActionDefinition {
    ActionDefinition {
        name: "store_syslog_message".to_string(),
        description: "Store syslog message for later analysis".to_string(),
        parameters: vec![Parameter {
            name: "message".to_string(),
            type_hint: "string".to_string(),
            description: "Syslog message to store".to_string(),
            required: true,
        }],
        example: json!({
            "type": "store_syslog_message",
            "message": "<34>Oct 11 22:14:15 mymachine su: 'su root' failed for user on /dev/pts/8"
        }),
    }
}

/// Action definition for ignore_syslog_message (sync)
fn ignore_syslog_message_action() -> ActionDefinition {
    ActionDefinition {
        name: "ignore_syslog_message".to_string(),
        description: "Ignore this syslog message (drop it)".to_string(),
        parameters: vec![],
        example: json!({
            "type": "ignore_syslog_message"
        }),
    }
}

// ============================================================================
// Syslog Event Type Constants
// ============================================================================

pub static SYSLOG_MESSAGE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("syslog_message", "Syslog client sent a log message", json!({"type": "placeholder", "event_id": "syslog_message"}))
    .with_parameters(vec![
        Parameter {
            name: "facility".to_string(),
            type_hint: "string".to_string(),
            description: "Syslog facility (kernel, user, mail, daemon, etc.)".to_string(),
            required: true,
        },
        Parameter {
            name: "severity".to_string(),
            type_hint: "string".to_string(),
            description: "Syslog severity (emergency, alert, critical, error, warning, notice, info, debug)".to_string(),
            required: true,
        },
        Parameter {
            name: "timestamp".to_string(),
            type_hint: "string".to_string(),
            description: "Message timestamp".to_string(),
            required: false,
        },
        Parameter {
            name: "hostname".to_string(),
            type_hint: "string".to_string(),
            description: "Hostname/source of the message".to_string(),
            required: false,
        },
        Parameter {
            name: "appname".to_string(),
            type_hint: "string".to_string(),
            description: "Application name".to_string(),
            required: false,
        },
        Parameter {
            name: "message".to_string(),
            type_hint: "string".to_string(),
            description: "The actual log message".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        store_syslog_message_action(),
        ignore_syslog_message_action(),
    ])
});

pub fn get_syslog_event_types() -> Vec<EventType> {
    vec![SYSLOG_MESSAGE_EVENT.clone()]
}
