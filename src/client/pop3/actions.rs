use crate::llm::actions::client_trait::{Client, ClientActionResult, ConnectContext};
use crate::llm::actions::{ActionDefinition, ActionParameter};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::Result;
use serde_json::json;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::LazyLock;

/// Event: POP3 client connected to server
pub static POP3_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("pop3_connected", "POP3 client connected to server")
        .with_parameters(vec![("pop3_server", "POP3 server hostname")])
});

/// Event: POP3 response received from server
pub static POP3_CLIENT_RESPONSE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "pop3_response_received",
        "POP3 response received from server",
    )
    .with_parameters(vec![
        ("response", "POP3 server response (e.g., '+OK' or '-ERR')"),
        ("is_ok", "Whether response is +OK (true) or -ERR (false)"),
    ])
});

pub struct Pop3ClientProtocol;

impl Client for Pop3ClientProtocol {
    fn connect(
        &self,
        ctx: ConnectContext,
    ) -> Pin<Box<dyn Future<Output = Result<SocketAddr>> + Send>> {
        Box::pin(async move {
            crate::client::pop3::Pop3Client::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.app_state,
                ctx.status_tx,
                ctx.client_id,
            )
            .await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "modify_pop3_instruction".to_string(),
                description: "Modify the POP3 client instruction".to_string(),
                parameters: vec![ActionParameter {
                    name: "instruction".to_string(),
                    description: "New instruction for the LLM".to_string(),
                    example: json!("Retrieve all messages from the mailbox"),
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

    fn get_sync_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "send_pop3_command".to_string(),
                description: "Send a POP3 command to the server".to_string(),
                parameters: vec![ActionParameter {
                    name: "command".to_string(),
                    description: "POP3 command to send (e.g., 'USER alice', 'PASS secret', 'STAT', 'LIST', 'RETR 1', 'QUIT')".to_string(),
                    example: json!("USER alice"),
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

    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action["type"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing action type"))?;

        match action_type {
            "send_pop3_command" => {
                let command = action["command"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing command parameter"))?
                    .to_string();

                Ok(ClientActionResult::Custom {
                    name: "pop3_command".to_string(),
                    data: json!({ "command": command }),
                })
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            "wait_for_more" => Ok(ClientActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!("Unknown action type: {}", action_type)),
        }
    }

    fn get_event_types(&self) -> Vec<&'static LazyLock<EventType>> {
        vec![
            &POP3_CLIENT_CONNECTED_EVENT,
            &POP3_CLIENT_RESPONSE_RECEIVED_EVENT,
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "pop3"
    }

    fn stack_name(&self) -> &'static str {
        "Application"
    }

    fn get_startup_params(&self) -> Vec<ActionParameter> {
        vec![
            ActionParameter {
                name: "use_tls".to_string(),
                description: "Whether to use TLS/SSL (POP3S). Default: false (plain POP3)"
                    .to_string(),
                example: json!(false),
            },
        ]
    }
}
