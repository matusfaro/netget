//! IGMP client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::LazyLock;

/// IGMP client connected event
pub static IGMP_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "igmp_connected",
        "IGMP client initialized and ready to join multicast groups"
    )
    .with_parameters(vec![
        Parameter {
            name: "local_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Local socket address bound for multicast reception".to_string(),
            required: true,
        },
    ])
});

/// IGMP client data received event
pub static IGMP_CLIENT_DATA_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "igmp_data_received",
        "Multicast data received from group"
    )
    .with_parameters(vec![
        Parameter {
            name: "data_hex".to_string(),
            type_hint: "string".to_string(),
            description: "The multicast data received (as hex string)".to_string(),
            required: true,
        },
        Parameter {
            name: "data_length".to_string(),
            type_hint: "number".to_string(),
            description: "Length of data in bytes".to_string(),
            required: true,
        },
        Parameter {
            name: "source_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Source address of the multicast sender".to_string(),
            required: true,
        },
    ])
});

/// IGMP client protocol action handler
pub struct IgmpClientProtocol;

impl IgmpClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Client for IgmpClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::igmp::IgmpClient;
            IgmpClient::connect_with_llm_actions(
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
                name: "join_multicast_group".to_string(),
                description: "Join a multicast group to start receiving multicast traffic".to_string(),
                parameters: vec![
                    Parameter {
                        name: "multicast_addr".to_string(),
                        type_hint: "string".to_string(),
                        description: "Multicast group IP address (e.g., 239.1.2.3)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "interface_addr".to_string(),
                        type_hint: "string".to_string(),
                        description: "Local interface address (use 0.0.0.0 for any interface)".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "join_multicast_group",
                    "multicast_addr": "239.1.2.3",
                    "interface_addr": "0.0.0.0"
                }),
            },
            ActionDefinition {
                name: "leave_multicast_group".to_string(),
                description: "Leave a multicast group to stop receiving multicast traffic".to_string(),
                parameters: vec![
                    Parameter {
                        name: "multicast_addr".to_string(),
                        type_hint: "string".to_string(),
                        description: "Multicast group IP address to leave".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "interface_addr".to_string(),
                        type_hint: "string".to_string(),
                        description: "Local interface address (use 0.0.0.0 for any interface)".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "leave_multicast_group",
                    "multicast_addr": "239.1.2.3",
                    "interface_addr": "0.0.0.0"
                }),
            },
            ActionDefinition {
                name: "send_multicast".to_string(),
                description: "Send data to a multicast group".to_string(),
                parameters: vec![
                    Parameter {
                        name: "multicast_addr".to_string(),
                        type_hint: "string".to_string(),
                        description: "Multicast group IP address".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "port".to_string(),
                        type_hint: "number".to_string(),
                        description: "Destination port".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "data_hex".to_string(),
                        type_hint: "string".to_string(),
                        description: "Hexadecimal encoded data to send".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "send_multicast",
                    "multicast_addr": "239.1.2.3",
                    "port": 5000,
                    "data_hex": "48656c6c6f"
                }),
            },
        ]
    }

    fn get_sync_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait for more multicast data before responding".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "wait_for_more"
                }),
            },
        ]
    }

    fn execute_action(
        &self,
        action: serde_json::Value,
        _state: &AppState,
    ) -> Result<ClientActionResult> {
        let action_type = action["type"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing 'type' field in action"))?;

        match action_type {
            "join_multicast_group" => {
                let multicast_addr = action["multicast_addr"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing multicast_addr"))?;

                let interface_addr = action["interface_addr"]
                    .as_str()
                    .unwrap_or("0.0.0.0");

                // Parse addresses
                let multicast_ip: Ipv4Addr = multicast_addr
                    .parse()
                    .context("Invalid multicast address")?;

                let interface_ip: Ipv4Addr = interface_addr
                    .parse()
                    .context("Invalid interface address")?;

                Ok(ClientActionResult::Custom {
                    name: "join_multicast_group".to_string(),
                    data: json!({
                        "multicast_addr": multicast_ip.to_string(),
                        "interface_addr": interface_ip.to_string(),
                    }),
                })
            }
            "leave_multicast_group" => {
                let multicast_addr = action["multicast_addr"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing multicast_addr"))?;

                let interface_addr = action["interface_addr"]
                    .as_str()
                    .unwrap_or("0.0.0.0");

                // Parse addresses
                let multicast_ip: Ipv4Addr = multicast_addr
                    .parse()
                    .context("Invalid multicast address")?;

                let interface_ip: Ipv4Addr = interface_addr
                    .parse()
                    .context("Invalid interface address")?;

                Ok(ClientActionResult::Custom {
                    name: "leave_multicast_group".to_string(),
                    data: json!({
                        "multicast_addr": multicast_ip.to_string(),
                        "interface_addr": interface_ip.to_string(),
                    }),
                })
            }
            "send_multicast" => {
                let multicast_addr = action["multicast_addr"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing multicast_addr"))?;

                let port = action["port"]
                    .as_u64()
                    .ok_or_else(|| anyhow!("Missing or invalid port"))?;

                let data_hex = action["data_hex"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing data_hex"))?;

                // Decode hex data
                let data = hex::decode(data_hex)
                    .context("Invalid hex data")?;

                Ok(ClientActionResult::Custom {
                    name: "send_multicast".to_string(),
                    data: json!({
                        "multicast_addr": multicast_addr,
                        "port": port,
                        "data": data,
                    }),
                })
            }
            "wait_for_more" => Ok(ClientActionResult::WaitForMore),
            _ => Err(anyhow!("Unknown action type: {}", action_type)),
        }
    }

    fn get_event_types(&self) -> Vec<&'static EventType> {
        vec![
            &IGMP_CLIENT_CONNECTED_EVENT,
            &IGMP_CLIENT_DATA_RECEIVED_EVENT,
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "igmp"
    }

    fn stack_name(&self) -> &'static str {
        "Network"
    }

    fn get_startup_params(&self) -> Vec<Parameter> {
        vec![
            Parameter {
                name: "bind_addr".to_string(),
                type_hint: "string".to_string(),
                description: "Local address to bind (default: 0.0.0.0:0)".to_string(),
                required: false,
            },
        ]
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["igmp", "multicast", "igmp client", "multicast client"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .description("IGMP multicast group management client")
            .example_prompt("Join multicast group 239.1.2.3 and wait for data")
            .libraries(vec!["tokio (UDP socket)", "socket2 (multicast join/leave)"])
            .notes("Supports IPv4 multicast only. Uses socket options (no raw packets).")
            .build()
    }

    fn description(&self) -> &'static str {
        "IGMP client for joining/leaving multicast groups and receiving/sending multicast data"
    }

    fn example_prompt(&self) -> &'static str {
        "Join multicast group 239.1.2.3 on port 5000, log all received data, and send 'Hello' to the group"
    }

    fn group_name(&self) -> &'static str {
        "Network"
    }
}
