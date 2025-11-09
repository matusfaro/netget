//! SSH Agent client protocol actions

use crate::llm::actions::client_trait::{Client, ClientActionResult};
use crate::llm::actions::{ActionDefinition, Parameter, ParameterDefinition, protocol_trait::Protocol};
use crate::protocol::{EventType, ConnectContext};
use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::LazyLock;

// Event type constants
pub static SSH_AGENT_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ssh_agent_client_connected",
        "SSH Agent client connected to agent socket",
    )
    .with_parameter(Parameter {
        name: "socket_path".to_string(),
        type_hint: "string".to_string(),
        description: "Path to agent socket".to_string(),
        required: true,
    })
});

pub static SSH_AGENT_CLIENT_RESPONSE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ssh_agent_client_response_received",
        "SSH Agent client received response from agent",
    )
    .with_parameters(vec![
        Parameter {
            name: "response_type".to_string(),
            type_hint: "string".to_string(),
            description: "Type of response (identities, signature, success, failure)".to_string(),
            required: true,
        },
        Parameter {
            name: "response_data".to_string(),
            type_hint: "object".to_string(),
            description: "Response data".to_string(),
            required: true,
        },
    ])
});

/// SSH Agent client protocol implementation
pub struct SshAgentClientProtocol;

impl SshAgentClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for SshAgentClientProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![ParameterDefinition {
            name: "socket_path".to_string(),
            type_hint: "string".to_string(),
            description: "Path to SSH Agent Unix socket (default: $SSH_AUTH_SOCK or ./tmp/ssh-agent.sock)".to_string(),
            required: false,
            example: json!("./tmp/ssh-agent.sock"),
        }]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "modify_instruction".to_string(),
                description: "Modify the LLM instruction for SSH Agent client".to_string(),
                parameters: vec![Parameter {
                    name: "instruction".to_string(),
                    type_hint: "string".to_string(),
                    description: "New LLM instruction".to_string(),
                    required: true,
                }],
                example: json!({"type": "modify_instruction", "instruction": "..."}),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from SSH Agent".to_string(),
                parameters: vec![],
                example: json!({"type": "disconnect"}),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "request_identities".to_string(),
                description: "Request list of identities from agent".to_string(),
                parameters: vec![],
                example: json!({"type": "request_identities"}),
            },
            ActionDefinition {
                name: "sign_request".to_string(),
                description: "Request to sign data with a key".to_string(),
                parameters: vec![
                    Parameter {
                        name: "public_key_blob_hex".to_string(),
                        type_hint: "string".to_string(),
                        description: "Hex-encoded public key blob to sign with".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "data_hex".to_string(),
                        type_hint: "string".to_string(),
                        description: "Hex-encoded data to sign".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "flags".to_string(),
                        type_hint: "integer".to_string(),
                        description: "Signature flags".to_string(),
                        required: false,
                    },
                ],
                example: json!({"type": "sign_request", "public_key_blob_hex": "...", "data_hex": "...", "flags": 0}),
            },
            ActionDefinition {
                name: "add_identity".to_string(),
                description: "Add an identity to the agent".to_string(),
                parameters: vec![
                    Parameter {
                        name: "key_type".to_string(),
                        type_hint: "string".to_string(),
                        description: "SSH key type".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "public_key_blob_hex".to_string(),
                        type_hint: "string".to_string(),
                        description: "Hex-encoded public key blob".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "private_key_blob_hex".to_string(),
                        type_hint: "string".to_string(),
                        description: "Hex-encoded private key blob".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "comment".to_string(),
                        type_hint: "string".to_string(),
                        description: "Key comment".to_string(),
                        required: false,
                    },
                ],
                example: json!({"type": "add_identity", "key_type": "ssh-ed25519", "public_key_blob_hex": "...", "private_key_blob_hex": "...", "comment": "my-key"}),
            },
            ActionDefinition {
                name: "remove_identity".to_string(),
                description: "Remove an identity from the agent".to_string(),
                parameters: vec![Parameter {
                    name: "public_key_blob_hex".to_string(),
                    type_hint: "string".to_string(),
                    description: "Hex-encoded public key blob to remove".to_string(),
                    required: true,
                }],
                example: json!({"type": "remove_identity", "public_key_blob_hex": "..."}),
            },
            ActionDefinition {
                name: "remove_all_identities".to_string(),
                description: "Remove all identities from the agent".to_string(),
                parameters: vec![],
                example: json!({"type": "remove_all_identities"}),
            },
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait for more data".to_string(),
                parameters: vec![],
                example: json!({"type": "wait_for_more"}),
            },
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "SSH Agent"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            (*SSH_AGENT_CLIENT_CONNECTED_EVENT).clone(),
            (*SSH_AGENT_CLIENT_RESPONSE_RECEIVED_EVENT).clone(),
        ]
    }

    fn stack_name(&self) -> &'static str {
        "UNIX Socket > SSH Agent"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["ssh-agent", "agent", "key-agent", "ssh keys"]
    }

    fn metadata(&self) -> ProtocolMetadataV2 {
        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("SSH Agent client using custom protocol implementation")
            .llm_control("Full control over agent operations and key management")
            .e2e_testing("OpenSSH agent, NetGet SSH Agent server")
            .notes("Connects to existing SSH agents via Unix sockets")
            .build()
    }

    fn description(&self) -> &'static str {
        "SSH Agent client for connecting to and managing SSH keys via agents"
    }

    fn example_prompt(&self) -> &'static str {
        "Connect to SSH Agent at $SSH_AUTH_SOCK; list all identities; use first key to sign 'Hello World'"
    }

    fn group_name(&self) -> &'static str {
        "Security"
    }
}

// Implement Client trait (client-specific functionality)
impl Client for SshAgentClientProtocol {
    fn connect(
        &self,
        ctx: ConnectContext,
    ) -> Pin<Box<dyn Future<Output = Result<SocketAddr>> + Send>> {
        Box::pin(async move {
            crate::client::ssh_agent::SshAgentClient::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.client_id,
            )
            .await
        })
    }

    fn execute_action(
        &self,
        action: Value,
    ) -> Result<ClientActionResult> {
        let action_type = action["type"]
            .as_str()
            .context("Missing 'type' field in action")?;

        match action_type {
            "request_identities" => Ok(ClientActionResult::Custom {
                name: "request_identities".to_string(),
                data: json!({}),
            }),
            "sign_request" => {
                let public_key_blob_hex = action["public_key_blob_hex"]
                    .as_str()
                    .context("Missing 'public_key_blob_hex' field")?;
                let data_hex = action["data_hex"]
                    .as_str()
                    .context("Missing 'data_hex' field")?;
                let flags = action["flags"].as_u64().unwrap_or(0) as u32;

                Ok(ClientActionResult::Custom {
                    name: "sign_request".to_string(),
                    data: json!({
                        "public_key_blob_hex": public_key_blob_hex,
                        "data_hex": data_hex,
                        "flags": flags,
                    }),
                })
            }
            "add_identity" => {
                let key_type = action["key_type"]
                    .as_str()
                    .context("Missing 'key_type' field")?;
                let public_key_blob_hex = action["public_key_blob_hex"]
                    .as_str()
                    .context("Missing 'public_key_blob_hex' field")?;
                let private_key_blob_hex = action["private_key_blob_hex"]
                    .as_str()
                    .context("Missing 'private_key_blob_hex' field")?;
                let comment = action["comment"].as_str().unwrap_or("");

                Ok(ClientActionResult::Custom {
                    name: "add_identity".to_string(),
                    data: json!({
                        "key_type": key_type,
                        "public_key_blob_hex": public_key_blob_hex,
                        "private_key_blob_hex": private_key_blob_hex,
                        "comment": comment,
                    }),
                })
            }
            "remove_identity" => {
                let public_key_blob_hex = action["public_key_blob_hex"]
                    .as_str()
                    .context("Missing 'public_key_blob_hex' field")?;

                Ok(ClientActionResult::Custom {
                    name: "remove_identity".to_string(),
                    data: json!({ "public_key_blob_hex": public_key_blob_hex }),
                })
            }
            "remove_all_identities" => Ok(ClientActionResult::Custom {
                name: "remove_all_identities".to_string(),
                data: json!({}),
            }),
            "disconnect" => Ok(ClientActionResult::Disconnect),
            "wait_for_more" => Ok(ClientActionResult::WaitForMore),
            _ => anyhow::bail!("Unknown action type: {}", action_type),
        }
    }
}
