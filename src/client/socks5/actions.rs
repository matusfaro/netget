//! SOCKS5 client protocol actions implementation

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

/// SOCKS5 client connected event
pub static SOCKS5_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "socks5_connected",
        "SOCKS5 client successfully connected through proxy",
    )
    .with_parameters(vec![
        Parameter {
            name: "proxy_addr".to_string(),
            type_hint: "string".to_string(),
            description: "SOCKS5 proxy server address".to_string(),
            required: true,
        },
        Parameter {
            name: "target_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Target server address through proxy".to_string(),
            required: true,
        },
    ])
});

/// SOCKS5 client data received event
pub static SOCKS5_CLIENT_DATA_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "socks5_data_received",
        "Data received from target server through SOCKS5 proxy",
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

/// SOCKS5 client protocol action handler
pub struct Socks5ClientProtocol;

impl Socks5ClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for Socks5ClientProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "target_addr".to_string(),
                description: "Target server address to connect through SOCKS5 proxy (host:port)"
                    .to_string(),
                type_hint: "string".to_string(),
                required: true,
                example: json!("example.com:80"),
            },
            ParameterDefinition {
                name: "auth_username".to_string(),
                description: "Username for SOCKS5 authentication (optional)".to_string(),
                type_hint: "string".to_string(),
                required: false,
                example: json!("user"),
            },
            ParameterDefinition {
                name: "auth_password".to_string(),
                description: "Password for SOCKS5 authentication (optional)".to_string(),
                type_hint: "string".to_string(),
                required: false,
                example: json!("password"),
            },
        ]
    }
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "send_socks5_data".to_string(),
                description: "Send raw data through SOCKS5 tunnel to target server".to_string(),
                parameters: vec![Parameter {
                    name: "data_hex".to_string(),
                    type_hint: "string".to_string(),
                    description: "Hexadecimal encoded data to send through tunnel".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "send_socks5_data",
                    "data_hex": "48656c6c6f"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from SOCKS5 proxy and target server".to_string(),
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
                name: "send_socks5_data".to_string(),
                description: "Send data in response to received data from target".to_string(),
                parameters: vec![Parameter {
                    name: "data_hex".to_string(),
                    type_hint: "string".to_string(),
                    description: "Hexadecimal encoded data to send".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "send_socks5_data",
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
        "SOCKS5"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType::new("socks5_connected", "Triggered when SOCKS5 client connects through proxy to target"),
            EventType::new("socks5_data_received", "Triggered when SOCKS5 client receives data from target server"),
        ]
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>SOCKS5"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec![
            "socks5",
            "socks",
            "proxy",
            "socks5 client",
            "connect via socks5",
        ]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("tokio-socks library for SOCKS5 protocol")
            .llm_control(
                "Full control over target address, authentication, and data flow through proxy",
            )
            .e2e_testing("Dante or SS5 SOCKS5 server for testing")
            .build()
    }
    fn description(&self) -> &'static str {
        "SOCKS5 client for connecting to servers through a SOCKS5 proxy"
    }
    fn example_prompt(&self) -> &'static str {
        "Connect to example.com:80 through SOCKS5 proxy at localhost:1080"
    }
    fn group_name(&self) -> &'static str {
        "Proxy"
    }
}

// Implement Client trait (client-specific functionality)
impl Client for Socks5ClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::socks5::Socks5Client;
            Socks5Client::connect_with_llm_actions(
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
    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "send_socks5_data" => {
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
                "Unknown SOCKS5 client action: {}",
                action_type
            )),
        }
    }
}
