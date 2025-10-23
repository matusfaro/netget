//! DNS protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, ProtocolActions},
    context::NetworkContext,
    ActionDefinition, Parameter,
};
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;

/// DNS protocol action handler
pub struct DnsProtocol;

impl DnsProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl ProtocolActions for DnsProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        Vec::new() // DNS has no async actions
    }

    fn get_sync_actions(&self, context: &NetworkContext) -> Vec<ActionDefinition> {
        match context {
            NetworkContext::DnsQuery { .. } => {
                vec![send_dns_response_action(), ignore_query_action()]
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
            "send_dns_response" => self.execute_send_dns_response(action, context),
            "ignore_query" => Ok(ActionResult::NoAction),
            _ => Err(anyhow::anyhow!("Unknown DNS action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "DNS"
    }
}

impl DnsProtocol {
    fn execute_send_dns_response(
        &self,
        action: serde_json::Value,
        context: Option<&NetworkContext>,
    ) -> Result<ActionResult> {
        if let Some(NetworkContext::DnsQuery { .. }) = context {
            let data = action
                .get("data")
                .and_then(|v| v.as_str())
                .context("Missing 'data' parameter")?;

            Ok(ActionResult::Output(data.as_bytes().to_vec()))
        } else {
            Err(anyhow::anyhow!(
                "send_dns_response requires DnsQuery context"
            ))
        }
    }
}

fn send_dns_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_dns_response".to_string(),
        description: "Send DNS response to the query".to_string(),
        parameters: vec![Parameter {
            name: "data".to_string(),
            type_hint: "string".to_string(),
            description: "DNS response data".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_dns_response",
            "data": "dns_response_bytes"
        }),
    }
}

fn ignore_query_action() -> ActionDefinition {
    ActionDefinition {
        name: "ignore_query".to_string(),
        description: "Ignore this DNS query and don't send a response".to_string(),
        parameters: vec![],
        example: json!({
            "type": "ignore_query"
        }),
    }
}
