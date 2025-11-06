//! TCP client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// TCP client connected event
pub static TCP_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "tcp_connected",
        "TCP client successfully connected to server"
    )
    .with_parameters(vec![
        Parameter {
            name: "remote_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Remote server address".to_string(),
            required: true,
        },
    ])
});

/// TCP client data received event
pub static TCP_CLIENT_DATA_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "tcp_data_received",
        "Data received from TCP server"
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

/// TCP client protocol action handler
pub struct TcpClientProtocol;

impl TcpClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Client for TcpClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::tcp::TcpClient;
            TcpClient::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.client_id,
            )
            .await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "send_tcp_data".to_string(),
                description: "Send raw TCP data to the server".to_string(),
                parameters: vec![
                    Parameter {
                        name: "data_hex".to_string(),
                        type_hint: "string".to_string(),
                        description: "Hexadecimal encoded data to send".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "send_tcp_data",
                    "data_hex": "48656c6c6f"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from the TCP server".to_string(),
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
                name: "send_tcp_data".to_string(),
                description: "Send TCP data in response to received data".to_string(),
                parameters: vec![
                    Parameter {
                        name: "data_hex".to_string(),
                        type_hint: "string".to_string(),
                        description: "Hexadecimal encoded data to send".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "send_tcp_data",
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

    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "send_tcp_data" => {
                let data_hex = action
                    .get("data_hex")
                    .and_then(|v| v.as_str())
                    .context("Missing 'data_hex' field")?;

                let data = hex::decode(data_hex)
                    .context("Invalid hex data")?;

                Ok(ClientActionResult::SendData(data))
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            "wait_for_more" => Ok(ClientActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!("Unknown TCP client action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "TCP"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType {
                id: "tcp_connected".to_string(),
                description: "Triggered when TCP client connects to server".to_string(),
                actions: vec![],
                parameters: vec![],
            },
            EventType {
                id: "tcp_data_received".to_string(),
                description: "Triggered when TCP client receives data from server".to_string(),
                actions: vec![],
                parameters: vec![],
            },
        ]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["tcp", "tcp client", "connect to tcp"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Tokio TcpStream for raw TCP connections")
            .llm_control("Full control over sent/received bytes")
            .e2e_testing("nc (netcat) as test server")
            .build()
    }

    fn description(&self) -> &'static str {
        "TCP client for connecting to TCP servers"
    }

    fn example_prompt(&self) -> &'static str {
        "Connect to TCP at localhost:8080 and send 'HELLO'"
    }

    fn group_name(&self) -> &'static str {
        "Core"
    }
}
