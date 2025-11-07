//! SSH client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::{ConnectContext, EventType};
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// SSH client connected event
pub static SSH_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ssh_connected",
        "SSH client successfully authenticated to server"
    )
    .with_parameters(vec![
        Parameter {
            name: "remote_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Remote SSH server address".to_string(),
            required: true,
        },
        Parameter {
            name: "username".to_string(),
            type_hint: "string".to_string(),
            description: "Username used for authentication".to_string(),
            required: true,
        },
    ])
});

/// SSH command output received event
pub static SSH_CLIENT_OUTPUT_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ssh_output_received",
        "Command output received from SSH server"
    )
    .with_parameters(vec![
        Parameter {
            name: "output".to_string(),
            type_hint: "string".to_string(),
            description: "Command output as UTF-8 string".to_string(),
            required: true,
        },
        Parameter {
            name: "exit_code".to_string(),
            type_hint: "number".to_string(),
            description: "Command exit code (if available)".to_string(),
            required: false,
        },
    ])
});

/// SSH client protocol action handler
pub struct SshClientProtocol;

impl SshClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Client for SshClientProtocol {
    fn connect(
        &self,
        ctx: ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::ssh::SshClient;

            SshClient::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.client_id,
                ctx.startup_params,
            )
            .await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "execute_command".to_string(),
                description: "Execute a command on the remote SSH server".to_string(),
                parameters: vec![
                    Parameter {
                        name: "command".to_string(),
                        type_hint: "string".to_string(),
                        description: "The shell command to execute".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "execute_command",
                    "command": "ls -la"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from the SSH server".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "disconnect"
                }),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "execute_command".to_string(),
                description: "Execute another command in response to output".to_string(),
                parameters: vec![
                    Parameter {
                        name: "command".to_string(),
                        type_hint: "string".to_string(),
                        description: "The shell command to execute".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "execute_command",
                    "command": "pwd"
                }),
            },
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait for more output before responding".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "wait_for_more"
                }),
            },
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "execute_command" => {
                let command = action
                    .get("command")
                    .and_then(|v| v.as_str())
                    .context("Missing 'command' field")?;

                Ok(ClientActionResult::Custom {
                    name: "execute_command".to_string(),
                    data: json!({ "command": command }),
                })
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            "wait_for_more" => Ok(ClientActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!("Unknown SSH client action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "SSH"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType {
                id: "ssh_connected".to_string(),
                description: "Triggered when SSH client authenticates successfully".to_string(),
                actions: vec![],
                parameters: vec![],
            },
            EventType {
                id: "ssh_output_received".to_string(),
                description: "Triggered when SSH command output is received".to_string(),
                actions: vec![],
                parameters: vec![],
            },
        ]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>SSH"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["ssh", "ssh client", "connect to ssh", "secure shell"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("russh library for SSH protocol")
            .llm_control("Execute commands and read output")
            .e2e_testing("OpenSSH server as test target")
            .build()
    }

    fn description(&self) -> &'static str {
        "SSH client for connecting to SSH servers and executing commands"
    }

    fn example_prompt(&self) -> &'static str {
        "Connect to SSH at localhost:22 with user 'admin' and execute 'ls -la'"
    }

    fn group_name(&self) -> &'static str {
        "Network Infrastructure"
    }

    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "username".to_string(),
                type_hint: "string".to_string(),
                description: "SSH username for authentication".to_string(),
                required: true,
                example: json!("testuser"),
            },
            ParameterDefinition {
                name: "password".to_string(),
                type_hint: "string".to_string(),
                description: "SSH password for authentication (if using password auth)".to_string(),
                required: false,
                example: json!("testpass"),
            },
            ParameterDefinition {
                name: "auth_method".to_string(),
                type_hint: "string".to_string(),
                description: "Authentication method: 'password' or 'publickey' (default: password)".to_string(),
                required: false,
                example: json!("password"),
            },
        ]
    }
}
