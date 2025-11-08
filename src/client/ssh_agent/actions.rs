//! SSH Agent client protocol actions

use crate::llm::actions::client_trait::{Client, ClientActionResult, ConnectContext};
use crate::protocol::{EventType, ParameterInfo, ParameterType};
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
    .with_parameters(vec![ParameterInfo::new(
        "socket_path",
        ParameterType::String,
        "Path to agent socket",
    )])
});

pub static SSH_AGENT_CLIENT_RESPONSE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ssh_agent_client_response_received",
        "SSH Agent client received response from agent",
    )
    .with_parameters(vec![
        ParameterInfo::new(
            "response_type",
            ParameterType::String,
            "Type of response (identities, signature, success, failure)",
        ),
        ParameterInfo::new("response_data", ParameterType::Object, "Response data"),
    ])
});

/// SSH Agent client protocol implementation
pub struct SshAgentClientProtocol;

impl SshAgentClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Client for SshAgentClientProtocol {
    fn connect(
        &self,
        ctx: ConnectContext,
    ) -> Pin<Box<dyn Future<Output = Result<SocketAddr>> + Send>> {
        Box::pin(async move {
            crate::client::ssh_agent::SshAgentClient::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.app_state,
                ctx.status_tx,
                ctx.client_id,
            )
            .await
        })
    }

    fn get_async_actions(&self) -> Vec<Value> {
        vec![
            json!({
                "type": "modify_instruction",
                "instruction": {
                    "type": "string",
                    "description": "New LLM instruction for SSH Agent client"
                }
            }),
            json!({
                "type": "disconnect",
                "description": "Disconnect from SSH Agent"
            }),
        ]
    }

    fn get_sync_actions(&self) -> Vec<Value> {
        vec![
            json!({
                "type": "request_identities",
                "description": "Request list of identities from agent (SSH_AGENTC_REQUEST_IDENTITIES)"
            }),
            json!({
                "type": "sign_request",
                "public_key_blob_hex": {
                    "type": "string",
                    "description": "Hex-encoded public key blob to sign with"
                },
                "data_hex": {
                    "type": "string",
                    "description": "Hex-encoded data to sign"
                },
                "flags": {
                    "type": "integer",
                    "description": "Signature flags (0 for default, 4 for RSA-SHA256)"
                }
            }),
            json!({
                "type": "add_identity",
                "key_type": {
                    "type": "string",
                    "description": "SSH key type (e.g., ssh-ed25519, ssh-rsa)"
                },
                "public_key_blob_hex": {
                    "type": "string",
                    "description": "Hex-encoded public key blob"
                },
                "private_key_blob_hex": {
                    "type": "string",
                    "description": "Hex-encoded private key blob"
                },
                "comment": {
                    "type": "string",
                    "description": "Key comment/description"
                }
            }),
            json!({
                "type": "remove_identity",
                "public_key_blob_hex": {
                    "type": "string",
                    "description": "Hex-encoded public key blob to remove"
                }
            }),
            json!({
                "type": "remove_all_identities",
                "description": "Remove all identities from agent"
            }),
            json!({
                "type": "wait_for_more",
                "description": "Wait for more data"
            }),
        ]
    }

    fn execute_action(&self, action: Value) -> Result<ClientActionResult> {
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
            "modify_instruction" => {
                let instruction = action["instruction"]
                    .as_str()
                    .context("Missing 'instruction' field")?
                    .to_string();
                Ok(ClientActionResult::ModifyInstruction(instruction))
            }
            _ => anyhow::bail!("Unknown action type: {}", action_type),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "SSH Agent"
    }

    fn stack_name(&self) -> &'static str {
        "Application"
    }

    fn get_event_types(&self) -> Vec<&'static EventType> {
        vec![
            &SSH_AGENT_CLIENT_CONNECTED_EVENT,
            &SSH_AGENT_CLIENT_RESPONSE_RECEIVED_EVENT,
        ]
    }

    fn get_startup_params(&self) -> Vec<crate::llm::actions::StartupParam> {
        vec![crate::llm::actions::StartupParam {
            name: "socket_path".to_string(),
            description: "Path to SSH Agent Unix socket (default: $SSH_AUTH_SOCK or /tmp/ssh-agent.sock)".to_string(),
            required: false,
        }]
    }
}
