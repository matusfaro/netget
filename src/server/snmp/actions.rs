//! SNMP protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// SNMP protocol action handler
pub struct SnmpProtocol;

impl SnmpProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Server for SnmpProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::snmp::SnmpServer;
            SnmpServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            ).await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // SNMP has async action for sending traps
        vec![send_trap_action()]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_snmp_response_action(),
            send_snmp_error_action(),
            ignore_request_action(),
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "send_trap" => self.execute_send_trap(action),
            "send_snmp_response" => self.execute_send_snmp_response(action),
            "send_snmp_error" => self.execute_send_snmp_error(action),
            "ignore_request" => Ok(ActionResult::NoAction),
            _ => Err(anyhow::anyhow!("Unknown SNMP action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "SNMP"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_snmp_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>SNMP"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["snmp"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadata {
        crate::protocol::metadata::ProtocolMetadata::new(
            crate::protocol::metadata::DevelopmentState::Beta
        )
    }

    fn description(&self) -> &'static str {
        "SNMP agent for network monitoring"
    }

    fn example_prompt(&self) -> &'static str {
        "SNMP Port 8161 serve OID 1.3.6.1.2.1.1.1.0 (sysDescr) return 'NetGet SNMP Server v0.1'"
    }

    fn group_name(&self) -> &'static str {
        "Core"
    }
}

impl SnmpProtocol {
    /// Execute send_trap async action
    fn execute_send_trap(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _target = action
            .get("target")
            .and_then(|v| v.as_str())
            .context("Missing 'target' parameter")?;

        let variables = action
            .get("variables")
            .and_then(|v| v.as_array())
            .context("Missing 'variables' parameter")?;

        // Encode trap data as JSON for now
        // The caller will need to convert this to actual SNMP trap format
        let trap_data = json!({
            "variables": variables
        });

        Ok(ActionResult::Output(
            serde_json::to_vec(&trap_data).context("Failed to serialize trap data")?,
        ))
    }

    /// Execute send_snmp_response sync action
    fn execute_send_snmp_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let variables = action
            .get("variables")
            .and_then(|v| v.as_array())
            .context("Missing 'variables' parameter")?;

        // Encode response data as JSON
        // The caller will convert this to actual SNMP response format
        let response_data = json!({
            "variables": variables,
            "error": false
        });

        Ok(ActionResult::Output(
            serde_json::to_vec(&response_data).context("Failed to serialize SNMP response")?,
        ))
    }

    /// Execute send_snmp_error sync action
    fn execute_send_snmp_error(&self, action: serde_json::Value) -> Result<ActionResult> {
        let error_message = action
            .get("error_message")
            .and_then(|v| v.as_str())
            .context("Missing 'error_message' parameter")?;

        // Encode error response as JSON
        let response_data = json!({
            "error": true,
            "error_message": error_message
        });

        Ok(ActionResult::Output(
            serde_json::to_vec(&response_data).context("Failed to serialize SNMP error")?,
        ))
    }
}

/// Action definition for send_trap (async)
fn send_trap_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_trap".to_string(),
        description: "Send SNMP trap to a target address (async action)".to_string(),
        parameters: vec![
            Parameter {
                name: "target".to_string(),
                type_hint: "string".to_string(),
                description: "Target address in format 'IP:port'".to_string(),
                required: true,
            },
            Parameter {
                name: "variables".to_string(),
                type_hint: "array".to_string(),
                description: "Array of variable bindings with oid, type, and value".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_trap",
            "target": "127.0.0.1:162",
            "variables": [
                {"oid": "1.3.6.1.2.1.1.3.0", "type": "timeticks", "value": 12345}
            ]
        }),
    }
}

/// Action definition for send_snmp_response (sync)
fn send_snmp_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_snmp_response".to_string(),
        description: "Send SNMP response with variable bindings".to_string(),
        parameters: vec![Parameter {
            name: "variables".to_string(),
            type_hint: "array".to_string(),
            description: "Array of variable bindings with oid, type, and value".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_snmp_response",
            "variables": [
                {"oid": "1.3.6.1.2.1.1.1.0", "type": "string", "value": "System Description"},
                {"oid": "1.3.6.1.2.1.1.5.0", "type": "string", "value": "hostname"}
            ]
        }),
    }
}

/// Action definition for send_snmp_error (sync)
fn send_snmp_error_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_snmp_error".to_string(),
        description: "Send SNMP error response".to_string(),
        parameters: vec![Parameter {
            name: "error_message".to_string(),
            type_hint: "string".to_string(),
            description: "Error message".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_snmp_error",
            "error_message": "No such object"
        }),
    }
}

/// Action definition for ignore_request (sync)
fn ignore_request_action() -> ActionDefinition {
    ActionDefinition {
        name: "ignore_request".to_string(),
        description: "Ignore this SNMP request and don't send a response".to_string(),
        parameters: vec![],
        example: json!({
            "type": "ignore_request"
        }),
    }
}

// ============================================================================
// SNMP Event Type Constants
// ============================================================================

pub static SNMP_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "snmp_request",
        "SNMP client sent a GET/GETNEXT/GETBULK request"
    )
    .with_parameters(vec![
        Parameter {
            name: "request_type".to_string(),
            type_hint: "string".to_string(),
            description: "SNMP request type (GET, GETNEXT, GETBULK, SET)".to_string(),
            required: true,
        },
        Parameter {
            name: "oids".to_string(),
            type_hint: "array".to_string(),
            description: "Requested OIDs".to_string(),
            required: true,
        },
        Parameter {
            name: "community".to_string(),
            type_hint: "string".to_string(),
            description: "SNMP community string".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![
        send_snmp_response_action(),
        send_snmp_error_action(),
        ignore_request_action(),
    ])
});

pub fn get_snmp_event_types() -> Vec<EventType> {
    vec![
        SNMP_REQUEST_EVENT.clone(),
    ]
}
