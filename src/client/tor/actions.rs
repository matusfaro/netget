//! Tor client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// Tor client connected event
pub static TOR_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "tor_connected",
        "Tor client successfully connected through Tor network"
    )
    .with_parameters(vec![
        Parameter {
            name: "target".to_string(),
            type_hint: "string".to_string(),
            description: "Target address (can be regular hostname:port or .onion:port)".to_string(),
            required: true,
        },
    ])
});

/// Tor client data received event
pub static TOR_CLIENT_DATA_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "tor_data_received",
        "Data received from destination through Tor"
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

/// Tor client protocol action handler
pub struct TorClientProtocol;

impl TorClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Client for TorClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::tor::TorClient;
            TorClient::connect_with_llm_actions(
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
                name: "send_tor_data".to_string(),
                description: "Send raw data to the destination through Tor".to_string(),
                parameters: vec![
                    Parameter {
                        name: "data_hex".to_string(),
                        type_hint: "string".to_string(),
                        description: "Hexadecimal encoded data to send".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "send_tor_data",
                    "data_hex": "48656c6c6f"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from the Tor circuit".to_string(),
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
                name: "send_tor_data".to_string(),
                description: "Send data in response to received data".to_string(),
                parameters: vec![
                    Parameter {
                        name: "data_hex".to_string(),
                        type_hint: "string".to_string(),
                        description: "Hexadecimal encoded data to send".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "send_tor_data",
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
            "send_tor_data" => {
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
            _ => Err(anyhow::anyhow!("Unknown Tor client action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "Tor"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType {
                id: "tor_connected".to_string(),
                description: "Triggered when Tor client connects through Tor network".to_string(),
                actions: vec![],
                parameters: vec![],
            },
            EventType {
                id: "tor_data_received".to_string(),
                description: "Triggered when Tor client receives data from destination".to_string(),
                actions: vec![],
                parameters: vec![],
            },
        ]
    }

    fn stack_name(&self) -> &'static str {
        "Tor>TCP"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["tor", "tor client", "onion", "anonymous", "privacy"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Arti Tor client (pure Rust Tor implementation)")
            .llm_control("Full control over data sent/received through Tor circuits")
            .e2e_testing("Connect to onion services or regular hosts through Tor")
            .build()
    }

    fn description(&self) -> &'static str {
        "Tor client for anonymous connections through the Tor network"
    }

    fn example_prompt(&self) -> &'static str {
        "Connect to example.onion:80 through Tor and send HTTP GET request"
    }

    fn group_name(&self) -> &'static str {
        "VPN & Tunneling"
    }
}
