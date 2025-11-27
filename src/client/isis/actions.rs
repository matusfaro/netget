//! IS-IS client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    protocol_trait::Protocol,
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// IS-IS PDU received event
pub static ISIS_PDU_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "isis_pdu_received",
        "IS-IS PDU captured from network interface",
        json!({"type": "wait_for_more"}),
    )
    .with_parameters(vec![
        Parameter {
            name: "pdu_type".to_string(),
            type_hint: "string".to_string(),
            description: "Type of IS-IS PDU (e.g., 'L1 LAN Hello', 'L2 LSP')".to_string(),
            required: true,
        },
        Parameter {
            name: "pdu_type_code".to_string(),
            type_hint: "number".to_string(),
            description: "Numeric PDU type code".to_string(),
            required: true,
        },
        Parameter {
            name: "version".to_string(),
            type_hint: "number".to_string(),
            description: "IS-IS protocol version".to_string(),
            required: true,
        },
        Parameter {
            name: "pdu_length".to_string(),
            type_hint: "number".to_string(),
            description: "Length of the PDU in bytes".to_string(),
            required: true,
        },
        Parameter {
            name: "raw_pdu_hex".to_string(),
            type_hint: "string".to_string(),
            description: "Raw IS-IS PDU data as hex string".to_string(),
            required: true,
        },
        Parameter {
            name: "raw_frame_hex".to_string(),
            type_hint: "string".to_string(),
            description: "Complete Ethernet frame as hex string".to_string(),
            required: true,
        },
    ])
});

/// IS-IS client protocol action handler
pub struct IsisClientProtocol;

impl IsisClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for IsisClientProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "analyze_topology".to_string(),
                description: "Analyze IS-IS topology from captured PDUs".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "analyze_topology"
                }),
            },
            ActionDefinition {
                name: "stop_capture".to_string(),
                description: "Stop capturing IS-IS PDUs".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "stop_capture"
                }),
            },
        ]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![ActionDefinition {
            name: "wait_for_more".to_string(),
            description: "Continue capturing IS-IS PDUs".to_string(),
            parameters: vec![],
            example: json!({
                "type": "wait_for_more"
            }),
        }]
    }
    fn protocol_name(&self) -> &'static str {
        "IS-IS"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![EventType::new("isis_pdu_received", "Triggered when an IS-IS PDU is captured from the network", json!({"type": "wait_for_more"}))]
    }
    fn stack_name(&self) -> &'static str {
        "ETH>LLC/SNAP>IS-IS"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec![
            "isis",
            "is-is",
            "intermediate system",
            "routing protocol",
            "layer 2 routing",
        ]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("pcap for raw packet capture, custom IS-IS PDU parsing")
            .llm_control("Passive capture and analysis of IS-IS topology")
            .e2e_testing("Requires IS-IS router or packet replay")
            .build()
    }
    fn description(&self) -> &'static str {
        "IS-IS client for capturing and analyzing IS-IS routing protocol PDUs"
    }
    fn example_prompt(&self) -> &'static str {
        "Capture IS-IS PDUs on eth0 and analyze network topology"
    }
    fn group_name(&self) -> &'static str {
        "Routing"
    }
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![ParameterDefinition {
            name: "interface".to_string(),
            type_hint: "string".to_string(),
            description: "Network interface to capture IS-IS traffic on (e.g., 'eth0', 'en0')"
                .to_string(),
            required: true,
            example: json!("eth0"),
        }]
    }
}

// Implement Client trait (client-specific functionality)
impl Client for IsisClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::isis::IsisClient;
            IsisClient::connect_with_llm_actions(
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
            "analyze_topology" => {
                // This is a placeholder - the LLM handles analysis in memory
                Ok(ClientActionResult::WaitForMore)
            }
            "stop_capture" => Ok(ClientActionResult::Disconnect),
            "wait_for_more" => Ok(ClientActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!(
                "Unknown IS-IS client action: {}",
                action_type
            )),
        }
    }
}
