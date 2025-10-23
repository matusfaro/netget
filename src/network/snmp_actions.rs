//! SNMP protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, ProtocolActions},
    context::NetworkContext,
    ActionDefinition, Parameter,
};
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;

/// SNMP protocol action handler
pub struct SnmpProtocol;

impl SnmpProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl ProtocolActions for SnmpProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // SNMP has async action for sending traps
        vec![send_trap_action()]
    }

    fn get_sync_actions(&self, context: &NetworkContext) -> Vec<ActionDefinition> {
        match context {
            NetworkContext::SnmpRequest { .. } => vec![
                send_snmp_response_action(),
                send_snmp_error_action(),
                ignore_request_action(),
            ],
            _ => Vec::new(),
        }
    }

    fn execute_action(
        &self,
        action: serde_json::Value,
        context: Option<&NetworkContext>,
    ) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "send_trap" => self.execute_send_trap(action),
            "send_snmp_response" => self.execute_send_snmp_response(action, context),
            "send_snmp_error" => self.execute_send_snmp_error(action, context),
            "ignore_request" => Ok(ActionResult::NoAction),
            _ => Err(anyhow::anyhow!("Unknown SNMP action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "SNMP"
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
    fn execute_send_snmp_response(
        &self,
        action: serde_json::Value,
        context: Option<&NetworkContext>,
    ) -> Result<ActionResult> {
        // Verify we have SNMP context
        if let Some(NetworkContext::SnmpRequest { .. }) = context {
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
                serde_json::to_vec(&response_data)
                    .context("Failed to serialize SNMP response")?,
            ))
        } else {
            Err(anyhow::anyhow!(
                "send_snmp_response requires SnmpRequest context"
            ))
        }
    }

    /// Execute send_snmp_error sync action
    fn execute_send_snmp_error(
        &self,
        action: serde_json::Value,
        context: Option<&NetworkContext>,
    ) -> Result<ActionResult> {
        // Verify we have SNMP context
        if let Some(NetworkContext::SnmpRequest { .. }) = context {
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
                serde_json::to_vec(&response_data)
                    .context("Failed to serialize SNMP error")?,
            ))
        } else {
            Err(anyhow::anyhow!(
                "send_snmp_error requires SnmpRequest context"
            ))
        }
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
