//! ICMP client protocol actions implementation

use crate::llm::actions::client_trait::{Client, ClientActionResult};
use crate::llm::actions::protocol_trait::Protocol;
use crate::llm::actions::{ActionDefinition, Parameter};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// ICMP client protocol handler
pub struct IcmpClientProtocol;

impl IcmpClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for IcmpClientProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_echo_request_action(),
            // send_timestamp_request_action(), // TODO: Removed - timestamp support requires pnet timestamp packet types
            wait_for_more_action(),
            disconnect_action(),
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "ICMP"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_icmp_client_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "IP>ICMP"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["icmp", "icmp client", "ping", "traceroute"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Raw IP sockets with pnet_packet for ICMP")
            .llm_control("Full control over ICMP message types, TTL, and payloads")
            .e2e_testing("Real ICMP pings to public IPs (requires CAP_NET_RAW)")
            .build()
    }

    fn description(&self) -> &'static str {
        "ICMP client for sending echo requests (ping) and traceroute"
    }

    fn example_prompt(&self) -> &'static str {
        "Ping 8.8.8.8 every 5 seconds and report RTT"
    }

    fn group_name(&self) -> &'static str {
        "Core"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        use serde_json::json;

        StartupExamples::new(
            // LLM mode: LLM handles ICMP client
            json!({
                "type": "open_client",
                "remote_addr": "8.8.8.8",
                "base_stack": "icmp",
                "instruction": "Ping 8.8.8.8 five times and report average latency"
            }),
            // Script mode: Code-based ICMP handling
            json!({
                "type": "open_client",
                "remote_addr": "8.8.8.8",
                "base_stack": "icmp",
                "event_handlers": [{
                    "event_pattern": "icmp_echo_reply",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<icmp_handler>"
                    }
                }]
            }),
            // Static mode: Fixed ICMP ping
            json!({
                "type": "open_client",
                "remote_addr": "8.8.8.8",
                "base_stack": "icmp",
                "event_handlers": [{
                    "event_pattern": "icmp_connected",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "send_echo_request",
                            "destination_ip": "8.8.8.8",
                            "identifier": 1234,
                            "sequence": 1,
                            "ttl": 64
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Client trait (client-specific functionality)
impl Client for IcmpClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            crate::client::icmp::IcmpClient::connect_with_llm_actions(
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
            "send_echo_request" => {
                let destination_ip = action
                    .get("destination_ip")
                    .and_then(|v| v.as_str())
                    .context("Missing 'destination_ip' parameter")?;

                let identifier = action
                    .get("identifier")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1234) as u16;

                let sequence = action
                    .get("sequence")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1) as u16;

                let payload_hex = action.get("payload_hex").and_then(|v| v.as_str()).unwrap_or("");

                let ttl = action.get("ttl").and_then(|v| v.as_u64()).unwrap_or(64) as u8;

                Ok(ClientActionResult::Custom {
                    name: "send_echo_request".to_string(),
                    data: json!({
                        "destination_ip": destination_ip,
                        "identifier": identifier,
                        "sequence": sequence,
                        "payload_hex": payload_hex,
                        "ttl": ttl,
                    }),
                })
            }
            /* TODO: Timestamp support requires pnet to add timestamp packet types
            "send_timestamp_request" => {
                let destination_ip = action
                    .get("destination_ip")
                    .and_then(|v| v.as_str())
                    .context("Missing 'destination_ip' parameter")?;

                let identifier = action
                    .get("identifier")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1234) as u16;

                let sequence = action
                    .get("sequence")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1) as u16;

                Ok(ClientActionResult::Custom {
                    name: "send_timestamp_request".to_string(),
                    data: json!({
                        "destination_ip": destination_ip,
                        "identifier": identifier,
                        "sequence": sequence,
                    }),
                })
            }
            */
            "wait_for_more" => Ok(ClientActionResult::WaitForMore),
            "disconnect" => Ok(ClientActionResult::Disconnect),
            _ => Err(anyhow::anyhow!("Unknown ICMP client action: {}", action_type)),
        }
    }
}

/// Action definition for send_echo_request
fn send_echo_request_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_echo_request".to_string(),
        description: "Send an ICMP Echo Request (ping) to a destination".to_string(),
        parameters: vec![
            Parameter {
                name: "destination_ip".to_string(),
                type_hint: "string".to_string(),
                description: "Destination IP address (format: X.X.X.X)".to_string(),
                required: true,
            },
            Parameter {
                name: "identifier".to_string(),
                type_hint: "number".to_string(),
                description: "ICMP identifier (default: 1234)".to_string(),
                required: false,
            },
            Parameter {
                name: "sequence".to_string(),
                type_hint: "number".to_string(),
                description: "ICMP sequence number (default: 1)".to_string(),
                required: false,
            },
            Parameter {
                name: "payload_hex".to_string(),
                type_hint: "string".to_string(),
                description: "Payload data as hex string (default: empty)".to_string(),
                required: false,
            },
            Parameter {
                name: "ttl".to_string(),
                type_hint: "number".to_string(),
                description: "Time to live (default: 64)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_echo_request",
            "destination_ip": "8.8.8.8",
            "identifier": 1234,
            "sequence": 1,
            "payload_hex": "48656c6c6f",
            "ttl": 64
        }),
    }
}

/* TODO: Timestamp support requires pnet to add timestamp packet types
/// Action definition for send_timestamp_request
fn send_timestamp_request_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_timestamp_request".to_string(),
        description: "Send an ICMP Timestamp Request to a destination".to_string(),
        parameters: vec![
            Parameter {
                name: "destination_ip".to_string(),
                type_hint: "string".to_string(),
                description: "Destination IP address (format: X.X.X.X)".to_string(),
                required: true,
            },
            Parameter {
                name: "identifier".to_string(),
                type_hint: "number".to_string(),
                description: "ICMP identifier (default: 1234)".to_string(),
                required: false,
            },
            Parameter {
                name: "sequence".to_string(),
                type_hint: "number".to_string(),
                description: "ICMP sequence number (default: 1)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_timestamp_request",
            "destination_ip": "192.168.1.1",
            "identifier": 5678,
            "sequence": 1
        }),
    }
}
*/

/// Action definition for wait_for_more
fn wait_for_more_action() -> ActionDefinition {
    ActionDefinition {
        name: "wait_for_more".to_string(),
        description: "Wait for more ICMP responses".to_string(),
        parameters: vec![],
        example: json!({"type": "wait_for_more"}),
    }
}

/// Action definition for disconnect
fn disconnect_action() -> ActionDefinition {
    ActionDefinition {
        name: "disconnect".to_string(),
        description: "Close the ICMP client".to_string(),
        parameters: vec![],
        example: json!({"type": "disconnect"}),
    }
}

// ============================================================================
// ICMP Client Event Type Constants
// ============================================================================

pub static ICMP_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "icmp_connected",
        "ICMP client socket created",
        json!({
            "type": "send_echo_request",
            "destination_ip": "8.8.8.8",
            "identifier": 1234,
            "sequence": 1,
            "ttl": 64
        })
    )
    .with_parameters(vec![
        Parameter {
            name: "local_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Local address".to_string(),
            required: true,
        },
        Parameter {
            name: "target_ip".to_string(),
            type_hint: "string".to_string(),
            description: "Target IP address".to_string(),
            required: true,
        },
    ])
});

pub static ICMP_ECHO_REPLY_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "icmp_echo_reply",
        "ICMP Echo Reply (ping response) received",
        json!({
            "type": "send_echo_request",
            "destination_ip": "8.8.8.8",
            "identifier": 1234,
            "sequence": 2,
            "ttl": 64
        })
    )
    .with_parameters(vec![
        Parameter {
            name: "source_ip".to_string(),
            type_hint: "string".to_string(),
            description: "Source IP address of the reply".to_string(),
            required: true,
        },
        Parameter {
            name: "identifier".to_string(),
            type_hint: "number".to_string(),
            description: "ICMP identifier".to_string(),
            required: true,
        },
        Parameter {
            name: "sequence".to_string(),
            type_hint: "number".to_string(),
            description: "ICMP sequence number".to_string(),
            required: true,
        },
        Parameter {
            name: "rtt_ms".to_string(),
            type_hint: "number".to_string(),
            description: "Round-trip time in milliseconds".to_string(),
            required: false,
        },
        Parameter {
            name: "ttl".to_string(),
            type_hint: "number".to_string(),
            description: "TTL from IP header".to_string(),
            required: false,
        },
        Parameter {
            name: "payload_hex".to_string(),
            type_hint: "string".to_string(),
            description: "Payload data as hex".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![
        send_echo_request_action(),
        wait_for_more_action(),
        disconnect_action(),
    ])
});

pub static ICMP_TIMEOUT_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "icmp_timeout",
        "ICMP request timed out without reply",
        json!({
            "type": "send_echo_request",
            "destination_ip": "8.8.8.8",
            "identifier": 1234,
            "sequence": 2,
            "ttl": 64
        })
    )
    .with_parameters(vec![
        Parameter {
            name: "destination_ip".to_string(),
            type_hint: "string".to_string(),
            description: "Destination IP address".to_string(),
            required: true,
        },
        Parameter {
            name: "identifier".to_string(),
            type_hint: "number".to_string(),
            description: "ICMP identifier".to_string(),
            required: true,
        },
        Parameter {
            name: "sequence".to_string(),
            type_hint: "number".to_string(),
            description: "ICMP sequence number".to_string(),
            required: true,
        },
    ])
});

pub static ICMP_DEST_UNREACHABLE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "icmp_destination_unreachable",
        "ICMP Destination Unreachable received",
        json!({
            "type": "wait_for_more"
        })
    )
    .with_parameters(vec![
        Parameter {
            name: "source_ip".to_string(),
            type_hint: "string".to_string(),
            description: "Source IP of the unreachable message".to_string(),
            required: true,
        },
        Parameter {
            name: "code".to_string(),
            type_hint: "number".to_string(),
            description: "Unreachable code (0=net, 1=host, 2=protocol, 3=port, etc.)".to_string(),
            required: true,
        },
    ])
});

pub static ICMP_TIME_EXCEEDED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "icmp_time_exceeded",
        "ICMP Time Exceeded (TTL=0) received - used in traceroute",
        json!({
            "type": "send_echo_request",
            "destination_ip": "8.8.8.8",
            "identifier": 1234,
            "sequence": 1,
            "ttl": 2
        })
    )
    .with_parameters(vec![
        Parameter {
            name: "source_ip".to_string(),
            type_hint: "string".to_string(),
            description: "Source IP of the time exceeded message (hop address)".to_string(),
            required: true,
        },
        Parameter {
            name: "code".to_string(),
            type_hint: "number".to_string(),
            description: "Time exceeded code (0=TTL, 1=fragment reassembly)".to_string(),
            required: true,
        },
    ])
});

pub fn get_icmp_client_event_types() -> Vec<EventType> {
    vec![
        ICMP_CLIENT_CONNECTED_EVENT.clone(),
        ICMP_ECHO_REPLY_EVENT.clone(),
        ICMP_TIMEOUT_EVENT.clone(),
        ICMP_DEST_UNREACHABLE_EVENT.clone(),
        ICMP_TIME_EXCEEDED_EVENT.clone(),
    ]
}
