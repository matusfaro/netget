//! SSH protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, ProtocolActions},
    context::NetworkContext,
    ActionDefinition, Parameter,
};
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;

pub struct SshProtocol;

impl SshProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl ProtocolActions for SshProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // SSH could have async actions like send_to_connection, similar to TCP
        Vec::new()
    }

    fn get_sync_actions(&self, context: &NetworkContext) -> Vec<ActionDefinition> {
        match context {
            NetworkContext::SshConnection { .. } => vec![
                send_ssh_data_action(),
                wait_for_more_action(),
                close_connection_action(),
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
            "send_ssh_data" => self.execute_send_ssh_data(action, context),
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            "close_connection" => Ok(ActionResult::CloseConnection),
            _ => Err(anyhow::anyhow!("Unknown SSH action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "SSH"
    }
}

impl SshProtocol {
    fn execute_send_ssh_data(
        &self,
        action: serde_json::Value,
        context: Option<&NetworkContext>,
    ) -> Result<ActionResult> {
        if let Some(NetworkContext::SshConnection { .. }) = context {
            let data = action
                .get("data")
                .and_then(|v| v.as_str())
                .context("Missing 'data' parameter")?;

            Ok(ActionResult::Output(data.as_bytes().to_vec()))
        } else {
            Err(anyhow::anyhow!(
                "send_ssh_data requires SshConnection context"
            ))
        }
    }
}

fn send_ssh_data_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_ssh_data".to_string(),
        description: "Send data over the SSH connection".to_string(),
        parameters: vec![Parameter {
            name: "data".to_string(),
            type_hint: "string".to_string(),
            description: "Data to send".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_ssh_data",
            "data": "SSH-2.0-OpenSSH_8.0\r\n"
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
        description: "Close the SSH connection".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_connection"
        }),
    }
}
