//! RIP client protocol actions implementation

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

/// RIP client connected event
pub static RIP_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("rip_connected", "RIP client connected to router").with_parameters(vec![
        Parameter {
            name: "remote_addr".to_string(),
            type_hint: "string".to_string(),
            description: "RIP router address".to_string(),
            required: true,
        },
    ])
});

/// RIP client response received event
pub static RIP_CLIENT_RESPONSE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("rip_response_received", "RIP response received from router").with_parameters(
        vec![
            Parameter {
                name: "version".to_string(),
                type_hint: "number".to_string(),
                description: "RIP version (1 or 2)".to_string(),
                required: true,
            },
            Parameter {
                name: "command".to_string(),
                type_hint: "string".to_string(),
                description: "RIP command (request or response)".to_string(),
                required: true,
            },
            Parameter {
                name: "route_count".to_string(),
                type_hint: "number".to_string(),
                description: "Number of routes in response".to_string(),
                required: true,
            },
            Parameter {
                name: "routes".to_string(),
                type_hint: "array".to_string(),
                description:
                    "Array of route entries with ip_address, subnet_mask, next_hop, and metric"
                        .to_string(),
                required: true,
            },
        ],
    )
});

/// RIP client protocol action handler
pub struct RipClientProtocol;

impl Default for RipClientProtocol {
    fn default() -> Self {
        Self
    }
}

impl RipClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for RipClientProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "send_rip_request".to_string(),
                description: "Send RIP request to query routing table".to_string(),
                parameters: vec![Parameter {
                    name: "version".to_string(),
                    type_hint: "number".to_string(),
                    description: "RIP version (1 or 2)".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "send_rip_request",
                    "version": 2
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Close RIP client connection".to_string(),
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
                name: "send_rip_request".to_string(),
                description: "Send RIP request in response to received data".to_string(),
                parameters: vec![Parameter {
                    name: "version".to_string(),
                    type_hint: "number".to_string(),
                    description: "RIP version (1 or 2)".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "send_rip_request",
                    "version": 2
                }),
            },
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait for more responses before responding".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "wait_for_more"
                }),
            },
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "RIP"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType::new("rip_connected", "Triggered when RIP client connects to router"),
            EventType {
                id: "rip_response_received".to_string(),
                description: "Triggered when RIP client receives routing table response"
                    .to_string(),
                actions: vec![],
                parameters: vec![],
            },
        ]
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>RIP"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["rip", "rip client", "routing information protocol"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("UDP socket with RIPv1/v2 packet parsing")
            .llm_control("Query routing tables, analyze routes")
            .e2e_testing("Mock RIP router or real router in test network")
            .build()
    }
    fn description(&self) -> &'static str {
        "RIP client for querying routing tables from RIP routers"
    }
    fn example_prompt(&self) -> &'static str {
        "Connect to RIP router at 192.168.1.1:520 and query routing table"
    }
    fn group_name(&self) -> &'static str {
        "Routing"
    }
}

// Implement Client trait (client-specific functionality)
impl Client for RipClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::rip::RipClient;
            RipClient::connect_with_llm_actions(
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
            "send_rip_request" => {
                let version = action
                    .get("version")
                    .and_then(|v| v.as_u64())
                    .context("Missing 'version' field")?;

                Ok(ClientActionResult::Custom {
                    name: "send_rip_request".to_string(),
                    data: json!({
                        "version": version
                    }),
                })
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            "wait_for_more" => Ok(ClientActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!(
                "Unknown RIP client action: {}",
                action_type
            )),
        }
    }
}
