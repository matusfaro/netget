//! SSH protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, ProtocolActions},
    ActionDefinition, Parameter,
};
use crate::network::connection::ConnectionId;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;

/// Connection data for SSH protocol
pub struct SshConnectionData {
    pub username: Option<String>,
    pub authenticated: bool,
    pub channels: u32,
}

/// SSH protocol action handler
pub struct SshProtocol {
    /// Map of active connections (for async actions)
    connections: Arc<Mutex<HashMap<ConnectionId, SshConnectionData>>>,
}

impl SshProtocol {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_connections(
        connections: Arc<Mutex<HashMap<ConnectionId, SshConnectionData>>>,
    ) -> Self {
        Self { connections }
    }

    /// Add a connection to the protocol handler
    pub async fn add_connection(
        &self,
        connection_id: ConnectionId,
        username: Option<String>,
        authenticated: bool,
    ) {
        self.connections.lock().await.insert(
            connection_id,
            SshConnectionData {
                username,
                authenticated,
                channels: 1,
            },
        );
    }

    /// Remove a connection from the protocol handler
    pub async fn remove_connection(&self, connection_id: &ConnectionId) {
        self.connections.lock().await.remove(connection_id);
    }

    /// Get all active connections
    pub async fn get_connections(&self) -> Vec<(ConnectionId, SshConnectionData)> {
        self.connections
            .lock()
            .await
            .iter()
            .map(|(id, data)| {
                (
                    *id,
                    SshConnectionData {
                        username: data.username.clone(),
                        authenticated: data.authenticated,
                        channels: data.channels,
                    },
                )
            })
            .collect()
    }
}

impl ProtocolActions for SshProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            close_ssh_connection_action(),
            list_ssh_connections_action(),
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_ssh_data_action(),
            wait_for_more_action(),
            close_this_connection_action(),
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
            "send_ssh_data" => self.execute_send_ssh_data(action),
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            "close_this_connection" => Ok(ActionResult::CloseConnection),
            "close_ssh_connection" => self.execute_close_ssh_connection(action),
            "list_ssh_connections" => self.execute_list_ssh_connections(action),
            "ssh_auth_decision" => self.execute_ssh_auth_decision(action),
            "ssh_send_banner" => self.execute_ssh_send_banner(action),
            "ssh_shell_response" => self.execute_ssh_shell_response(action),
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
    ) -> Result<ActionResult> {
        let data = action
            .get("data")
            .and_then(|v| v.as_str())
            .context("Missing 'data' parameter")?;

        Ok(ActionResult::Output(data.as_bytes().to_vec()))
    }

    fn execute_ssh_auth_decision(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let allowed = action
            .get("allowed")
            .and_then(|v| v.as_bool())
            .context("Missing 'allowed' parameter")?;

        debug!("SSH auth decision action: allowed={}", allowed);

        // Store the decision in the action result metadata
        Ok(ActionResult::Custom {
            name: "ssh_auth_decision".to_string(),
            data: json!({"allowed": allowed}),
        })
    }

    fn execute_ssh_send_banner(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let banner = action
            .get("banner")
            .and_then(|v| v.as_str())
            .context("Missing 'banner' parameter")?;

        debug!("SSH sending banner: {}", banner);
        Ok(ActionResult::Output(banner.as_bytes().to_vec()))
    }

    fn execute_ssh_shell_response(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let response = action
            .get("response")
            .and_then(|v| v.as_str())
            .context("Missing 'response' parameter")?;

        debug!("SSH shell response: {}", response);
        Ok(ActionResult::Output(response.as_bytes().to_vec()))
    }

    fn execute_close_ssh_connection(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let connection_id_str = action
            .get("connection_id")
            .and_then(|v| v.as_str())
            .context("Missing 'connection_id' parameter")?;

        debug!("SSH close connection: {}", connection_id_str);

        // Return a custom result with the connection ID to close
        // The caller will need to handle the actual closing
        Ok(ActionResult::Custom {
            name: "close_ssh_connection".to_string(),
            data: json!({"connection_id": connection_id_str}),
        })
    }

    fn execute_list_ssh_connections(
        &self,
        _action: serde_json::Value,
    ) -> Result<ActionResult> {
        debug!("SSH list connections requested");

        // Return a custom result indicating list was requested
        // The executor will use the protocol's get_connections() method
        Ok(ActionResult::Custom {
            name: "list_ssh_connections".to_string(),
            data: json!({}),
        })
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

fn close_this_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_this_connection".to_string(),
        description: "Close the current SSH connection (sync action)".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_this_connection"
        }),
    }
}

/// Action definition for close_ssh_connection (async)
fn close_ssh_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_ssh_connection".to_string(),
        description: "Close a specific SSH connection (async action)".to_string(),
        parameters: vec![Parameter {
            name: "connection_id".to_string(),
            type_hint: "string".to_string(),
            description: "Connection ID to close".to_string(),
            required: true,
        }],
        example: json!({
            "type": "close_ssh_connection",
            "connection_id": "conn_12345"
        }),
    }
}

/// Action definition for list_ssh_connections (async)
fn list_ssh_connections_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_ssh_connections".to_string(),
        description: "List all active SSH connections".to_string(),
        parameters: vec![],
        example: json!({
            "type": "list_ssh_connections"
        }),
    }
}

/// Custom action for SSH authentication decisions
///
/// This action is used when SSH receives an authentication request.
/// The LLM decides whether to allow or deny the authentication.
pub fn ssh_auth_decision_action(username: &str, auth_type: &str) -> ActionDefinition {
    ActionDefinition {
        name: "ssh_auth_decision".to_string(),
        description: format!(
            "Decide whether to allow SSH authentication for user '{}' using method '{}'. \
            Consider the user instruction to determine if this user should be allowed. \
            Common scenarios: allow all users, allow specific usernames, deny all.",
            username, auth_type
        ),
        parameters: vec![Parameter {
            name: "allowed".to_string(),
            type_hint: "boolean".to_string(),
            description: "true to allow authentication, false to deny".to_string(),
            required: true,
        }],
        example: json!({
            "type": "ssh_auth_decision",
            "allowed": true
        }),
    }
}

/// Custom action for SSH shell banner/greeting
///
/// This action is used when an SSH shell session is opened.
/// The LLM can optionally send a banner or greeting message.
pub fn ssh_send_banner_action() -> ActionDefinition {
    ActionDefinition {
        name: "ssh_send_banner".to_string(),
        description: "Send a banner or greeting message when the SSH shell session opens. \
            This is typically a welcome message, MOTD (message of the day), or system information. \
            If no banner is needed, use show_message to indicate that instead.".to_string(),
        parameters: vec![Parameter {
            name: "banner".to_string(),
            type_hint: "string".to_string(),
            description: "The banner text to display (may include newlines)".to_string(),
            required: true,
        }],
        example: json!({
            "type": "ssh_send_banner",
            "banner": "Welcome to NetGet SSH Server!\nType 'help' for available commands.\n"
        }),
    }
}

/// Custom action for SSH shell command responses
///
/// This action is used when the SSH shell receives a command.
/// The LLM interprets the command and generates an appropriate response.
pub fn ssh_shell_response_action(command: &str) -> ActionDefinition {
    ActionDefinition {
        name: "ssh_shell_response".to_string(),
        description: format!(
            "Respond to the SSH shell command: '{}'. \
            Parse the command and generate appropriate output. \
            Common commands: pwd, ls, cd, cat, echo, help, exit, logout. \
            Use memory (set_memory/append_memory) to track state like current directory or session variables.",
            command
        ),
        parameters: vec![Parameter {
            name: "response".to_string(),
            type_hint: "string".to_string(),
            description: "The command output to send back to the user".to_string(),
            required: true,
        }],
        example: json!({
            "type": "ssh_shell_response",
            "response": "/home/user\n"
        }),
    }
}

/// Custom action for closing SSH connection (for shell commands)
///
/// This action allows the LLM to close the SSH connection,
/// typically in response to commands like 'exit', 'logout', or explicit close requests.
/// This is just an alias for close_this_connection for use in custom action lists.
pub fn ssh_close_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_this_connection".to_string(),
        description: "Close the SSH connection. Use this when the user types 'exit', 'logout', \
            or explicitly requests to close/disconnect. The connection will be terminated gracefully.".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_this_connection"
        }),
    }
}
