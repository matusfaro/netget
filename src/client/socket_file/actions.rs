//! Socket File client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    protocol_trait::Protocol,
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// Socket File client connected event
pub static SOCKET_FILE_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "socket_file_connected",
        "Socket File client successfully connected to Unix domain socket",
        json!({
            "type": "send_socket_file_data",
            "data_hex": "48656c6c6f"
        })
    )
    .with_parameters(vec![Parameter {
        name: "socket_path".to_string(),
        type_hint: "string".to_string(),
        description: "Unix domain socket path".to_string(),
        required: true,
    }])
});

/// Socket File client data received event
pub static SOCKET_FILE_CLIENT_DATA_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "socket_file_data_received",
        "Data received from Unix domain socket",
        json!({
            "type": "send_socket_file_data",
            "data_hex": "48656c6c6f"
        })
    )
    .with_parameters(vec![
        Parameter {
            name: "data_hex".to_string(),
            type_hint: "string".to_string(),
            description: "The data received (as hex string)".to_string(),
            required: true,
        },
        Parameter {
            name: "data_length".to_string(),
            type_hint: "number".to_string(),
            description: "Length of data in bytes".to_string(),
            required: true,
        },
    ])
});

/// Socket File client protocol action handler
pub struct SocketFileClientProtocol;

impl SocketFileClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for SocketFileClientProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "send_socket_file_data".to_string(),
                description: "Send raw data to the Unix domain socket".to_string(),
                parameters: vec![Parameter {
                    name: "data_hex".to_string(),
                    type_hint: "string".to_string(),
                    description: "Hexadecimal encoded data to send".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "send_socket_file_data",
                    "data_hex": "48656c6c6f"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from the Unix domain socket".to_string(),
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
                name: "send_socket_file_data".to_string(),
                description: "Send data in response to received data".to_string(),
                parameters: vec![Parameter {
                    name: "data_hex".to_string(),
                    type_hint: "string".to_string(),
                    description: "Hexadecimal encoded data to send".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "send_socket_file_data",
                    "data_hex": "48656c6c6f"
                }),
            },
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait for more data before responding".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "wait_for_more"
                }),
            },
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "SocketFile"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType::new("socket_file_connected", "Triggered when Socket File client connects to Unix domain socket", json!({"type": "placeholder", "event_id": "socket_file_connected"})),
            EventType::new("socket_file_data_received", "Triggered when Socket File client receives data from Unix domain socket", json!({"type": "placeholder", "event_id": "socket_file_data_received"})),
        ]
    }
    fn stack_name(&self) -> &'static str {
        "UnixSocket"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec![
            "socket file",
            "unix socket",
            "domain socket",
            "socket-file",
            "socketfile",
        ]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Tokio UnixStream for Unix domain socket connections")
            .llm_control("Full control over sent/received bytes")
            .e2e_testing("socat/nc for Unix socket testing")
            .build()
    }
    fn description(&self) -> &'static str {
        "Socket File client for connecting to Unix domain sockets"
    }
    fn example_prompt(&self) -> &'static str {
        "Connect to socket file at ./app.sock and send 'HELLO'"
    }
    fn group_name(&self) -> &'static str {
        "Core"
    }
}

// Implement Client trait (client-specific functionality)
impl Client for SocketFileClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::socket_file::SocketFileClient;
            SocketFileClient::connect_with_llm_actions(
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
            "send_socket_file_data" => {
                let data_hex = action
                    .get("data_hex")
                    .and_then(|v| v.as_str())
                    .context("Missing 'data_hex' field")?;

                let data = hex::decode(data_hex).context("Invalid hex data")?;

                Ok(ClientActionResult::SendData(data))
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            "wait_for_more" => Ok(ClientActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!(
                "Unknown Socket File client action: {}",
                action_type
            )),
        }
    }
}
