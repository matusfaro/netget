//! ARP protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// ARP protocol action handler
pub struct ArpProtocol;

impl ArpProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for ArpProtocol {
    fn default_binding(&self) -> Option<crate::protocol::BindingDefaults> {
        // ARP uses interface-based binding (loopback by default)
        Some(crate::protocol::BindingDefaults::interface_based("lo"))
    }

    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        // Interface is now provided via flexible binding system, not startup params
        vec![]
    }
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![send_arp_reply_action(), ignore_arp_action()]
    }
    fn protocol_name(&self) -> &'static str {
        "ARP"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_arp_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>ARP"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["arp", "address resolution"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{
            DevelopmentState, PrivilegeRequirement, ProtocolMetadataV2,
        };

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .privilege_requirement(PrivilegeRequirement::RawSockets)
            .implementation("libpcap (pcap crate) + pnet for ARP packet handling")
            .llm_control("Full control - can respond to ARP requests with custom MAC addresses")
            .e2e_testing("pnet for packet crafting and validation")
            .notes("Requires root/CAP_NET_RAW for promiscuous mode and packet injection")
            .build()
    }
    fn description(&self) -> &'static str {
        "ARP (Address Resolution Protocol) server"
    }
    fn example_prompt(&self) -> &'static str {
        "Listen for ARP requests on eth0"
    }
    fn group_name(&self) -> &'static str {
        "Core"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for ArpProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::arp::ArpServer;

            // ARP uses interface-based binding
            // Extract interface from context (defaults already applied)
            let interface = ctx
                .interface()
                .context("ARP requires network interface")?
                .to_string();

            // Spawn the ARP server
            let _interface_name = ArpServer::spawn_with_llm(
                interface,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            )
            .await?;

            // ARP doesn't bind to a socket, so return a dummy address
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
            "send_arp_reply" => self.execute_send_arp_reply(action),
            "ignore_arp" => Ok(ActionResult::NoAction),
            _ => Err(anyhow::anyhow!("Unknown ARP action: {}", action_type)),
        }
    }
}

impl ArpProtocol {
    /// Execute send_arp_reply action
    fn execute_send_arp_reply(&self, action: serde_json::Value) -> Result<ActionResult> {
        let sender_mac = action
            .get("sender_mac")
            .and_then(|v| v.as_str())
            .context("Missing 'sender_mac' parameter")?;

        let sender_ip = action
            .get("sender_ip")
            .and_then(|v| v.as_str())
            .context("Missing 'sender_ip' parameter")?;

        let target_mac = action
            .get("target_mac")
            .and_then(|v| v.as_str())
            .context("Missing 'target_mac' parameter")?;

        let target_ip = action
            .get("target_ip")
            .and_then(|v| v.as_str())
            .context("Missing 'target_ip' parameter")?;

        // Parse MAC addresses
        let sender_mac_parsed =
            parse_mac_address(sender_mac).context("Invalid sender_mac format")?;
        let target_mac_parsed =
            parse_mac_address(target_mac).context("Invalid target_mac format")?;

        // Parse IP addresses
        let sender_ip_parsed: std::net::Ipv4Addr =
            sender_ip.parse().context("Invalid sender_ip format")?;
        let target_ip_parsed: std::net::Ipv4Addr =
            target_ip.parse().context("Invalid target_ip format")?;

        // Build ARP reply packet
        use crate::server::arp::ArpServer;
        let packet = ArpServer::build_arp_reply(
            sender_mac_parsed,
            sender_ip_parsed,
            target_mac_parsed,
            target_ip_parsed,
        );

        Ok(ActionResult::Output(packet))
    }
}

/// Parse a MAC address string (e.g., "aa:bb:cc:dd:ee:ff") into pnet MacAddr
fn parse_mac_address(mac_str: &str) -> Result<pnet::util::MacAddr> {
    let parts: Vec<&str> = mac_str.split(':').collect();
    if parts.len() != 6 {
        return Err(anyhow::anyhow!(
            "MAC address must have 6 octets separated by colons"
        ));
    }

    let octets: Result<Vec<u8>> = parts
        .iter()
        .map(|s| u8::from_str_radix(s, 16).context("Invalid hex in MAC address"))
        .collect();

    let octets = octets?;
    Ok(pnet::util::MacAddr::new(
        octets[0], octets[1], octets[2], octets[3], octets[4], octets[5],
    ))
}

/// Action definition for send_arp_reply
fn send_arp_reply_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_arp_reply".to_string(),
        description: "Send an ARP reply packet in response to an ARP request".to_string(),
        parameters: vec![
            Parameter {
                name: "sender_mac".to_string(),
                type_hint: "string".to_string(),
                description: "MAC address to send from (format: aa:bb:cc:dd:ee:ff)".to_string(),
                required: true,
            },
            Parameter {
                name: "sender_ip".to_string(),
                type_hint: "string".to_string(),
                description: "IP address to send from (format: X.X.X.X)".to_string(),
                required: true,
            },
            Parameter {
                name: "target_mac".to_string(),
                type_hint: "string".to_string(),
                description: "Target MAC address (format: aa:bb:cc:dd:ee:ff)".to_string(),
                required: true,
            },
            Parameter {
                name: "target_ip".to_string(),
                type_hint: "string".to_string(),
                description: "Target IP address (format: X.X.X.X)".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_arp_reply",
            "sender_mac": "aa:bb:cc:dd:ee:ff",
            "sender_ip": "192.168.1.100",
            "target_mac": "11:22:33:44:55:66",
            "target_ip": "192.168.1.1"
        }),
    }
}

/// Action definition for ignore_arp
fn ignore_arp_action() -> ActionDefinition {
    ActionDefinition {
        name: "ignore_arp".to_string(),
        description: "Ignore this ARP packet (no action taken)".to_string(),
        parameters: vec![],
        example: json!({
            "type": "ignore_arp"
        }),
    }
}

// ============================================================================
// ARP Event Type Constants
// ============================================================================

pub static ARP_REQUEST_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "arp_request_received",
        "ARP request or reply packet received from network interface",
    )
    .with_parameters(vec![
        Parameter {
            name: "operation".to_string(),
            type_hint: "string".to_string(),
            description: "ARP operation (REQUEST or REPLY)".to_string(),
            required: true,
        },
        Parameter {
            name: "sender_mac".to_string(),
            type_hint: "string".to_string(),
            description: "Sender MAC address".to_string(),
            required: true,
        },
        Parameter {
            name: "sender_ip".to_string(),
            type_hint: "string".to_string(),
            description: "Sender IP address".to_string(),
            required: true,
        },
        Parameter {
            name: "target_mac".to_string(),
            type_hint: "string".to_string(),
            description: "Target MAC address".to_string(),
            required: true,
        },
        Parameter {
            name: "target_ip".to_string(),
            type_hint: "string".to_string(),
            description: "Target IP address".to_string(),
            required: true,
        },
        Parameter {
            name: "packet_hex".to_string(),
            type_hint: "string".to_string(),
            description: "Hexadecimal representation of the full packet data".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![send_arp_reply_action(), ignore_arp_action()])
});

pub fn get_arp_event_types() -> Vec<EventType> {
    vec![ARP_REQUEST_RECEIVED_EVENT.clone()]
}
