//! DHCP protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, ProtocolActions},
    ActionDefinition, Parameter,
};
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;

pub struct DhcpProtocol;

impl DhcpProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl ProtocolActions for DhcpProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        Vec::new()
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![send_dhcp_response_action(), ignore_request_action()]
    }

    fn execute_action(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "send_dhcp_response" => self.execute_send_dhcp_response(action),
            "ignore_request" => Ok(ActionResult::NoAction),
            _ => Err(anyhow::anyhow!("Unknown DHCP action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "DHCP"
    }
}

impl DhcpProtocol {
    fn execute_send_dhcp_response(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let data = action
            .get("data")
            .and_then(|v| v.as_str())
            .context("Missing 'data' parameter")?;

        // Try to decode as hex first (for binary DHCP packets)
        // If hex decode fails, treat as raw string
        let bytes = if let Ok(decoded) = hex::decode(data) {
            decoded
        } else {
            data.as_bytes().to_vec()
        };

        Ok(ActionResult::Output(bytes))
    }
}

fn send_dhcp_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_dhcp_response".to_string(),
        description: "Send DHCP response packet to the client".to_string(),
        parameters: vec![Parameter {
            name: "data".to_string(),
            type_hint: "string".to_string(),
            description: "DHCP response packet as hex-encoded string".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_dhcp_response",
            "data": "020106006395a3e3000080000000000000000000c0a8016400000000..."
        }),
    }
}

fn ignore_request_action() -> ActionDefinition {
    ActionDefinition {
        name: "ignore_request".to_string(),
        description: "Ignore this DHCP request".to_string(),
        parameters: vec![],
        example: json!({
            "type": "ignore_request"
        }),
    }
}
