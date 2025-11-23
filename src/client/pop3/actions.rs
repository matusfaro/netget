use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    protocol_trait::Protocol,
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// Event: POP3 client connected to server
pub static POP3_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("pop3_connected", "POP3 client connected to server")
        .with_parameters(vec![Parameter {
            name: "pop3_server".to_string(),
            type_hint: "string".to_string(),
            description: "POP3 server hostname".to_string(),
            required: true,
        }])
});

/// Event: POP3 response received from server
pub static POP3_CLIENT_RESPONSE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "pop3_response_received",
        "POP3 response received from server",
    )
    .with_parameters(vec![
        Parameter {
            name: "response".to_string(),
            type_hint: "string".to_string(),
            description: "POP3 server response (e.g., '+OK' or '-ERR')".to_string(),
            required: true,
        },
        Parameter {
            name: "is_ok".to_string(),
            type_hint: "boolean".to_string(),
            description: "Whether response is +OK (true) or -ERR (false)".to_string(),
            required: true,
        },
    ])
});

pub struct Pop3ClientProtocol;

impl Default for Pop3ClientProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl Pop3ClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for Pop3ClientProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![ParameterDefinition {
            name: "use_tls".to_string(),
            description: "Whether to use TLS/SSL (POP3S). Default: false (plain POP3)".to_string(),
            type_hint: "boolean".to_string(),
            required: false,
            example: json!(false),
        }]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "modify_pop3_instruction".to_string(),
                description: "Modify the POP3 client instruction".to_string(),
                parameters: vec![Parameter {
                    name: "instruction".to_string(),
                    type_hint: "string".to_string(),
                    description: "New instruction for the LLM".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "modify_pop3_instruction",
                    "instruction": "Retrieve all messages from the mailbox"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from POP3 server".to_string(),
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
                name: "send_pop3_command".to_string(),
                description: "Send a POP3 command to the server".to_string(),
                parameters: vec![Parameter {
                    name: "command".to_string(),
                    type_hint: "string".to_string(),
                    description:
                        "POP3 command to send (e.g., 'USER alice', 'PASS secret', 'STAT', 'LIST', 'RETR 1', 'QUIT')"
                            .to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "send_pop3_command",
                    "command": "USER alice"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from POP3 server".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "disconnect"
                }),
            },
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait for more data from server".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "wait_for_more"
                }),
            },
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "POP3"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType::new("pop3_connected", "Triggered when POP3 client connects to server"),
            EventType {
                id: "pop3_response_received".to_string(),
                description: "Triggered when POP3 client receives a response from server"
                    .to_string(),
                actions: vec![],
                parameters: vec![],
            },
        ]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>POP3"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["pop3", "pop3 client", "connect to pop3", "pop3s"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation(
                "Custom TCP/TLS client using tokio and rustls for POP3/POP3S email retrieval",
            )
            .llm_control("Full control over POP3 commands (USER, PASS, STAT, LIST, RETR, DELE)")
            .e2e_testing("NetGet POP3 server or local Dovecot server")
            .build()
    }

    fn description(&self) -> &'static str {
        "POP3/POP3S client for retrieving email from mailboxes"
    }

    fn example_prompt(&self) -> &'static str {
        "Connect to pop.gmail.com:995 with TLS and authenticate as user@example.com"
    }

    fn group_name(&self) -> &'static str {
        "Application"
    }
}

// Implement Client trait (client-specific functionality)
impl Client for Pop3ClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            crate::client::pop3::Pop3Client::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.client_id,
            )
            .await
        })
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "send_pop3_command" => {
                let command = action
                    .get("command")
                    .and_then(|v| v.as_str())
                    .context("Missing 'command' parameter")?
                    .to_string();

                Ok(ClientActionResult::Custom {
                    name: "pop3_command".to_string(),
                    data: json!({ "command": command }),
                })
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            "wait_for_more" => Ok(ClientActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!(
                "Unknown POP3 client action: {}",
                action_type
            )),
        }
    }
}
