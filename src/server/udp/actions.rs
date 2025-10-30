//! UDP protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, ProtocolActions},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::{Arc, LazyLock};
use tokio::net::UdpSocket;

/// UDP protocol action handler
pub struct UdpProtocol {
    /// Shared UDP socket for async actions
    #[allow(dead_code)]
    socket: Option<Arc<UdpSocket>>,
}

impl UdpProtocol {
    pub fn new() -> Self {
        Self { socket: None }
    }

    pub fn with_socket(socket: Arc<UdpSocket>) -> Self {
        Self {
            socket: Some(socket),
        }
    }
}

impl ProtocolActions for UdpProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![send_to_address_action()]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![send_udp_response_action(), ignore_datagram_action()]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "send_to_address" => self.execute_send_to_address(action),
            "send_udp_response" => self.execute_send_udp_response(action),
            "ignore_datagram" => Ok(ActionResult::NoAction),
            _ => Err(anyhow::anyhow!("Unknown UDP action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "UDP"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_udp_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["udp"]
    }

    fn metadata(&self) -> crate::protocol::base_stack::ProtocolMetadata {
        crate::protocol::base_stack::ProtocolMetadata::new(
            crate::protocol::base_stack::ProtocolState::Beta
        )
    }
}

impl UdpProtocol {
    /// Execute send_to_address async action
    fn execute_send_to_address(&self, action: serde_json::Value) -> Result<ActionResult> {
        let address = action
            .get("address")
            .and_then(|v| v.as_str())
            .context("Missing 'address' parameter")?;

        let data = action
            .get("data")
            .and_then(|v| v.as_str())
            .context("Missing 'data' parameter")?;

        let _addr: SocketAddr = address.parse().context("Invalid socket address format")?;

        // For async actions, we need to return the data and let the caller handle sending
        // This is because we need the socket reference from the network handler
        // We'll encode the target address in the result
        Ok(ActionResult::Output(data.as_bytes().to_vec()))
    }

    /// Execute send_udp_response sync action
    fn execute_send_udp_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let data = action
            .get("data")
            .and_then(|v| v.as_str())
            .context("Missing 'data' parameter")?;

        Ok(ActionResult::Output(data.as_bytes().to_vec()))
    }
}

/// Action definition for send_to_address
fn send_to_address_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_to_address".to_string(),
        description: "Send UDP datagram to a specific address (async action)".to_string(),
        parameters: vec![
            Parameter {
                name: "address".to_string(),
                type_hint: "string".to_string(),
                description: "Target address in format 'IP:port' (e.g., '127.0.0.1:8080')"
                    .to_string(),
                required: true,
            },
            Parameter {
                name: "data".to_string(),
                type_hint: "string".to_string(),
                description: "Data to send".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_to_address",
            "address": "127.0.0.1:8080",
            "data": "Hello from UDP"
        }),
    }
}

/// Action definition for send_udp_response
fn send_udp_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_udp_response".to_string(),
        description: "Send UDP response back to the peer that sent the current datagram"
            .to_string(),
        parameters: vec![Parameter {
            name: "data".to_string(),
            type_hint: "string".to_string(),
            description: "Response data to send".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_udp_response",
            "data": "Response data"
        }),
    }
}

/// Action definition for ignore_datagram
fn ignore_datagram_action() -> ActionDefinition {
    ActionDefinition {
        name: "ignore_datagram".to_string(),
        description: "Ignore this datagram and don't send a response".to_string(),
        parameters: vec![],
        example: json!({
            "type": "ignore_datagram"
        }),
    }
}

// ============================================================================
// UDP Event Type Constants
// ============================================================================

pub static UDP_DATAGRAM_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "udp_datagram_received",
        "UDP datagram received from a peer"
    )
    .with_parameters(vec![
        Parameter {
            name: "peer_address".to_string(),
            type_hint: "string".to_string(),
            description: "Source address of the datagram (IP:port)".to_string(),
            required: true,
        },
        Parameter {
            name: "data_length".to_string(),
            type_hint: "number".to_string(),
            description: "Length of the received data in bytes".to_string(),
            required: true,
        },
        Parameter {
            name: "data_preview".to_string(),
            type_hint: "string".to_string(),
            description: "Preview of the received data".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![
        send_udp_response_action(),
        ignore_datagram_action(),
    ])
});

pub fn get_udp_event_types() -> Vec<EventType> {
    vec![
        UDP_DATAGRAM_RECEIVED_EVENT.clone(),
    ]
}
