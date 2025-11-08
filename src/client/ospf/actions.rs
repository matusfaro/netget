//! OSPF client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    protocol_trait::Protocol,
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// OSPF client connected event
pub static OSPF_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ospf_client_connected",
        "OSPF client successfully joined multicast group and ready to query routers"
    )
    .with_parameters(vec![
        Parameter {
            name: "interface_ip".to_string(),
            type_hint: "string".to_string(),
            description: "Interface IP address".to_string(),
            required: true,
        },
        Parameter {
            name: "router_id".to_string(),
            type_hint: "string".to_string(),
            description: "Client's OSPF router ID".to_string(),
            required: true,
        },
    ])
});

/// OSPF Hello packet received event
pub static OSPF_CLIENT_HELLO_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ospf_hello_received",
        "OSPF Hello packet received from router"
    )
    .with_parameters(vec![
        Parameter {
            name: "neighbor_id".to_string(),
            type_hint: "string".to_string(),
            description: "Neighbor router ID".to_string(),
            required: true,
        },
        Parameter {
            name: "neighbor_ip".to_string(),
            type_hint: "string".to_string(),
            description: "Neighbor IP address".to_string(),
            required: true,
        },
        Parameter {
            name: "area_id".to_string(),
            type_hint: "string".to_string(),
            description: "OSPF area ID".to_string(),
            required: true,
        },
        Parameter {
            name: "network_mask".to_string(),
            type_hint: "string".to_string(),
            description: "Network mask".to_string(),
            required: true,
        },
        Parameter {
            name: "hello_interval".to_string(),
            type_hint: "number".to_string(),
            description: "Hello interval in seconds".to_string(),
            required: true,
        },
        Parameter {
            name: "router_dead_interval".to_string(),
            type_hint: "number".to_string(),
            description: "Router dead interval in seconds".to_string(),
            required: true,
        },
        Parameter {
            name: "router_priority".to_string(),
            type_hint: "number".to_string(),
            description: "Router priority for DR election".to_string(),
            required: true,
        },
        Parameter {
            name: "dr".to_string(),
            type_hint: "string".to_string(),
            description: "Designated router IP".to_string(),
            required: true,
        },
        Parameter {
            name: "bdr".to_string(),
            type_hint: "string".to_string(),
            description: "Backup designated router IP".to_string(),
            required: true,
        },
        Parameter {
            name: "neighbors".to_string(),
            type_hint: "array".to_string(),
            description: "List of neighbor router IDs".to_string(),
            required: true,
        },
    ])
});

/// OSPF Database Description packet received event
pub static OSPF_CLIENT_DD_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ospf_database_description_received",
        "OSPF Database Description packet received"
    )
    .with_parameters(vec![
        Parameter {
            name: "neighbor_id".to_string(),
            type_hint: "string".to_string(),
            description: "Neighbor router ID".to_string(),
            required: true,
        },
        Parameter {
            name: "sequence".to_string(),
            type_hint: "number".to_string(),
            description: "DD sequence number".to_string(),
            required: true,
        },
        Parameter {
            name: "init".to_string(),
            type_hint: "boolean".to_string(),
            description: "Init flag".to_string(),
            required: true,
        },
        Parameter {
            name: "more".to_string(),
            type_hint: "boolean".to_string(),
            description: "More flag".to_string(),
            required: true,
        },
        Parameter {
            name: "master".to_string(),
            type_hint: "boolean".to_string(),
            description: "Master/Slave flag".to_string(),
            required: true,
        },
    ])
});

/// OSPF Link State Update received event
pub static OSPF_CLIENT_LSU_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ospf_link_state_update_received",
        "OSPF Link State Update packet received with LSAs"
    )
    .with_parameters(vec![
        Parameter {
            name: "neighbor_id".to_string(),
            type_hint: "string".to_string(),
            description: "Neighbor router ID".to_string(),
            required: true,
        },
        Parameter {
            name: "lsa_count".to_string(),
            type_hint: "number".to_string(),
            description: "Number of LSAs in update".to_string(),
            required: true,
        },
    ])
});

/// OSPF client protocol action handler
#[derive(Default)]
pub struct OspfClientProtocol;

impl OspfClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for OspfClientProtocol {
        fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
            vec![
                ActionDefinition {
                    name: "send_hello".to_string(),
                    description: "Send OSPF Hello packet to discover neighbors".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "router_id".to_string(),
                            type_hint: "string".to_string(),
                            description: "Our router ID (e.g., '1.1.1.1')".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "area_id".to_string(),
                            type_hint: "string".to_string(),
                            description: "OSPF area ID (e.g., '0.0.0.0' for backbone)".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "network_mask".to_string(),
                            type_hint: "string".to_string(),
                            description: "Network mask (e.g., '255.255.255.0')".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "priority".to_string(),
                            type_hint: "number".to_string(),
                            description: "Router priority (0-255, 0 means never DR)".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "neighbors".to_string(),
                            type_hint: "array".to_string(),
                            description: "List of neighbor router IDs we've seen".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "destination".to_string(),
                            type_hint: "string".to_string(),
                            description: "Destination: 'multicast', 'dr_multicast', or IP address".to_string(),
                            required: false,
                        },
                    ],
                    example: json!({
                        "type": "send_hello",
                        "router_id": "1.1.1.1",
                        "area_id": "0.0.0.0",
                        "network_mask": "255.255.255.0",
                        "priority": 1,
                        "neighbors": [],
                        "destination": "multicast"
                    }),
                },
                ActionDefinition {
                    name: "send_database_description".to_string(),
                    description: "Send Database Description packet to exchange LSDB info".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "router_id".to_string(),
                            type_hint: "string".to_string(),
                            description: "Our router ID".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "area_id".to_string(),
                            type_hint: "string".to_string(),
                            description: "OSPF area ID".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "sequence".to_string(),
                            type_hint: "number".to_string(),
                            description: "DD sequence number".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "init".to_string(),
                            type_hint: "boolean".to_string(),
                            description: "Init flag (first DD packet)".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "more".to_string(),
                            type_hint: "boolean".to_string(),
                            description: "More flag (more DD packets to follow)".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "master".to_string(),
                            type_hint: "boolean".to_string(),
                            description: "Master/Slave flag".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "destination".to_string(),
                            type_hint: "string".to_string(),
                            description: "Destination IP address".to_string(),
                            required: true,
                        },
                    ],
                    example: json!({
                        "type": "send_database_description",
                        "router_id": "1.1.1.1",
                        "area_id": "0.0.0.0",
                        "sequence": 12345,
                        "init": false,
                        "more": true,
                        "master": true,
                        "destination": "192.168.1.2"
                    }),
                },
                ActionDefinition {
                    name: "send_link_state_request".to_string(),
                    description: "Send Link State Request to query specific LSAs".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "router_id".to_string(),
                            type_hint: "string".to_string(),
                            description: "Our router ID".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "area_id".to_string(),
                            type_hint: "string".to_string(),
                            description: "OSPF area ID".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "destination".to_string(),
                            type_hint: "string".to_string(),
                            description: "Destination IP address".to_string(),
                            required: true,
                        },
                    ],
                    example: json!({
                        "type": "send_link_state_request",
                        "router_id": "1.1.1.1",
                        "area_id": "0.0.0.0",
                        "destination": "192.168.1.2"
                    }),
                },
                ActionDefinition {
                    name: "disconnect".to_string(),
                    description: "Stop OSPF client and leave multicast group".to_string(),
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
                    name: "wait_for_more".to_string(),
                    description: "Wait for more OSPF packets without sending a response".to_string(),
                    parameters: vec![],
                    example: json!({
                        "type": "wait_for_more"
                    }),
                },
            ]
        }
        fn get_event_types(&self) -> Vec<EventType> {
            vec![
                OSPF_CLIENT_CONNECTED_EVENT.clone(),
                OSPF_CLIENT_HELLO_RECEIVED_EVENT.clone(),
                OSPF_CLIENT_DD_RECEIVED_EVENT.clone(),
                OSPF_CLIENT_LSU_RECEIVED_EVENT.clone(),
            ]
        }
        fn protocol_name(&self) -> &'static str {
            "ospf"
        }
        fn stack_name(&self) -> &'static str {
            "network"
        }
        fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
            vec![
                crate::llm::actions::ParameterDefinition {
                    name: "router_id".to_string(),
                    type_hint: "string".to_string(),
                    description: "OSPF router ID (defaults to interface IP)".to_string(),
                    required: false,
                    example: json!("1.1.1.1"),
                },
                crate::llm::actions::ParameterDefinition {
                    name: "area_id".to_string(),
                    type_hint: "string".to_string(),
                    description: "OSPF area ID (default: 0.0.0.0)".to_string(),
                    required: false,
                    example: json!("0.0.0.0"),
                },
            ]
        }
        fn keywords(&self) -> Vec<&'static str> {
            vec!["ospf", "ospf client", "open shortest path first"]
        }
        fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
            use crate::protocol::metadata::*;
            ProtocolMetadataV2 {
                state: DevelopmentState::Experimental,
                privilege_requirement: PrivilegeRequirement::RawSockets,
                implementation: "Raw IP socket (protocol 89) client for OSPF network monitoring",
                llm_control: "LLM controls Hello packet sending, Database Description requests, LSR queries",
                e2e_testing: "E2E tests verify OSPF packet exchange with server (requires root)",
                notes: Some("Query mode only - topology discovery, not full OSPF router"),
            }
        }
        fn description(&self) -> &'static str {
            "OSPF client for network topology discovery and OSPF monitoring"
        }
        fn example_prompt(&self) -> &'static str {
            "Connect to 192.168.1.100 via OSPF in area 0, discover neighbors and query LSDB for topology"
        }
        fn group_name(&self) -> &'static str {
            "VPN & Routing"
        }
}

// Implement Client trait (client-specific functionality)
impl Client for OspfClientProtocol {
        fn connect(
            &self,
            ctx: crate::protocol::ConnectContext,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
        > {
            Box::pin(async move {
                use crate::client::ospf::OspfClient;
                OspfClient::connect_with_llm_actions(
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
            let action_type = action["type"]
                .as_str()
                .context("Missing 'type' field in action")?;
    
            match action_type {
                "send_hello" => {
                    Ok(ClientActionResult::Custom {
                        name: "ospf_send_hello".to_string(),
                        data: action.clone(),
                    })
                }
                "send_database_description" => {
                    Ok(ClientActionResult::Custom {
                        name: "ospf_send_dd".to_string(),
                        data: action.clone(),
                    })
                }
                "send_link_state_request" => {
                    Ok(ClientActionResult::Custom {
                        name: "ospf_send_lsr".to_string(),
                        data: action.clone(),
                    })
                }
                "disconnect" => Ok(ClientActionResult::Disconnect),
                "wait_for_more" => Ok(ClientActionResult::WaitForMore),
                _ => Err(anyhow!("Unknown action type: {}", action_type)),
            }
        }
}

