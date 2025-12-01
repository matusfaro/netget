//! IGMP protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::net::Ipv4Addr;
use std::sync::LazyLock;

/// IGMP protocol action handler
pub struct IgmpProtocol {
    _private: (),
}

impl IgmpProtocol {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for IgmpProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![join_group_action(), leave_group_action()]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_membership_report_action(),
            send_leave_group_action(),
            ignore_message_action(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "IGMP"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_igmp_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>IGMP"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["igmp", "multicast"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Raw socket IGMP with socket2")
            .llm_control("Full control - multicast group membership, query responses")
            .e2e_testing("Manual IGMP packet construction")
            .notes("IGMPv2 support, multicast group management")
            .build()
    }
    fn description(&self) -> &'static str {
        "IGMP multicast group management server"
    }
    fn example_prompt(&self) -> &'static str {
        "Create an IGMP server that joins multicast group 239.255.255.250 and responds to membership queries"
    }
    fn group_name(&self) -> &'static str {
        "Network"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        use serde_json::json;

        StartupExamples::new(
            // LLM mode: LLM handles all IGMP messages intelligently
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "igmp",
                "instruction": "IGMP multicast group management server"
            }),
            // Script mode: Code-based deterministic responses
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "igmp",
                "event_handlers": [{
                    "event_pattern": "igmp_query_received",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<igmp_handler>"
                    }
                }]
            }),
            // Static mode: Fixed responses
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "igmp",
                "event_handlers": [{
                    "event_pattern": "igmp_query_received",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "send_membership_report",
                            "group_address": "239.255.255.250"
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for IgmpProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::igmp::IgmpServer;
            IgmpServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            )
            .await
        })
    }
    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "join_group" => self.execute_join_group(action),
            "leave_group" => self.execute_leave_group(action),
            "send_membership_report" => self.execute_send_membership_report(action),
            "send_leave_group" => self.execute_send_leave_group_message(action),
            "ignore_message" => Ok(ActionResult::NoAction),
            _ => Err(anyhow::anyhow!("Unknown IGMP action: {}", action_type)),
        }
    }
}

impl IgmpProtocol {
    /// Execute join_group async action
    fn execute_join_group(&self, action: serde_json::Value) -> Result<ActionResult> {
        let group = action
            .get("group_address")
            .and_then(|v| v.as_str())
            .context("Missing 'group_address' parameter")?;

        let _addr: Ipv4Addr = group.parse().context("Invalid IPv4 multicast address")?;

        // Return the group address as a custom result for async processing
        Ok(ActionResult::Custom {
            name: "igmp_join_group".to_string(),
            data: json!({"group_address": group}),
        })
    }

    /// Execute leave_group async action
    fn execute_leave_group(&self, action: serde_json::Value) -> Result<ActionResult> {
        let group = action
            .get("group_address")
            .and_then(|v| v.as_str())
            .context("Missing 'group_address' parameter")?;

        let _addr: Ipv4Addr = group.parse().context("Invalid IPv4 multicast address")?;

        // Return the group address as a custom result for async processing
        Ok(ActionResult::Custom {
            name: "igmp_leave_group".to_string(),
            data: json!({"group_address": group}),
        })
    }

    /// Execute send_membership_report sync action
    fn execute_send_membership_report(&self, action: serde_json::Value) -> Result<ActionResult> {
        let group = action
            .get("group_address")
            .and_then(|v| v.as_str())
            .context("Missing 'group_address' parameter")?;

        let addr: Ipv4Addr = group.parse().context("Invalid IPv4 multicast address")?;

        // Build IGMPv2 Membership Report
        let packet = build_igmp_v2_report(addr)?;
        Ok(ActionResult::Output(packet))
    }

    /// Execute send_leave_group sync action
    fn execute_send_leave_group_message(&self, action: serde_json::Value) -> Result<ActionResult> {
        let group = action
            .get("group_address")
            .and_then(|v| v.as_str())
            .context("Missing 'group_address' parameter")?;

        let addr: Ipv4Addr = group.parse().context("Invalid IPv4 multicast address")?;

        // Build IGMPv2 Leave Group message
        let packet = build_igmp_v2_leave(addr)?;
        Ok(ActionResult::Output(packet))
    }
}

/// Build an IGMPv2 Membership Report packet
fn build_igmp_v2_report(group: Ipv4Addr) -> Result<Vec<u8>> {
    let mut packet = Vec::new();

    // Type: Membership Report (0x16)
    packet.push(0x16);

    // Max Response Time: 0 for reports
    packet.push(0x00);

    // Checksum: placeholder (will calculate)
    packet.push(0x00);
    packet.push(0x00);

    // Group Address
    packet.extend_from_slice(&group.octets());

    // Calculate and insert checksum
    let checksum = calculate_checksum(&packet);
    packet[2] = (checksum >> 8) as u8;
    packet[3] = (checksum & 0xFF) as u8;

    Ok(packet)
}

/// Build an IGMPv2 Leave Group packet
fn build_igmp_v2_leave(group: Ipv4Addr) -> Result<Vec<u8>> {
    let mut packet = Vec::new();

    // Type: Leave Group (0x17)
    packet.push(0x17);

    // Max Response Time: 0 for leave messages
    packet.push(0x00);

    // Checksum: placeholder (will calculate)
    packet.push(0x00);
    packet.push(0x00);

    // Group Address
    packet.extend_from_slice(&group.octets());

    // Calculate and insert checksum
    let checksum = calculate_checksum(&packet);
    packet[2] = (checksum >> 8) as u8;
    packet[3] = (checksum & 0xFF) as u8;

    Ok(packet)
}

/// Calculate Internet Checksum (RFC 1071)
fn calculate_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;

    // Sum 16-bit words
    while i < data.len() - 1 {
        sum += u32::from(u16::from_be_bytes([data[i], data[i + 1]]));
        i += 2;
    }

    // Add remaining byte if odd length
    if i < data.len() {
        sum += u32::from(data[i]) << 8;
    }

    // Fold 32-bit sum to 16 bits
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    // Return one's complement
    !sum as u16
}

// ============================================================================
// Action Definitions
// ============================================================================

fn join_group_action() -> ActionDefinition {
    ActionDefinition {
        name: "join_group".to_string(),
        description: "Join a multicast group (async action)".to_string(),
        parameters: vec![Parameter {
            name: "group_address".to_string(),
            type_hint: "string".to_string(),
            description: "IPv4 multicast group address (e.g., '239.255.255.250')".to_string(),
            required: true,
        }],
        example: json!({
            "type": "join_group",
            "group_address": "239.255.255.250"
        }),
    }
}

fn leave_group_action() -> ActionDefinition {
    ActionDefinition {
        name: "leave_group".to_string(),
        description: "Leave a multicast group (async action)".to_string(),
        parameters: vec![Parameter {
            name: "group_address".to_string(),
            type_hint: "string".to_string(),
            description: "IPv4 multicast group address to leave".to_string(),
            required: true,
        }],
        example: json!({
            "type": "leave_group",
            "group_address": "239.255.255.250"
        }),
    }
}

fn send_membership_report_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_membership_report".to_string(),
        description: "Send an IGMP Membership Report for a multicast group".to_string(),
        parameters: vec![Parameter {
            name: "group_address".to_string(),
            type_hint: "string".to_string(),
            description: "IPv4 multicast group address".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_membership_report",
            "group_address": "239.255.255.250"
        }),
    }
}

fn send_leave_group_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_leave_group".to_string(),
        description: "Send an IGMP Leave Group message".to_string(),
        parameters: vec![Parameter {
            name: "group_address".to_string(),
            type_hint: "string".to_string(),
            description: "IPv4 multicast group address to leave".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_leave_group",
            "group_address": "239.255.255.250"
        }),
    }
}

fn ignore_message_action() -> ActionDefinition {
    ActionDefinition {
        name: "ignore_message".to_string(),
        description: "Ignore this IGMP message and don't send a response".to_string(),
        parameters: vec![],
        example: json!({
            "type": "ignore_message"
        }),
    }
}

// ============================================================================
// IGMP Event Type Constants
// ============================================================================

pub static IGMP_QUERY_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "igmp_query_received",
        "IGMP Membership Query received",
        json!({
            "type": "send_membership_report",
            "group_address": "239.255.255.250"
        }),
    )
    .with_parameters(vec![
            Parameter {
                name: "query_type".to_string(),
                type_hint: "string".to_string(),
                description: "Type of query (General or Group-Specific)".to_string(),
                required: true,
            },
            Parameter {
                name: "group_address".to_string(),
                type_hint: "string".to_string(),
                description: "Multicast group address (0.0.0.0 for general query)".to_string(),
                required: true,
            },
            Parameter {
                name: "max_response_time".to_string(),
                type_hint: "number".to_string(),
                description: "Maximum response time in deciseconds".to_string(),
                required: false,
            },
        ])
        .with_actions(vec![
            send_membership_report_action(),
            ignore_message_action(),
        ])
});

pub static IGMP_REPORT_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "igmp_report_received",
        "IGMP Membership Report received from another host",
        json!({
            "type": "ignore_message"
        }),
    )
    .with_parameters(vec![Parameter {
        name: "group_address".to_string(),
        type_hint: "string".to_string(),
        description: "Multicast group address being reported".to_string(),
        required: true,
    }])
    .with_actions(vec![ignore_message_action()])
});

pub static IGMP_LEAVE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "igmp_leave_received",
        "IGMP Leave Group message received",
        json!({
            "type": "ignore_message"
        }),
    )
    .with_parameters(vec![Parameter {
            name: "group_address".to_string(),
            type_hint: "string".to_string(),
            description: "Multicast group address being left".to_string(),
            required: true,
        }])
        .with_actions(vec![ignore_message_action()])
});

pub fn get_igmp_event_types() -> Vec<EventType> {
    vec![
        IGMP_QUERY_RECEIVED_EVENT.clone(),
        IGMP_REPORT_RECEIVED_EVENT.clone(),
        IGMP_LEAVE_RECEIVED_EVENT.clone(),
    ]
}
