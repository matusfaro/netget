//! OSPF protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::log_template::LogTemplate;
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;
use tracing::debug;

/// OSPF protocol action handler
pub struct OspfProtocol;

impl OspfProtocol {
    pub fn new() -> Self {
        Self
    }

    fn execute_send_hello(&self, action: serde_json::Value) -> Result<ActionResult> {
        let router_id = action
            .get("router_id")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");

        let area_id = action
            .get("area_id")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");

        let _network_mask = action
            .get("network_mask")
            .and_then(|v| v.as_str())
            .unwrap_or("255.255.255.0");

        let _hello_interval = action
            .get("hello_interval")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as u16;

        let _router_dead_interval = action
            .get("router_dead_interval")
            .and_then(|v| v.as_u64())
            .unwrap_or(40) as u32;

        let priority = action.get("priority").and_then(|v| v.as_u64()).unwrap_or(1) as u8;

        let _dr = action
            .get("dr")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");

        let _bdr = action
            .get("bdr")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");

        let destination = action
            .get("destination")
            .and_then(|v| v.as_str())
            .unwrap_or("multicast")
            .to_string();

        debug!(
            "OSPF sending Hello: router_id={}, area={}, priority={}, dest={}",
            router_id, area_id, priority, destination
        );

        // Return structured action data - packet will be built in mod.rs
        Ok(ActionResult::Custom {
            name: "ospf_action".to_string(),
            data: action.clone(),
        })
    }

    fn execute_send_database_description(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _router_id = action
            .get("router_id")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");

        let _area_id = action
            .get("area_id")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");

        let sequence = action.get("sequence").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

        let init = action
            .get("init")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let more = action
            .get("more")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let master = action
            .get("master")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let destination = action
            .get("destination")
            .and_then(|v| v.as_str())
            .unwrap_or("multicast")
            .to_string();

        debug!(
            "OSPF sending Database Description: seq={}, init={}, more={}, master={}, dest={}",
            sequence, init, more, master, destination
        );

        // Return structured action data - packet will be built in mod.rs
        Ok(ActionResult::Custom {
            name: "ospf_action".to_string(),
            data: action.clone(),
        })
    }

    fn execute_send_link_state_request(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _router_id = action
            .get("router_id")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");

        let _area_id = action
            .get("area_id")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");

        let destination = action
            .get("destination")
            .and_then(|v| v.as_str())
            .unwrap_or("multicast")
            .to_string();

        debug!("OSPF sending Link State Request to {}", destination);

        // Return structured action data - packet will be built in mod.rs
        Ok(ActionResult::Custom {
            name: "ospf_action".to_string(),
            data: action.clone(),
        })
    }

    fn execute_send_link_state_update(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _router_id = action
            .get("router_id")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");

        let _area_id = action
            .get("area_id")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");

        let destination = action
            .get("destination")
            .and_then(|v| v.as_str())
            .unwrap_or("multicast")
            .to_string();

        debug!("OSPF sending Link State Update to {}", destination);

        // Return structured action data - packet will be built in mod.rs
        Ok(ActionResult::Custom {
            name: "ospf_action".to_string(),
            data: action.clone(),
        })
    }

    fn execute_send_link_state_ack(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _router_id = action
            .get("router_id")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");

        let _area_id = action
            .get("area_id")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");

        let destination = action
            .get("destination")
            .and_then(|v| v.as_str())
            .unwrap_or("multicast")
            .to_string();

        debug!("OSPF sending Link State Acknowledgment to {}", destination);

        // Return structured action data - packet will be built in mod.rs
        Ok(ActionResult::Custom {
            name: "ospf_action".to_string(),
            data: action.clone(),
        })
    }

    // Helper: Parse IPv4 address string to bytes
    fn parse_ipv4(ip: &str) -> Result<[u8; 4]> {
        let parts: Vec<u8> = ip.split('.').filter_map(|s| s.parse::<u8>().ok()).collect();

        if parts.len() == 4 {
            Ok([parts[0], parts[1], parts[2], parts[3]])
        } else {
            Ok([0, 0, 0, 0])
        }
    }

    // Helper: Calculate OSPF checksum
    fn calculate_checksum(data: &[u8]) -> u16 {
        // Fletcher checksum for OSPF (RFC 2328 Section D.4)
        // Simplified implementation - should skip authentication field
        let mut c0: u32 = 0;
        let mut c1: u32 = 0;

        // Start after first 2 bytes (version and type), skip checksum field
        for (i, &byte) in data.iter().enumerate() {
            if i >= 2 && i < 12 || i >= 14 {
                c0 = (c0 + byte as u32) % 255;
                c1 = (c1 + c0) % 255;
            }
        }

        let x = (((data.len() - 14) as u32 * c0) - c1) % 255;
        let y = (510 - c0 - x) % 255;

        ((x as u16) << 8) | (y as u16)
    }

    /// Build OSPF Hello packet from action data
    pub fn build_hello_packet(action: &serde_json::Value) -> Result<Vec<u8>> {
        let router_id = action
            .get("router_id")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");
        let area_id = action
            .get("area_id")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");
        let network_mask = action
            .get("network_mask")
            .and_then(|v| v.as_str())
            .unwrap_or("255.255.255.0");
        let hello_interval = action
            .get("hello_interval")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as u16;
        let router_dead_interval = action
            .get("router_dead_interval")
            .and_then(|v| v.as_u64())
            .unwrap_or(40) as u32;
        let priority = action.get("priority").and_then(|v| v.as_u64()).unwrap_or(1) as u8;
        let dr = action
            .get("dr")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");
        let bdr = action
            .get("bdr")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");

        let mut msg = Vec::new();

        // OSPF Header (24 bytes)
        msg.push(2); // Version = 2 (OSPFv2)
        msg.push(1); // Type = 1 (Hello)
        msg.extend_from_slice(&[0, 0]); // Packet Length (placeholder)
        msg.extend_from_slice(&Self::parse_ipv4(router_id)?);
        msg.extend_from_slice(&Self::parse_ipv4(area_id)?);
        msg.extend_from_slice(&[0, 0]); // Checksum (placeholder)
        msg.extend_from_slice(&[0, 0]); // AuType = 0 (no authentication)
        msg.extend_from_slice(&[0; 8]); // Authentication (8 bytes, zeros)

        // Hello packet body
        msg.extend_from_slice(&Self::parse_ipv4(network_mask)?);
        msg.extend_from_slice(&hello_interval.to_be_bytes());
        msg.push(0); // Options
        msg.push(priority);
        msg.extend_from_slice(&router_dead_interval.to_be_bytes());
        msg.extend_from_slice(&Self::parse_ipv4(dr)?);
        msg.extend_from_slice(&Self::parse_ipv4(bdr)?);

        // Neighbor list
        if let Some(neighbors) = action.get("neighbors").and_then(|v| v.as_array()) {
            for neighbor in neighbors {
                if let Some(neighbor_id) = neighbor.as_str() {
                    msg.extend_from_slice(&Self::parse_ipv4(neighbor_id)?);
                }
            }
        }

        // Update packet length and checksum
        let packet_len = msg.len() as u16;
        msg[2..4].copy_from_slice(&packet_len.to_be_bytes());
        let checksum = Self::calculate_checksum(&msg);
        msg[12..14].copy_from_slice(&checksum.to_be_bytes());

        Ok(msg)
    }

    /// Build OSPF Database Description packet from action data
    pub fn build_database_description_packet(action: &serde_json::Value) -> Result<Vec<u8>> {
        let router_id = action
            .get("router_id")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");
        let area_id = action
            .get("area_id")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");
        let sequence = action.get("sequence").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let init = action
            .get("init")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let more = action
            .get("more")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let master = action
            .get("master")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut msg = Vec::new();

        // OSPF Header
        msg.push(2); // Version
        msg.push(2); // Type = 2 (Database Description)
        msg.extend_from_slice(&[0, 0]); // Packet Length (placeholder)
        msg.extend_from_slice(&Self::parse_ipv4(router_id)?);
        msg.extend_from_slice(&Self::parse_ipv4(area_id)?);
        msg.extend_from_slice(&[0, 0]); // Checksum (placeholder)
        msg.extend_from_slice(&[0, 0]); // AuType
        msg.extend_from_slice(&[0; 8]); // Authentication

        // DD packet body
        msg.extend_from_slice(&[0, 0]); // Interface MTU
        msg.push(0); // Options
        let mut flags: u8 = 0;
        if init {
            flags |= 0x04;
        }
        if more {
            flags |= 0x02;
        }
        if master {
            flags |= 0x01;
        }
        msg.push(flags);
        msg.extend_from_slice(&sequence.to_be_bytes());

        // LSA headers would go here (simplified - empty for now)

        // Update packet length and checksum
        let packet_len = msg.len() as u16;
        msg[2..4].copy_from_slice(&packet_len.to_be_bytes());
        let checksum = Self::calculate_checksum(&msg);
        msg[12..14].copy_from_slice(&checksum.to_be_bytes());

        Ok(msg)
    }

    /// Build OSPF Link State Request packet from action data
    pub fn build_link_state_request_packet(action: &serde_json::Value) -> Result<Vec<u8>> {
        let router_id = action
            .get("router_id")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");
        let area_id = action
            .get("area_id")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");

        let mut msg = Vec::new();

        // OSPF Header
        msg.push(2); // Version
        msg.push(3); // Type = 3 (Link State Request)
        msg.extend_from_slice(&[0, 0]); // Packet Length (placeholder)
        msg.extend_from_slice(&Self::parse_ipv4(router_id)?);
        msg.extend_from_slice(&Self::parse_ipv4(area_id)?);
        msg.extend_from_slice(&[0, 0]); // Checksum (placeholder)
        msg.extend_from_slice(&[0, 0]); // AuType
        msg.extend_from_slice(&[0; 8]); // Authentication

        // LSR body (simplified - would contain list of requested LSAs)

        // Update packet length and checksum
        let packet_len = msg.len() as u16;
        msg[2..4].copy_from_slice(&packet_len.to_be_bytes());
        let checksum = Self::calculate_checksum(&msg);
        msg[12..14].copy_from_slice(&checksum.to_be_bytes());

        Ok(msg)
    }

    /// Build OSPF Link State Update packet from action data
    pub fn build_link_state_update_packet(action: &serde_json::Value) -> Result<Vec<u8>> {
        let router_id = action
            .get("router_id")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");
        let area_id = action
            .get("area_id")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");

        let mut msg = Vec::new();

        // OSPF Header
        msg.push(2); // Version
        msg.push(4); // Type = 4 (Link State Update)
        msg.extend_from_slice(&[0, 0]); // Packet Length (placeholder)
        msg.extend_from_slice(&Self::parse_ipv4(router_id)?);
        msg.extend_from_slice(&Self::parse_ipv4(area_id)?);
        msg.extend_from_slice(&[0, 0]); // Checksum (placeholder)
        msg.extend_from_slice(&[0, 0]); // AuType
        msg.extend_from_slice(&[0; 8]); // Authentication

        // Number of LSAs
        msg.extend_from_slice(&[0, 0, 0, 0]); // 0 LSAs (simplified)

        // LSAs would go here

        // Update packet length and checksum
        let packet_len = msg.len() as u16;
        msg[2..4].copy_from_slice(&packet_len.to_be_bytes());
        let checksum = Self::calculate_checksum(&msg);
        msg[12..14].copy_from_slice(&checksum.to_be_bytes());

        Ok(msg)
    }

    /// Build OSPF Link State Acknowledgment packet from action data
    pub fn build_link_state_ack_packet(action: &serde_json::Value) -> Result<Vec<u8>> {
        let router_id = action
            .get("router_id")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");
        let area_id = action
            .get("area_id")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");

        let mut msg = Vec::new();

        // OSPF Header
        msg.push(2); // Version
        msg.push(5); // Type = 5 (Link State Acknowledgment)
        msg.extend_from_slice(&[0, 0]); // Packet Length (placeholder)
        msg.extend_from_slice(&Self::parse_ipv4(router_id)?);
        msg.extend_from_slice(&Self::parse_ipv4(area_id)?);
        msg.extend_from_slice(&[0, 0]); // Checksum (placeholder)
        msg.extend_from_slice(&[0, 0]); // AuType
        msg.extend_from_slice(&[0; 8]); // Authentication

        // LSA headers to acknowledge would go here

        // Update packet length and checksum
        let packet_len = msg.len() as u16;
        msg[2..4].copy_from_slice(&packet_len.to_be_bytes());
        let checksum = Self::calculate_checksum(&msg);
        msg[12..14].copy_from_slice(&checksum.to_be_bytes());

        Ok(msg)
    }
}

// Event types for OSPF
pub static OSPF_HELLO_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("ospf_hello", "OSPF Hello packet received from neighbor", json!({"type": "placeholder", "event_id": "ospf_hello"}))
        .with_log_template(
            LogTemplate::new()
                .with_info("OSPF Hello from {neighbor_id}")
                .with_debug("OSPF Hello: neighbor={neighbor_id} area={area_id} priority={router_priority}")
                .with_trace("OSPF Hello: {json_pretty(.)}"),
        )
});

pub static OSPF_DATABASE_DESCRIPTION_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("ospf_database_description", "OSPF Database Description packet received", json!({"type": "placeholder", "event_id": "ospf_database_description"}))
        .with_log_template(
            LogTemplate::new()
                .with_info("OSPF DD from {neighbor_id}")
                .with_debug("OSPF Database Description: neighbor={neighbor_id}")
                .with_trace("OSPF DD: {json_pretty(.)}"),
        )
});

pub static OSPF_LINK_STATE_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("ospf_link_state_request", "OSPF Link State Request packet received", json!({"type": "placeholder", "event_id": "ospf_link_state_request"}))
        .with_log_template(
            LogTemplate::new()
                .with_info("OSPF LSR from {neighbor_id}")
                .with_debug("OSPF Link State Request: neighbor={neighbor_id}")
                .with_trace("OSPF LSR: {json_pretty(.)}"),
        )
});

pub static OSPF_LINK_STATE_UPDATE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("ospf_link_state_update", "OSPF Link State Update packet received", json!({"type": "placeholder", "event_id": "ospf_link_state_update"}))
        .with_log_template(
            LogTemplate::new()
                .with_info("OSPF LSU from {neighbor_id}")
                .with_debug("OSPF Link State Update: neighbor={neighbor_id}")
                .with_trace("OSPF LSU: {json_pretty(.)}"),
        )
});

pub static OSPF_LINK_STATE_ACK_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("ospf_link_state_ack", "OSPF Link State Acknowledgment packet received", json!({"type": "placeholder", "event_id": "ospf_link_state_ack"}))
        .with_log_template(
            LogTemplate::new()
                .with_info("OSPF LSAck from {neighbor_id}")
                .with_debug("OSPF Link State Acknowledgment: neighbor={neighbor_id}")
                .with_trace("OSPF LSAck: {json_pretty(.)}"),
        )
});

// Implement Protocol trait (common functionality)
impl Protocol for OspfProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "list_neighbors".to_string(),
                description: "List all OSPF neighbors and their states".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "list_neighbors"
                }),
                log_template: Some(
                    LogTemplate::new()
                        .with_info("-> OSPF list neighbors")
                        .with_debug("OSPF list_neighbors"),
                ),
            },
            ActionDefinition {
                name: "list_lsdb".to_string(),
                description: "List Link State Database entries".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "list_lsdb"
                }),
                log_template: Some(
                    LogTemplate::new()
                        .with_info("-> OSPF list LSDB")
                        .with_debug("OSPF list_lsdb"),
                ),
            },
        ]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
                ActionDefinition {
                    name: "send_hello".to_string(),
                    description: "Send OSPF Hello packet to discover/maintain neighbors".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "router_id".to_string(),
                            type_hint: "string".to_string(),
                            description: "OSPF router ID (IPv4 format)".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "area_id".to_string(),
                            type_hint: "string".to_string(),
                            description: "OSPF area ID (IPv4 format, 0.0.0.0 = backbone)".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "network_mask".to_string(),
                            type_hint: "string".to_string(),
                            description: "Network mask (e.g., 255.255.255.0)".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "hello_interval".to_string(),
                            type_hint: "number".to_string(),
                            description: "Hello interval in seconds (default 10)".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "router_dead_interval".to_string(),
                            type_hint: "number".to_string(),
                            description: "Router dead interval in seconds (default 40)".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "priority".to_string(),
                            type_hint: "number".to_string(),
                            description: "Router priority for DR election (0-255, default 1)".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "dr".to_string(),
                            type_hint: "string".to_string(),
                            description: "Designated Router IP (0.0.0.0 if none)".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "bdr".to_string(),
                            type_hint: "string".to_string(),
                            description: "Backup Designated Router IP (0.0.0.0 if none)".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "neighbors".to_string(),
                            type_hint: "array".to_string(),
                            description: "List of neighbor router IDs".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "destination".to_string(),
                            type_hint: "string".to_string(),
                            description: "Destination IP: 'multicast' (default, 224.0.0.5), 'dr_multicast' (224.0.0.6), or unicast IP".to_string(),
                            required: false,
                        },
                    ],
                    example: json!({
                        "type": "send_hello",
                        "router_id": "1.1.1.1",
                        "area_id": "0.0.0.0",
                        "priority": 1,
                        "neighbors": ["2.2.2.2"],
                        "destination": "multicast"
                    }),
                    log_template: Some(
                        LogTemplate::new()
                            .with_info("-> OSPF Hello router={router_id} area={area_id}")
                            .with_debug("OSPF send_hello: router_id={router_id} area={area_id} priority={priority}"),
                    ),
                },
                ActionDefinition {
                    name: "send_database_description".to_string(),
                    description: "Send OSPF Database Description packet".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "router_id".to_string(),
                            type_hint: "string".to_string(),
                            description: "OSPF router ID".to_string(),
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
                            description: "Init flag (true for first DD packet)".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "more".to_string(),
                            type_hint: "boolean".to_string(),
                            description: "More flag (true if more DD packets follow)".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "master".to_string(),
                            type_hint: "boolean".to_string(),
                            description: "Master flag (true if this router is master)".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "destination".to_string(),
                            type_hint: "string".to_string(),
                            description: "Destination IP: 'multicast' (default) or unicast IP".to_string(),
                            required: false,
                        },
                    ],
                    example: json!({
                        "type": "send_database_description",
                        "router_id": "1.1.1.1",
                        "area_id": "0.0.0.0",
                        "sequence": 1,
                        "init": true,
                        "master": true,
                        "destination": "192.168.1.2"
                    }),
                    log_template: Some(
                        LogTemplate::new()
                            .with_info("-> OSPF DD seq={sequence}")
                            .with_debug("OSPF send_database_description: router_id={router_id} seq={sequence} init={init}"),
                    ),
                },
                ActionDefinition {
                    name: "send_link_state_request".to_string(),
                    description: "Send OSPF Link State Request packet".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "router_id".to_string(),
                            type_hint: "string".to_string(),
                            description: "OSPF router ID".to_string(),
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
                            description: "Destination IP: 'multicast' (default) or unicast IP".to_string(),
                            required: false,
                        },
                    ],
                    example: json!({
                        "type": "send_link_state_request",
                        "router_id": "1.1.1.1",
                        "area_id": "0.0.0.0",
                        "destination": "192.168.1.2"
                    }),
                    log_template: Some(
                        LogTemplate::new()
                            .with_info("-> OSPF LSR to {destination}")
                            .with_debug("OSPF send_link_state_request: router_id={router_id} dest={destination}"),
                    ),
                },
                ActionDefinition {
                    name: "send_link_state_update".to_string(),
                    description: "Send OSPF Link State Update packet".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "router_id".to_string(),
                            type_hint: "string".to_string(),
                            description: "OSPF router ID".to_string(),
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
                            description: "Destination IP: 'multicast' (default) or unicast IP".to_string(),
                            required: false,
                        },
                    ],
                    example: json!({
                        "type": "send_link_state_update",
                        "router_id": "1.1.1.1",
                        "area_id": "0.0.0.0",
                        "destination": "multicast"
                    }),
                    log_template: Some(
                        LogTemplate::new()
                            .with_info("-> OSPF LSU to {destination}")
                            .with_debug("OSPF send_link_state_update: router_id={router_id} dest={destination}"),
                    ),
                },
                ActionDefinition {
                    name: "send_link_state_ack".to_string(),
                    description: "Send OSPF Link State Acknowledgment packet".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "router_id".to_string(),
                            type_hint: "string".to_string(),
                            description: "OSPF router ID".to_string(),
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
                            description: "Destination IP: 'multicast' (default) or unicast IP".to_string(),
                            required: false,
                        },
                    ],
                    example: json!({
                        "type": "send_link_state_ack",
                        "router_id": "1.1.1.1",
                        "area_id": "0.0.0.0",
                        "destination": "192.168.1.2"
                    }),
                    log_template: Some(
                        LogTemplate::new()
                            .with_info("-> OSPF LSAck to {destination}")
                            .with_debug("OSPF send_link_state_ack: router_id={router_id} dest={destination}"),
                    ),
                },
                ActionDefinition {
                    name: "wait_for_more".to_string(),
                    description: "Wait for more OSPF packets before responding".to_string(),
                    parameters: vec![],
                    example: json!({
                        "type": "wait_for_more"
                    }),
                    log_template: Some(
                        LogTemplate::new()
                            .with_info("-> OSPF wait for more")
                            .with_debug("OSPF wait_for_more"),
                    ),
                },
            ]
    }
    fn protocol_name(&self) -> &'static str {
        "OSPF"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            OSPF_HELLO_EVENT.clone(),
            OSPF_DATABASE_DESCRIPTION_EVENT.clone(),
            OSPF_LINK_STATE_REQUEST_EVENT.clone(),
            OSPF_LINK_STATE_UPDATE_EVENT.clone(),
            OSPF_LINK_STATE_ACK_EVENT.clone(),
        ]
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP(89)>OSPF"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["ospf", "open shortest path first"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{
            DevelopmentState, PrivilegeRequirement, ProtocolMetadataV2,
        };

        ProtocolMetadataV2::builder()
                .state(DevelopmentState::Experimental)
                .privilege_requirement(PrivilegeRequirement::Root)
                .implementation("Manual OSPFv2 (RFC 2328), IP protocol 89, raw sockets")
                .llm_control("Neighbor states, Hello protocol, packet generation")
                .e2e_testing("Integration with real OSPF routers (FRR, BIRD)")
                .notes("Requires root for raw sockets. TODO: DR/BDR election, SPF calculation, routing table, LSA flooding")
                .build()
    }
    fn description(&self) -> &'static str {
        "OSPF routing protocol server"
    }
    fn example_prompt(&self) -> &'static str {
        "Start an OSPF server on interface 192.168.1.100 as router 1.1.1.1 in area 0.0.0.0"
    }
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "router_id".to_string(),
                type_hint: "string".to_string(),
                description: "OSPF router ID in IPv4 address format (e.g., 1.1.1.1)".to_string(),
                required: false,
                example: json!("1.1.1.1"),
            },
            ParameterDefinition {
                name: "area_id".to_string(),
                type_hint: "string".to_string(),
                description: "OSPF area ID in IPv4 format (0.0.0.0 = backbone area)".to_string(),
                required: false,
                example: json!("0.0.0.0"),
            },
            ParameterDefinition {
                name: "network_mask".to_string(),
                type_hint: "string".to_string(),
                description: "Network mask (e.g., 255.255.255.0)".to_string(),
                required: false,
                example: json!("255.255.255.0"),
            },
            ParameterDefinition {
                name: "hello_interval".to_string(),
                type_hint: "integer".to_string(),
                description: "Hello packet interval in seconds (default 10)".to_string(),
                required: false,
                example: json!(10),
            },
            ParameterDefinition {
                name: "router_dead_interval".to_string(),
                type_hint: "integer".to_string(),
                description: "Router dead interval in seconds (default 40)".to_string(),
                required: false,
                example: json!(40),
            },
            ParameterDefinition {
                name: "router_priority".to_string(),
                type_hint: "integer".to_string(),
                description: "Router priority for DR election (0-255, default 1)".to_string(),
                required: false,
                example: json!(1),
            },
        ]
    }
    fn group_name(&self) -> &'static str {
        "VPN & Routing"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        StartupExamples::new(
            // LLM mode: LLM simulates OSPF router behavior
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "ospf",
                "instruction": "OSPF router with ID 192.168.1.1 in area 0. Respond to Hello packets from neighbors. Claim DR role with priority 100."
            }),
            // Script mode: Scripted OSPF Hello response
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "ospf",
                "event_handlers": [{
                    "event_pattern": "ospf_hello",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "return {type='send_hello', router_id='192.168.1.1', area_id='0.0.0.0', network_mask='255.255.255.0', priority=100, dr='192.168.1.1', bdr='0.0.0.0', neighbors={event.neighbor_id}}"
                    }
                }]
            }),
            // Static mode: Fixed OSPF Hello response
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "ospf",
                "event_handlers": [{
                    "event_pattern": "ospf_hello",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "send_hello",
                            "router_id": "192.168.1.1",
                            "area_id": "0.0.0.0",
                            "network_mask": "255.255.255.0",
                            "priority": 1,
                            "dr": "0.0.0.0",
                            "bdr": "0.0.0.0"
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for OspfProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::ospf::OspfServer;
            OspfServer::spawn_with_llm_actions(
                ctx.legacy_listen_addr(),
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
                ctx.startup_params,
            )
            .await
        })
    }
    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing action type")?;

        match action_type {
            "send_hello" => self.execute_send_hello(action),
            "send_database_description" => self.execute_send_database_description(action),
            "send_link_state_request" => self.execute_send_link_state_request(action),
            "send_link_state_update" => self.execute_send_link_state_update(action),
            "send_link_state_ack" => self.execute_send_link_state_ack(action),
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!("Unknown OSPF action type: {}", action_type)),
        }
    }
}
