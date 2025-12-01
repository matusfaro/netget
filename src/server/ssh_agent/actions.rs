//! SSH Agent server protocol actions

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::Result;
use serde_json::json;
use std::sync::LazyLock;

// Event type constants
pub static SSH_AGENT_CONNECTION_OPENED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ssh_agent_connection_opened",
        "SSH Agent connection opened (client connected to agent socket)",
        json!({
            "type": "send_success"
        })
    )
    .with_parameter(Parameter {
        name: "connection_id".to_string(),
        type_hint: "string".to_string(),
        description: "Unique connection identifier".to_string(),
        required: true,
    })
});

pub static SSH_AGENT_REQUEST_IDENTITIES_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ssh_agent_request_identities",
        "Client requested list of available SSH keys",
        json!({
            "type": "send_identities_list",
            "identities": []
        })
    )
    .with_parameter(Parameter {
        name: "connection_id".to_string(),
        type_hint: "string".to_string(),
        description: "Connection that requested identities".to_string(),
        required: true,
    })
});

pub static SSH_AGENT_SIGN_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ssh_agent_sign_request",
        "Client requested to sign data with a key",
        json!({
            "type": "send_sign_response",
            "signature_hex": "0000000b7373682d656432353531390000004000..."
        })
    )
    .with_parameters(vec![
        Parameter {
            name: "connection_id".to_string(),
            type_hint: "string".to_string(),
            description: "Connection identifier".to_string(),
            required: true,
        },
        Parameter {
            name: "public_key_blob_hex".to_string(),
            type_hint: "string".to_string(),
            description: "Hex-encoded public key blob".to_string(),
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
            required: true,
        },
    ])
});

pub static SSH_AGENT_ADD_IDENTITY_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ssh_agent_add_identity",
        "Client requested to add a key to the agent",
        json!({
            "type": "send_success"
        })
    )
    .with_parameters(vec![
        Parameter {
            name: "connection_id".to_string(),
            type_hint: "string".to_string(),
            description: "Connection identifier".to_string(),
            required: true,
        },
        Parameter {
            name: "key_type".to_string(),
            type_hint: "string".to_string(),
            description: "SSH key type (e.g., ssh-ed25519)".to_string(),
            required: true,
        },
        Parameter {
            name: "public_key_blob_hex".to_string(),
            type_hint: "string".to_string(),
            description: "Hex-encoded public key blob".to_string(),
            required: true,
        },
        Parameter {
            name: "comment".to_string(),
            type_hint: "string".to_string(),
            description: "Key comment/description".to_string(),
            required: true,
        },
        Parameter {
            name: "constrained".to_string(),
            type_hint: "boolean".to_string(),
            description: "Whether key has constraints".to_string(),
            required: true,
        },
    ])
});

pub static SSH_AGENT_REMOVE_IDENTITY_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ssh_agent_remove_identity",
        "Client requested to remove a key from the agent",
        json!({
            "type": "send_success"
        })
    )
    .with_parameters(vec![
        Parameter {
            name: "connection_id".to_string(),
            type_hint: "string".to_string(),
            description: "Connection identifier".to_string(),
            required: true,
        },
        Parameter {
            name: "public_key_blob_hex".to_string(),
            type_hint: "string".to_string(),
            description: "Hex-encoded public key blob to remove".to_string(),
            required: true,
        },
    ])
});

pub static SSH_AGENT_REMOVE_ALL_IDENTITIES_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ssh_agent_remove_all_identities",
        "Client requested to remove all keys from the agent",
        json!({
            "type": "send_success"
        })
    )
    .with_parameter(Parameter {
        name: "connection_id".to_string(),
        type_hint: "string".to_string(),
        description: "Connection identifier".to_string(),
        required: true,
    })
});

pub static SSH_AGENT_LOCK_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ssh_agent_lock",
        "Client requested to lock the agent with a passphrase",
        json!({
            "type": "send_success"
        })
    )
    .with_parameter(Parameter {
        name: "connection_id".to_string(),
        type_hint: "string".to_string(),
        description: "Connection identifier".to_string(),
        required: true,
    })
});

pub static SSH_AGENT_UNLOCK_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ssh_agent_unlock",
        "Client requested to unlock the agent with a passphrase",
        json!({
            "type": "send_success"
        })
    )
    .with_parameter(Parameter {
        name: "connection_id".to_string(),
        type_hint: "string".to_string(),
        description: "Connection identifier".to_string(),
        required: true,
    })
});

/// SSH Agent server protocol implementation
pub struct SshAgentProtocol;

impl SshAgentProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for SshAgentProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![ParameterDefinition {
            name: "socket_path".to_string(),
            type_hint: "string".to_string(),
            description: "Path to Unix domain socket (default: ./netget-ssh-agent.sock)"
                .to_string(),
            required: false,
            example: json!("./netget-ssh-agent.sock"),
        }]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "modify_instruction".to_string(),
                description: "Modify the LLM instruction for handling SSH Agent operations"
                    .to_string(),
                parameters: vec![Parameter {
                    name: "instruction".to_string(),
                    type_hint: "string".to_string(),
                    description: "New LLM instruction".to_string(),
                    required: true,
                }],
                example: json!({"type": "modify_instruction", "instruction": "..."}),
            },
            ActionDefinition {
                name: "close_connection".to_string(),
                description: "Close a specific SSH Agent connection".to_string(),
                parameters: vec![Parameter {
                    name: "connection_id".to_string(),
                    type_hint: "string".to_string(),
                    description: "Connection ID to close".to_string(),
                    required: true,
                }],
                example: json!({"type": "close_connection", "connection_id": "conn-123"}),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "send_identities_list".to_string(),
                description: "Send list of SSH identities to client".to_string(),
                parameters: vec![Parameter {
                    name: "identities".to_string(),
                    type_hint: "array".to_string(),
                    description: "Array of identity objects".to_string(),
                    required: true,
                }],
                example: json!({"type": "send_identities_list", "identities": [{"key_type": "ssh-ed25519", "public_key_blob_hex": "...", "comment": "my-key"}]}),
            },
            ActionDefinition {
                name: "send_sign_response".to_string(),
                description: "Send signature response to client".to_string(),
                parameters: vec![Parameter {
                    name: "signature_hex".to_string(),
                    type_hint: "string".to_string(),
                    description: "Hex-encoded signature blob".to_string(),
                    required: true,
                }],
                example: json!({"type": "send_sign_response", "signature_hex": "..."}),
            },
            ActionDefinition {
                name: "send_success".to_string(),
                description: "Send SSH_AGENT_SUCCESS response".to_string(),
                parameters: vec![],
                example: json!({"type": "send_success"}),
            },
            ActionDefinition {
                name: "send_failure".to_string(),
                description: "Send SSH_AGENT_FAILURE response".to_string(),
                parameters: vec![],
                example: json!({"type": "send_failure"}),
            },
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait for more data before making a decision".to_string(),
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
            (*SSH_AGENT_CONNECTION_OPENED_EVENT).clone(),
            (*SSH_AGENT_REQUEST_IDENTITIES_EVENT).clone(),
            (*SSH_AGENT_SIGN_REQUEST_EVENT).clone(),
            (*SSH_AGENT_ADD_IDENTITY_EVENT).clone(),
            (*SSH_AGENT_REMOVE_IDENTITY_EVENT).clone(),
            (*SSH_AGENT_REMOVE_ALL_IDENTITIES_EVENT).clone(),
            (*SSH_AGENT_LOCK_EVENT).clone(),
            (*SSH_AGENT_UNLOCK_EVENT).clone(),
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
            .implementation("Custom SSH Agent protocol parser with Unix domain sockets")
            .llm_control("Full control over key management, signing operations, and access control")
            .e2e_testing("ssh-add, ssh-keygen, OpenSSH agent clients")
            .notes("Virtual agent - keys stored in LLM memory, not persistent")
            .build()
    }

    fn description(&self) -> &'static str {
        "SSH Agent protocol server for managing SSH keys and signing operations"
    }

    fn example_prompt(&self) -> &'static str {
        "Start SSH Agent on ./netget-ssh-agent.sock; provide 2 Ed25519 keys (admin-key, deploy-key); sign any requests automatically"
    }

    fn group_name(&self) -> &'static str {
        "Security"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        use serde_json::json;

        StartupExamples::new(
            // LLM mode: LLM handles all SSH Agent responses intelligently
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "ssh-agent",
                "instruction": "SSH Agent managing keys and signing operations"
            }),
            // Script mode: Code-based deterministic responses
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "ssh-agent",
                "event_handlers": [{
                    "event_pattern": "ssh_agent_request_identities",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<ssh_agent_handler>"
                    }
                }]
            }),
            // Static mode: Fixed responses
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "ssh-agent",
                "event_handlers": [{
                    "event_pattern": "ssh_agent_request_identities",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "send_identities_list",
                            "identities": []
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for SshAgentProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>>
    {
        Box::pin(async move {
            // For Unix sockets, we need a path not a SocketAddr
            // Extract socket_path from startup_params or use default
            let socket_path = ctx
                .startup_params
                .as_ref()
                .and_then(|p| p.get_optional_string("socket_path"))
                .unwrap_or_else(|| "./netget-ssh-agent.sock".to_string());

            let socket_path_buf = std::path::PathBuf::from(socket_path);

            use crate::server::ssh_agent::SshAgentServer;
            let _actual_path = SshAgentServer::spawn_with_llm_actions(
                socket_path_buf,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            )
            .await?;

            // Return a dummy SocketAddr since Unix sockets don't have IP addresses
            Ok("127.0.0.1:0".parse().unwrap())
        })
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action["type"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'type' field in action"))?;

        match action_type {
            "send_identities_list" => {
                let identities = action["identities"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("Missing or invalid 'identities' field"))?;

                Ok(ActionResult::Custom {
                    name: "send_identities_list".to_string(),
                    data: json!({ "identities": identities }),
                })
            }
            "send_sign_response" => {
                let signature_hex = action["signature_hex"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'signature_hex' field"))?;

                Ok(ActionResult::Custom {
                    name: "send_sign_response".to_string(),
                    data: json!({ "signature_hex": signature_hex }),
                })
            }
            "send_success" => Ok(ActionResult::Custom {
                name: "send_success".to_string(),
                data: json!({}),
            }),
            "send_failure" => Ok(ActionResult::Custom {
                name: "send_failure".to_string(),
                data: json!({}),
            }),
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            "modify_instruction" => {
                let instruction = action["instruction"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'instruction' field"))?
                    .to_string();
                // ModifyInstruction is handled as a Custom action
                Ok(ActionResult::Custom {
                    name: "modify_instruction".to_string(),
                    data: json!({ "instruction": instruction }),
                })
            }
            "close_connection" => {
                // CloseConnection is a unit variant
                Ok(ActionResult::CloseConnection)
            }
            _ => anyhow::bail!("Unknown action type: {}", action_type),
        }
    }
}
