//! DataLink protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol},
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

impl Protocol for DataLinkProtocol {
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
