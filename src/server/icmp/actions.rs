//! ICMP protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// ICMP protocol action handler
pub struct IcmpProtocol;

impl IcmpProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for IcmpProtocol {
    fn default_binding(&self) -> Option<crate::protocol::BindingDefaults> {
        // ICMP uses interface-based binding (loopback by default)
        Some(crate::protocol::BindingDefaults::interface_based("lo"))
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_echo_reply_action(),
            send_destination_unreachable_action(),
            send_time_exceeded_action(),
            // send_timestamp_reply_action(), // TODO: Removed - timestamp support requires pnet timestamp packet types
            ignore_icmp_action(),
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "ICMP"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_icmp_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "IP>ICMP"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["icmp", "ping", "echo", "traceroute"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{
            DevelopmentState, PrivilegeRequirement, ProtocolMetadataV2,
        };

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .privilege_requirement(PrivilegeRequirement::RawSockets)
            .implementation("Raw IP sockets + pnet for ICMP packet handling")
            .llm_control("Full control - can respond to all ICMP message types")
            .e2e_testing("pnet for packet crafting and validation")
            .notes("Requires root/CAP_NET_RAW for raw socket access")
            .build()
    }

    fn description(&self) -> &'static str {
        "ICMP (Internet Control Message Protocol) server"
    }

    fn example_prompt(&self) -> &'static str {
        "Listen for ICMP echo requests on eth0"
    }

    fn group_name(&self) -> &'static str {
        "Core"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        StartupExamples::new(
            json!({
                "type": "open_server",
                "interface": "eth0",
                "base_stack": "icmp",
                "instruction": "ICMP server that responds to ping requests"
            }),
            json!({
                "type": "open_server",
                "interface": "eth0",
                "base_stack": "icmp",
                "event_handlers": [{
                    "event_pattern": "icmp_echo_request",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<icmp_handler>"
                    }
                }]
            }),
            json!({
                "type": "open_server",
                "interface": "eth0",
                "base_stack": "icmp",
                "event_handlers": [{
                    "event_pattern": "icmp_echo_request",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "ignore_icmp"
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for IcmpProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::icmp::IcmpServer;

            // ICMP uses interface-based binding
            // Extract interface from context (defaults already applied)
            let interface = ctx
                .interface()
                .context("ICMP requires network interface")?
                .to_string();

            // Spawn the ICMP server
            let _interface_name = IcmpServer::spawn_with_llm(
                interface,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            )
            .await?;

            // ICMP doesn't bind to a socket, so return a dummy address
            // The listen_addr from context is just a placeholder
            Ok(ctx.listen_addr)
        })
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "send_echo_reply" => self.execute_send_echo_reply(action),
            "send_destination_unreachable" => self.execute_send_destination_unreachable(action),
            "send_time_exceeded" => self.execute_send_time_exceeded(action),
            // "send_timestamp_reply" => self.execute_send_timestamp_reply(action), // TODO: Removed - timestamp support requires pnet timestamp packet types
            "ignore_icmp" => Ok(ActionResult::NoAction),
            _ => Err(anyhow::anyhow!("Unknown ICMP action: {}", action_type)),
        }
    }
}

impl IcmpProtocol {
    /// Execute send_echo_reply action
    fn execute_send_echo_reply(&self, action: serde_json::Value) -> Result<ActionResult> {
        let source_ip = action
            .get("source_ip")
            .and_then(|v| v.as_str())
            .context("Missing 'source_ip' parameter")?;

        let destination_ip = action
            .get("destination_ip")
            .and_then(|v| v.as_str())
            .context("Missing 'destination_ip' parameter")?;

        let identifier = action
            .get("identifier")
            .and_then(|v| v.as_u64())
            .context("Missing 'identifier' parameter")? as u16;

        let sequence = action
            .get("sequence")
            .and_then(|v| v.as_u64())
            .context("Missing 'sequence' parameter")? as u16;

        let payload_hex = action
            .get("payload_hex")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let payload = if payload_hex.is_empty() {
            Vec::new()
        } else {
            hex::decode(payload_hex).context("Invalid hex in payload_hex")?
        };

        // Parse IP addresses
        let source_ip_parsed: std::net::Ipv4Addr =
            source_ip.parse().context("Invalid source_ip format")?;
        let destination_ip_parsed: std::net::Ipv4Addr =
            destination_ip.parse().context("Invalid destination_ip format")?;

        // Build ICMP echo reply packet
        use crate::server::icmp::IcmpServer;
        let packet = IcmpServer::build_echo_reply(
            source_ip_parsed,
            destination_ip_parsed,
            identifier,
            sequence,
            &payload,
        );

        Ok(ActionResult::Output(packet))
    }

    /// Execute send_destination_unreachable action
    fn execute_send_destination_unreachable(&self, action: serde_json::Value) -> Result<ActionResult> {
        let source_ip = action
            .get("source_ip")
            .and_then(|v| v.as_str())
            .context("Missing 'source_ip' parameter")?;

        let destination_ip = action
            .get("destination_ip")
            .and_then(|v| v.as_str())
            .context("Missing 'destination_ip' parameter")?;

        let code = action
            .get("code")
            .and_then(|v| v.as_u64())
            .context("Missing 'code' parameter")? as u8;

        let original_packet_hex = action
            .get("original_packet_hex")
            .and_then(|v| v.as_str())
            .context("Missing 'original_packet_hex' parameter")?;

        let original_packet = hex::decode(original_packet_hex).context("Invalid hex in original_packet_hex")?;

        // Parse IP addresses
        let source_ip_parsed: std::net::Ipv4Addr =
            source_ip.parse().context("Invalid source_ip format")?;
        let destination_ip_parsed: std::net::Ipv4Addr =
            destination_ip.parse().context("Invalid destination_ip format")?;

        // Build ICMP destination unreachable packet
        use crate::server::icmp::IcmpServer;
        let packet = IcmpServer::build_destination_unreachable(
            source_ip_parsed,
            destination_ip_parsed,
            code,
            &original_packet,
        );

        Ok(ActionResult::Output(packet))
    }

    /// Execute send_time_exceeded action
    fn execute_send_time_exceeded(&self, action: serde_json::Value) -> Result<ActionResult> {
        let source_ip = action
            .get("source_ip")
            .and_then(|v| v.as_str())
            .context("Missing 'source_ip' parameter")?;

        let destination_ip = action
            .get("destination_ip")
            .and_then(|v| v.as_str())
            .context("Missing 'destination_ip' parameter")?;

        let code = action
            .get("code")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u8; // Default to 0 (TTL exceeded in transit)

        let original_packet_hex = action
            .get("original_packet_hex")
            .and_then(|v| v.as_str())
            .context("Missing 'original_packet_hex' parameter")?;

        let original_packet = hex::decode(original_packet_hex).context("Invalid hex in original_packet_hex")?;

        // Parse IP addresses
        let source_ip_parsed: std::net::Ipv4Addr =
            source_ip.parse().context("Invalid source_ip format")?;
        let destination_ip_parsed: std::net::Ipv4Addr =
            destination_ip.parse().context("Invalid destination_ip format")?;

        // Build ICMP time exceeded packet
        use crate::server::icmp::IcmpServer;
        let packet = IcmpServer::build_time_exceeded(
            source_ip_parsed,
            destination_ip_parsed,
            code,
            &original_packet,
        );

        Ok(ActionResult::Output(packet))
    }

    /* TODO: Timestamp support requires pnet to add timestamp packet types
    /// Execute send_timestamp_reply action
    fn execute_send_timestamp_reply(&self, action: serde_json::Value) -> Result<ActionResult> {
        let source_ip = action
            .get("source_ip")
            .and_then(|v| v.as_str())
            .context("Missing 'source_ip' parameter")?;

        let destination_ip = action
            .get("destination_ip")
            .and_then(|v| v.as_str())
            .context("Missing 'destination_ip' parameter")?;

        let identifier = action
            .get("identifier")
            .and_then(|v| v.as_u64())
            .context("Missing 'identifier' parameter")? as u16;

        let sequence = action
            .get("sequence")
            .and_then(|v| v.as_u64())
            .context("Missing 'sequence' parameter")? as u16;

        let originate_timestamp = action
            .get("originate_timestamp")
            .and_then(|v| v.as_u64())
            .context("Missing 'originate_timestamp' parameter")? as u32;

        // Parse IP addresses
        let source_ip_parsed: std::net::Ipv4Addr =
            source_ip.parse().context("Invalid source_ip format")?;
        let destination_ip_parsed: std::net::Ipv4Addr =
            destination_ip.parse().context("Invalid destination_ip format")?;

        // Build ICMP timestamp reply packet
        use crate::server::icmp::IcmpServer;
        let packet = IcmpServer::build_timestamp_reply(
            source_ip_parsed,
            destination_ip_parsed,
            identifier,
            sequence,
            originate_timestamp,
        );

        Ok(ActionResult::Output(packet))
    }
    */
}

/// Action definition for send_echo_reply
fn send_echo_reply_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_echo_reply".to_string(),
        description: "Send an ICMP Echo Reply packet in response to an Echo Request (ping)"
            .to_string(),
        parameters: vec![
            Parameter {
                name: "source_ip".to_string(),
                type_hint: "string".to_string(),
                description: "Source IP address (format: X.X.X.X)".to_string(),
                required: true,
            },
            Parameter {
                name: "destination_ip".to_string(),
                type_hint: "string".to_string(),
                description: "Destination IP address (format: X.X.X.X)".to_string(),
                required: true,
            },
            Parameter {
                name: "identifier".to_string(),
                type_hint: "number".to_string(),
                description: "ICMP identifier (must match request)".to_string(),
                required: true,
            },
            Parameter {
                name: "sequence".to_string(),
                type_hint: "number".to_string(),
                description: "ICMP sequence number (must match request)".to_string(),
                required: true,
            },
            Parameter {
                name: "payload_hex".to_string(),
                type_hint: "string".to_string(),
                description: "Payload data as hex string (must match request)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_echo_reply",
            "source_ip": "192.168.1.100",
            "destination_ip": "192.168.1.50",
            "identifier": 1234,
            "sequence": 1,
            "payload_hex": "48656c6c6f"
        }),
    }
}

/// Action definition for send_destination_unreachable
fn send_destination_unreachable_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_destination_unreachable".to_string(),
        description: "Send an ICMP Destination Unreachable message".to_string(),
        parameters: vec![
            Parameter {
                name: "source_ip".to_string(),
                type_hint: "string".to_string(),
                description: "Source IP address (format: X.X.X.X)".to_string(),
                required: true,
            },
            Parameter {
                name: "destination_ip".to_string(),
                type_hint: "string".to_string(),
                description: "Destination IP address (format: X.X.X.X)".to_string(),
                required: true,
            },
            Parameter {
                name: "code".to_string(),
                type_hint: "number".to_string(),
                description: "Unreachable code: 0=net, 1=host, 2=protocol, 3=port, 4=fragmentation needed, 5=source route failed".to_string(),
                required: true,
            },
            Parameter {
                name: "original_packet_hex".to_string(),
                type_hint: "string".to_string(),
                description: "Original IP header + first 8 bytes of original datagram (hex)"
                    .to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_destination_unreachable",
            "source_ip": "192.168.1.1",
            "destination_ip": "192.168.1.50",
            "code": 1,
            "original_packet_hex": "4500003c..."
        }),
    }
}

/// Action definition for send_time_exceeded
fn send_time_exceeded_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_time_exceeded".to_string(),
        description: "Send an ICMP Time Exceeded message (used in traceroute)".to_string(),
        parameters: vec![
            Parameter {
                name: "source_ip".to_string(),
                type_hint: "string".to_string(),
                description: "Source IP address (format: X.X.X.X)".to_string(),
                required: true,
            },
            Parameter {
                name: "destination_ip".to_string(),
                type_hint: "string".to_string(),
                description: "Destination IP address (format: X.X.X.X)".to_string(),
                required: true,
            },
            Parameter {
                name: "code".to_string(),
                type_hint: "number".to_string(),
                description: "Time exceeded code: 0=TTL exceeded in transit, 1=fragment reassembly time exceeded".to_string(),
                required: false,
            },
            Parameter {
                name: "original_packet_hex".to_string(),
                type_hint: "string".to_string(),
                description: "Original IP header + first 8 bytes of original datagram (hex)"
                    .to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_time_exceeded",
            "source_ip": "10.0.0.1",
            "destination_ip": "192.168.1.50",
            "code": 0,
            "original_packet_hex": "4500003c..."
        }),
    }
}

/* TODO: Timestamp support requires pnet to add timestamp packet types
/// Action definition for send_timestamp_reply
fn send_timestamp_reply_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_timestamp_reply".to_string(),
        description: "Send an ICMP Timestamp Reply message".to_string(),
        parameters: vec![
            Parameter {
                name: "source_ip".to_string(),
                type_hint: "string".to_string(),
                description: "Source IP address (format: X.X.X.X)".to_string(),
                required: true,
            },
            Parameter {
                name: "destination_ip".to_string(),
                type_hint: "string".to_string(),
                description: "Destination IP address (format: X.X.X.X)".to_string(),
                required: true,
            },
            Parameter {
                name: "identifier".to_string(),
                type_hint: "number".to_string(),
                description: "ICMP identifier (must match request)".to_string(),
                required: true,
            },
            Parameter {
                name: "sequence".to_string(),
                type_hint: "number".to_string(),
                description: "ICMP sequence number (must match request)".to_string(),
                required: true,
            },
            Parameter {
                name: "originate_timestamp".to_string(),
                type_hint: "number".to_string(),
                description:
                    "Originate timestamp from request (milliseconds since midnight UT)".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_timestamp_reply",
            "source_ip": "192.168.1.1",
            "destination_ip": "192.168.1.50",
            "identifier": 1234,
            "sequence": 1,
            "originate_timestamp": 12345678
        }),
    }
}
*/

/// Action definition for ignore_icmp
fn ignore_icmp_action() -> ActionDefinition {
    ActionDefinition {
        name: "ignore_icmp".to_string(),
        description: "Ignore this ICMP packet (no action taken)".to_string(),
        parameters: vec![],
        example: json!({
            "type": "ignore_icmp"
        }),
    }
}

// ============================================================================
// ICMP Event Type Constants
// ============================================================================

pub static ICMP_ECHO_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "icmp_echo_request",
        "ICMP Echo Request (ping) received from network",
        json!({
            "type": "send_echo_reply",
            "source_ip": "192.168.1.100",
            "destination_ip": "192.168.1.50",
            "identifier": 1234,
            "sequence": 1,
            "payload_hex": "48656c6c6f"
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "source_ip".to_string(),
            type_hint: "string".to_string(),
            description: "Source IP address of the ping request".to_string(),
            required: true,
        },
        Parameter {
            name: "destination_ip".to_string(),
            type_hint: "string".to_string(),
            description: "Destination IP address (our server)".to_string(),
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
            name: "payload_hex".to_string(),
            type_hint: "string".to_string(),
            description: "Hexadecimal representation of the payload data".to_string(),
            required: false,
        },
        Parameter {
            name: "ttl".to_string(),
            type_hint: "number".to_string(),
            description: "Time to live from IP header".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![send_echo_reply_action(), ignore_icmp_action()])
});

/* TODO: Timestamp support requires pnet to add timestamp packet types
pub static ICMP_TIMESTAMP_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "icmp_timestamp_request",
        "ICMP Timestamp Request received from network",
    )
    .with_parameters(vec![
        Parameter {
            name: "source_ip".to_string(),
            type_hint: "string".to_string(),
            description: "Source IP address".to_string(),
            required: true,
        },
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
        Parameter {
            name: "originate_timestamp".to_string(),
            type_hint: "number".to_string(),
            description: "Originate timestamp (milliseconds since midnight UT)".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![send_timestamp_reply_action(), ignore_icmp_action()])
});
*/

pub static ICMP_OTHER_MESSAGE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "icmp_other_message",
        "Other ICMP message type received (not echo or timestamp)",
        json!({
            "type": "send_destination_unreachable",
            "source_ip": "192.168.1.1",
            "destination_ip": "192.168.1.50",
            "code": 1,
            "original_packet_hex": "4500003c..."
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "source_ip".to_string(),
            type_hint: "string".to_string(),
            description: "Source IP address".to_string(),
            required: true,
        },
        Parameter {
            name: "destination_ip".to_string(),
            type_hint: "string".to_string(),
            description: "Destination IP address".to_string(),
            required: true,
        },
        Parameter {
            name: "icmp_type".to_string(),
            type_hint: "number".to_string(),
            description: "ICMP message type".to_string(),
            required: true,
        },
        Parameter {
            name: "icmp_code".to_string(),
            type_hint: "number".to_string(),
            description: "ICMP message code".to_string(),
            required: true,
        },
        Parameter {
            name: "packet_hex".to_string(),
            type_hint: "string".to_string(),
            description: "Full ICMP packet as hex".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![
        send_destination_unreachable_action(),
        send_time_exceeded_action(),
        ignore_icmp_action(),
    ])
});

pub fn get_icmp_event_types() -> Vec<EventType> {
    vec![
        ICMP_ECHO_REQUEST_EVENT.clone(),
        // ICMP_TIMESTAMP_REQUEST_EVENT.clone(), // TODO: Removed - timestamp support requires pnet timestamp packet types
        ICMP_OTHER_MESSAGE_EVENT.clone(),
    ]
}
