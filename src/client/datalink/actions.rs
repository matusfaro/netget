//! DataLink client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// DataLink client frame injected event
pub static DATALINK_CLIENT_FRAME_INJECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "datalink_frame_injected",
        "Raw Ethernet frame successfully injected"
    )
    .with_parameters(vec![
        Parameter {
            name: "frame_length".to_string(),
            type_hint: "number".to_string(),
            description: "Length of injected frame in bytes".to_string(),
            required: true,
        },
    ])
});

/// DataLink client frame captured event (for promiscuous mode listening)
pub static DATALINK_CLIENT_FRAME_CAPTURED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "datalink_frame_captured",
        "Raw Ethernet frame captured on interface"
    )
    .with_parameters(vec![
        Parameter {
            name: "frame_hex".to_string(),
            type_hint: "string".to_string(),
            description: "The frame data (as hex string)".to_string(),
            required: true,
        },
        Parameter {
            name: "frame_length".to_string(),
            type_hint: "number".to_string(),
            description: "Length of frame in bytes".to_string(),
            required: true,
        },
    ])
});

/// DataLink client protocol action handler
pub struct DataLinkClientProtocol;

impl DataLinkClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Client for DataLinkClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::datalink::DataLinkClient;
            DataLinkClient::connect_with_llm_actions(
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

    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "interface".to_string(),
                type_hint: "string".to_string(),
                description: "Network interface name for frame injection (e.g., 'eth0', 'en0', 'wlan0')".to_string(),
                required: true,
                example: json!("eth0"),
            },
            ParameterDefinition {
                name: "promiscuous".to_string(),
                type_hint: "boolean".to_string(),
                description: "Enable promiscuous mode to capture frames (requires root/CAP_NET_RAW)".to_string(),
                required: false,
                example: json!(false),
            },
        ]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "inject_frame".to_string(),
                description: "Inject a raw Ethernet frame onto the network".to_string(),
                parameters: vec![
                    Parameter {
                        name: "frame_hex".to_string(),
                        type_hint: "string".to_string(),
                        description: "Hexadecimal encoded Ethernet frame (including dst MAC, src MAC, ethertype, payload, FCS)".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "inject_frame",
                    "frame_hex": "ffffffffffff001122334455080600010800060400010011223344550a0000010000000000000a000002"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Close the DataLink client and release the interface".to_string(),
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
                name: "inject_frame".to_string(),
                description: "Inject frame in response to captured frame".to_string(),
                parameters: vec![
                    Parameter {
                        name: "frame_hex".to_string(),
                        type_hint: "string".to_string(),
                        description: "Hexadecimal encoded Ethernet frame".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "inject_frame",
                    "frame_hex": "ffffffffffff001122334455080600010800060400010011223344550a0000010000000000000a000002"
                }),
            },
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait for more frames before responding".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "wait_for_more"
                }),
            },
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "inject_frame" => {
                let frame_hex = action
                    .get("frame_hex")
                    .and_then(|v| v.as_str())
                    .context("Missing 'frame_hex' field")?;

                let frame = hex::decode(frame_hex)
                    .context("Invalid hex frame data")?;

                Ok(ClientActionResult::SendData(frame))
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            "wait_for_more" => Ok(ClientActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!("Unknown DataLink client action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "DataLink"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType {
                id: "datalink_frame_injected".to_string(),
                description: "Triggered when frame is successfully injected".to_string(),
                actions: vec![],
                parameters: vec![],
            },
            EventType {
                id: "datalink_frame_captured".to_string(),
                description: "Triggered when frame is captured in promiscuous mode".to_string(),
                actions: vec![],
                parameters: vec![],
            },
        ]
    }

    fn stack_name(&self) -> &'static str {
        "ETH"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["datalink", "data link", "layer 2", "layer2", "l2", "ethernet", "frame", "inject", "pcap"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, PrivilegeRequirement, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .privilege_requirement(PrivilegeRequirement::RawSockets)
            .implementation("libpcap (pcap crate) for Layer 2 frame injection and capture")
            .llm_control("Full control over Ethernet frames (inject/capture)")
            .e2e_testing("libpcap for frame validation")
            .notes("Requires root/CAP_NET_RAW for frame injection and promiscuous mode")
            .build()
    }

    fn description(&self) -> &'static str {
        "DataLink client for raw Ethernet frame injection"
    }

    fn example_prompt(&self) -> &'static str {
        "Inject ARP request on eth0"
    }

    fn group_name(&self) -> &'static str {
        "Core"
    }
}
