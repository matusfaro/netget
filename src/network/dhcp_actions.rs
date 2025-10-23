//! DHCP protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, ProtocolActions},
    context::NetworkContext,
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

    fn get_sync_actions(&self, context: &NetworkContext) -> Vec<ActionDefinition> {
        match context {
            NetworkContext::DhcpRequest { .. } => {
                vec![send_dhcp_response_action(), ignore_request_action()]
            }
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
            "send_dhcp_response" => self.execute_send_dhcp_response(action, context),
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
        context: Option<&NetworkContext>,
    ) -> Result<ActionResult> {
        if let Some(NetworkContext::DhcpRequest { .. }) = context {
            let data = action
                .get("data")
                .and_then(|v| v.as_str())
                .context("Missing 'data' parameter")?;

            Ok(ActionResult::Output(data.as_bytes().to_vec()))
        } else {
            Err(anyhow::anyhow!(
                "send_dhcp_response requires DhcpRequest context"
            ))
        }
    }
}

fn send_dhcp_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_dhcp_response".to_string(),
        description: "Send DHCP response to the request".to_string(),
        parameters: vec![Parameter {
            name: "data".to_string(),
            type_hint: "string".to_string(),
            description: "DHCP response data".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_dhcp_response",
            "data": "dhcp_response_bytes"
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
