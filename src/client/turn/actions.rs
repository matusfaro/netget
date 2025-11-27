//! TURN client protocol actions implementation

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

/// TURN client connected event
pub static TURN_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "turn_connected",
        "TURN client successfully connected to server",
        json!({
            "type": "allocate_turn_relay",
            "lifetime_seconds": 600
        }),
    )
    .with_parameters(vec![Parameter {
        name: "remote_addr".to_string(),
        type_hint: "string".to_string(),
        description: "TURN server address".to_string(),
        required: true,
    }])
});

/// TURN client allocation success event
pub static TURN_CLIENT_ALLOCATED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "turn_allocated",
        "TURN relay address allocated successfully",
        json!({
            "type": "create_permission",
            "peer_address": "192.168.1.100:5000"
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "relay_address".to_string(),
            type_hint: "string".to_string(),
            description: "The allocated relay address (IP:port)".to_string(),
            required: true,
        },
        Parameter {
            name: "lifetime_seconds".to_string(),
            type_hint: "number".to_string(),
            description: "Allocation lifetime in seconds".to_string(),
            required: true,
        },
        Parameter {
            name: "transaction_id".to_string(),
            type_hint: "string".to_string(),
            description: "Transaction ID (hex)".to_string(),
            required: true,
        },
    ])
});

/// TURN client data received event
pub static TURN_CLIENT_DATA_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "turn_data_received",
        "Data received from peer via TURN relay",
        json!({
            "type": "send_turn_data",
            "peer_address": "192.168.1.100:5000",
            "data_hex": "48656c6c6f"
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "peer_address".to_string(),
            type_hint: "string".to_string(),
            description: "Peer address that sent the data".to_string(),
            required: true,
        },
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

/// TURN client permission created event
pub static TURN_CLIENT_PERMISSION_CREATED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "turn_permission_created",
        "Permission created for peer address",
        json!({
            "type": "wait_for_more"
        }),
    )
    .with_parameters(vec![Parameter {
        name: "peer_address".to_string(),
        type_hint: "string".to_string(),
        description: "Peer address granted permission".to_string(),
        required: true,
    }])
});

/// TURN client allocation refreshed event
pub static TURN_CLIENT_REFRESHED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("turn_refreshed", "TURN allocation lifetime extended", json!({"type": "placeholder", "event_id": "turn_refreshed"})).with_parameters(vec![
        Parameter {
            name: "lifetime_seconds".to_string(),
            type_hint: "number".to_string(),
            description: "New lifetime in seconds".to_string(),
            required: true,
        },
    ])
});

/// TURN client protocol action handler
pub struct TurnClientProtocol;

impl TurnClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for TurnClientProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "allocate_turn_relay".to_string(),
                description: "Request a relay address allocation from TURN server".to_string(),
                parameters: vec![Parameter {
                    name: "lifetime_seconds".to_string(),
                    type_hint: "number".to_string(),
                    description: "Requested lifetime in seconds (default: 600)".to_string(),
                    required: false,
                }],
                example: json!({
                    "type": "allocate_turn_relay",
                    "lifetime_seconds": 600
                }),
            },
            ActionDefinition {
                name: "create_permission".to_string(),
                description: "Create permission for a peer address to send/receive data"
                    .to_string(),
                parameters: vec![Parameter {
                    name: "peer_address".to_string(),
                    type_hint: "string".to_string(),
                    description: "Peer IP:port to grant permission (e.g., '192.168.1.100:5000')"
                        .to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "create_permission",
                    "peer_address": "192.168.1.100:5000"
                }),
            },
            ActionDefinition {
                name: "send_turn_data".to_string(),
                description: "Send data to peer via TURN relay".to_string(),
                parameters: vec![
                    Parameter {
                        name: "peer_address".to_string(),
                        type_hint: "string".to_string(),
                        description: "Peer IP:port to send data to".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "data_hex".to_string(),
                        type_hint: "string".to_string(),
                        description: "Data to send (as hex string)".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "send_turn_data",
                    "peer_address": "192.168.1.100:5000",
                    "data_hex": "48656c6c6f"
                }),
            },
            ActionDefinition {
                name: "refresh_allocation".to_string(),
                description: "Refresh TURN allocation to extend lifetime".to_string(),
                parameters: vec![Parameter {
                    name: "lifetime_seconds".to_string(),
                    type_hint: "number".to_string(),
                    description: "New lifetime in seconds (0 to delete allocation)".to_string(),
                    required: false,
                }],
                example: json!({
                    "type": "refresh_allocation",
                    "lifetime_seconds": 600
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from TURN server".to_string(),
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
                name: "send_turn_data".to_string(),
                description: "Send data to peer via TURN relay in response to received data"
                    .to_string(),
                parameters: vec![
                    Parameter {
                        name: "peer_address".to_string(),
                        type_hint: "string".to_string(),
                        description: "Peer IP:port to send data to".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "data_hex".to_string(),
                        type_hint: "string".to_string(),
                        description: "Data to send (as hex string)".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "send_turn_data",
                    "peer_address": "192.168.1.100:5000",
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
        "TURN"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType::new("turn_connected", "Triggered when TURN client connects to server", json!({"type": "placeholder", "event_id": "turn_connected"})),
            EventType::new("turn_allocated", "Triggered when relay address is allocated", json!({"type": "placeholder", "event_id": "turn_allocated"})),
            EventType::new("turn_data_received", "Triggered when data is received from peer via relay", json!({"type": "placeholder", "event_id": "turn_data_received"})),
            EventType::new("turn_permission_created", "Triggered when permission is created for a peer", json!({"type": "placeholder", "event_id": "turn_permission_created"})),
            EventType::new("turn_refreshed", "Triggered when allocation is refreshed", json!({"type": "placeholder", "event_id": "turn_refreshed"})),
        ]
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>STUN/TURN"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["turn", "turn client", "relay", "nat traversal", "webrtc"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("webrtc-turn library for TURN/STUN protocol")
            .llm_control("Full control over allocations, permissions, and relay data")
            .e2e_testing("NetGet TURN server as test server")
            .build()
    }
    fn description(&self) -> &'static str {
        "TURN client for NAT traversal relay"
    }
    fn example_prompt(&self) -> &'static str {
        "Connect to TURN server at localhost:3478 and allocate a relay address"
    }
    fn group_name(&self) -> &'static str {
        "Network Infrastructure"
    }
}

// Implement Client trait (client-specific functionality)
impl Client for TurnClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::turn::TurnClient;
            TurnClient::connect_with_llm_actions(
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
            "allocate_turn_relay" => {
                let lifetime = action
                    .get("lifetime_seconds")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(600);

                Ok(ClientActionResult::Custom {
                    name: "allocate".to_string(),
                    data: json!({
                        "lifetime_seconds": lifetime
                    }),
                })
            }
            "create_permission" => {
                let peer_address = action
                    .get("peer_address")
                    .and_then(|v| v.as_str())
                    .context("Missing 'peer_address' field")?;

                Ok(ClientActionResult::Custom {
                    name: "create_permission".to_string(),
                    data: json!({
                        "peer_address": peer_address
                    }),
                })
            }
            "send_turn_data" => {
                let peer_address = action
                    .get("peer_address")
                    .and_then(|v| v.as_str())
                    .context("Missing 'peer_address' field")?;

                let data_hex = action
                    .get("data_hex")
                    .and_then(|v| v.as_str())
                    .context("Missing 'data_hex' field")?;

                let data = hex::decode(data_hex).context("Invalid hex data")?;

                Ok(ClientActionResult::Custom {
                    name: "send_indication".to_string(),
                    data: json!({
                        "peer_address": peer_address,
                        "data": data
                    }),
                })
            }
            "refresh_allocation" => {
                let lifetime = action
                    .get("lifetime_seconds")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(600);

                Ok(ClientActionResult::Custom {
                    name: "refresh".to_string(),
                    data: json!({
                        "lifetime_seconds": lifetime
                    }),
                })
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            "wait_for_more" => Ok(ClientActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!(
                "Unknown TURN client action: {}",
                action_type
            )),
        }
    }
}
