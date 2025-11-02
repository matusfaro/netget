//! DataLink protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// DataLink protocol action handler
pub struct DataLinkProtocol;

impl DataLinkProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Server for DataLinkProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::datalink::DataLinkServer;

            // DataLink doesn't use SocketAddr, it uses interface name
            // Extract interface and filter from startup_params
            let params = ctx.startup_params
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("DataLink requires startup parameters (interface)"))?;

            let interface = params.get_string("interface");
            let filter = params.get_optional_string("filter");

            // Spawn the datalink server
            let _interface_name = DataLinkServer::spawn_with_llm(
                interface,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                filter,
                ctx.server_id,
            ).await?;

            // DataLink doesn't bind to a socket, so return a dummy address
            // The listen_addr from context is just a placeholder
            Ok(ctx.listen_addr)
        })
    }

    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        vec![
            crate::llm::actions::ParameterDefinition {
                name: "interface".to_string(),
                type_hint: "string".to_string(),
                description: "Network interface name to capture packets from (e.g., 'eth0', 'en0', 'wlan0')".to_string(),
                required: true,
                example: json!("eth0"),
            },
            crate::llm::actions::ParameterDefinition {
                name: "filter".to_string(),
                type_hint: "string".to_string(),
                description: "Optional BPF (Berkeley Packet Filter) expression to filter captured packets (e.g., 'arp', 'tcp port 80')".to_string(),
                required: false,
                example: json!("arp"),
            },
        ]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            show_message_action(),
            ignore_packet_action(),
        ]
    }

    fn execute_action(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "show_message" => {
                // Message actions are handled by the LLM's text response
                // This action just acknowledges the intent
                Ok(ActionResult::NoAction)
            }
            "ignore_packet" => Ok(ActionResult::NoAction),
            _ => Err(anyhow::anyhow!("Unknown DataLink action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "DataLink"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_datalink_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["datalink", "data link", "layer 2", "layer2", "l2", "ethernet", "arp", "pcap"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, ProtocolState, PrivilegeRequirement};

        ProtocolMetadataV2::builder()
            .state(ProtocolState::Beta)
            .privilege_requirement(PrivilegeRequirement::RawSockets)
            .implementation("libpcap (pcap crate) for Layer 2 packet capture")
            .llm_control("Observation only - no packet injection")
            .e2e_testing("libpcap for packet validation")
            .notes("Requires root/CAP_NET_RAW for promiscuous mode")
            .build()
    }

    fn description(&self) -> &'static str {
        "Layer 2 Ethernet frame server"
    }

    fn example_prompt(&self) -> &'static str {
        "Listen on eth0 via Ethernet"
    }

    fn group_name(&self) -> &'static str {
        "Core"
    }
}

/// Action definition for show_message
fn show_message_action() -> ActionDefinition {
    ActionDefinition {
        name: "show_message".to_string(),
        description: "Show a message about the packet analysis".to_string(),
        parameters: vec![Parameter {
            name: "message".to_string(),
            type_hint: "string".to_string(),
            description: "Message to display".to_string(),
            required: true,
        }],
        example: json!({
            "type": "show_message",
            "message": "ARP request detected for 192.168.1.1"
        }),
    }
}

/// Action definition for ignore_packet
fn ignore_packet_action() -> ActionDefinition {
    ActionDefinition {
        name: "ignore_packet".to_string(),
        description: "Ignore this packet (no action taken)".to_string(),
        parameters: vec![],
        example: json!({
            "type": "ignore_packet"
        }),
    }
}

// ============================================================================
// DataLink Event Type Constants
// ============================================================================

pub static DATALINK_PACKET_CAPTURED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "datalink_packet_captured",
        "Layer 2 Ethernet packet captured from network interface"
    )
    .with_parameters(vec![
        Parameter {
            name: "packet_length".to_string(),
            type_hint: "number".to_string(),
            description: "Length of the captured packet in bytes".to_string(),
            required: true,
        },
        Parameter {
            name: "packet_hex".to_string(),
            type_hint: "string".to_string(),
            description: "Hexadecimal representation of the packet data".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![
        show_message_action(),
        ignore_packet_action(),
    ])
});

pub fn get_datalink_event_types() -> Vec<EventType> {
    vec![
        DATALINK_PACKET_CAPTURED_EVENT.clone(),
    ]
}
