//! IRC protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, ProtocolActions},
    ActionDefinition, Parameter,
};
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;

pub struct IrcProtocol;

impl IrcProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl ProtocolActions for IrcProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // IRC could have async actions like broadcast_message
        Vec::new()
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_irc_data_action(),
            wait_for_more_action(),
            close_connection_action(),
        ]
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
            "send_irc_data" => self.execute_send_irc_data(action),
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            "close_connection" => Ok(ActionResult::CloseConnection),
            _ => Err(anyhow::anyhow!("Unknown IRC action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "IRC"
    }
}

impl IrcProtocol {
    fn execute_send_irc_data(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let data = action
            .get("data")
            .and_then(|v| v.as_str())
            .context("Missing 'data' parameter")?;

        Ok(ActionResult::Output(data.as_bytes().to_vec()))
    }
}

fn send_irc_data_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_irc_data".to_string(),
        description: "Send data over the IRC connection".to_string(),
        parameters: vec![Parameter {
            name: "data".to_string(),
            type_hint: "string".to_string(),
            description: "Data to send (IRC message)".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_irc_data",
            "data": ":server 001 user :Welcome to IRC\r\n"
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
        description: "Close the IRC connection".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_connection"
        }),
    }
}
