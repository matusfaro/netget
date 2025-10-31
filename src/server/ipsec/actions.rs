//! IPSec/IKEv2 protocol actions implementation
//!
//! Defines LLM-controllable actions for IPSec/IKEv2 honeypot

use crate::llm::actions::{
    protocol_trait::{ActionResult, ProtocolActions},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// IPSec/IKEv2 handshake initiation event
pub static IPSEC_HANDSHAKE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "ipsec_handshake",
        "IPSec/IKEv2 client initiated handshake",
    )
});

/// IPSec/IKEv2 data packet event
pub static IPSEC_DATA_EVENT: LazyLock<EventType> =
    LazyLock::new(|| EventType::new("ipsec_data", "IPSec encrypted data packet received"));

/// Get all IPSec event types
pub fn get_ipsec_event_types() -> Vec<EventType> {
    vec![
        IPSEC_HANDSHAKE_EVENT.clone(),
        IPSEC_DATA_EVENT.clone(),
    ]
}

/// IPSec protocol implementation
pub struct IpsecProtocol;

impl IpsecProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl ProtocolActions for IpsecProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![list_connections_action(), close_connection_action()]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            accept_connection_action(),
            reject_connection_action(),
            log_handshake_action(),
            send_notify_action(),
            inspect_traffic_action(),
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "accept_connection" => self.execute_accept_connection(action),
            "reject_connection" => self.execute_reject_connection(action),
            "log_handshake" => self.execute_log_handshake(action),
            "send_notify" => self.execute_send_notify(action),
            "inspect_traffic" => self.execute_inspect_traffic(action),
            "list_connections" => Ok(ActionResult::NoAction), // Async action
            "close_connection" => Ok(ActionResult::NoAction),  // Async action
            _ => Err(anyhow::anyhow!("Unknown IPSec action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "IPSec/IKEv2"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_ipsec_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>IPSEC"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["ipsec", "ikev2", "ike"]
    }

    fn metadata(&self) -> crate::protocol::base_stack::ProtocolMetadata {
        crate::protocol::base_stack::ProtocolMetadata::with_notes(
            crate::protocol::base_stack::ProtocolState::Disabled,
            "No actual VPN tunnels. Full IPSec/IKEv2 implementation is infeasible: no viable Rust library (ipsec-parser is parse-only), protocol requires deep OS integration (XFRM policy), extremely complex (hundreds of thousands of lines in strongSwan). Use WireGuard for production VPN."
        )
    }
}

impl IpsecProtocol {
    /// Execute accept_connection action - allow IKE handshake to proceed
    fn execute_accept_connection(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Connection accepted");

        // In a full implementation, this would generate IKE response
        // For honeypot, just log the decision (logged via tracing in server)
        Ok(ActionResult::NoAction)
    }

    /// Execute reject_connection action - deny IKE handshake
    fn execute_reject_connection(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _reason = action
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("Unauthorized");

        // In full implementation, send NOTIFY with error or drop packet
        // For honeypot, just log (logged via tracing in server)
        Ok(ActionResult::NoAction)
    }

    /// Execute log_handshake action - capture IKE handshake details for honeypot
    fn execute_log_handshake(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _details = action
            .get("details")
            .and_then(|v| v.as_str())
            .unwrap_or("Handshake logged");

        // Logged via tracing in server
        Ok(ActionResult::NoAction)
    }

    /// Execute send_notify action - send IKE NOTIFY message
    fn execute_send_notify(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _notify_type = action
            .get("notify_type")
            .and_then(|v| v.as_str())
            .unwrap_or("NO_PROPOSAL_CHOSEN");

        // For honeypot, just log (logged via tracing in server)
        Ok(ActionResult::NoAction)
    }

    /// Execute inspect_traffic action - log decrypted ESP packet info
    fn execute_inspect_traffic(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _inspect = action
            .get("inspect")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        // Logged via tracing in server
        Ok(ActionResult::NoAction)
    }
}

/// Action: Accept IKE connection handshake
fn accept_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "accept_connection".to_string(),
        description: "Accept IPSec/IKEv2 connection handshake and establish SA".to_string(),
        parameters: vec![Parameter {
            name: "message".to_string(),
            type_hint: "string".to_string(),
            description: "Optional message to log".to_string(),
            required: false,
        }],
        example: json!({
            "type": "accept_connection",
            "message": "Legitimate VPN connection accepted"
        }),
    }
}

/// Action: Reject IKE connection handshake
fn reject_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "reject_connection".to_string(),
        description: "Reject IPSec/IKEv2 connection handshake (honeypot decision)".to_string(),
        parameters: vec![Parameter {
            name: "reason".to_string(),
            type_hint: "string".to_string(),
            description: "Reason for rejection".to_string(),
            required: false,
        }],
        example: json!({
            "type": "reject_connection",
            "reason": "Suspicious reconnaissance attempt"
        }),
    }
}

/// Action: Log IKE handshake details
fn log_handshake_action() -> ActionDefinition {
    ActionDefinition {
        name: "log_handshake".to_string(),
        description: "Log IPSec/IKEv2 handshake details for analysis".to_string(),
        parameters: vec![Parameter {
            name: "details".to_string(),
            type_hint: "string".to_string(),
            description: "Additional details to log".to_string(),
            required: false,
        }],
        example: json!({
            "type": "log_handshake",
            "details": "VPN scan attempt detected"
        }),
    }
}

/// Action: Send IKE NOTIFY message
fn send_notify_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_notify".to_string(),
        description: "Send IKE NOTIFY message to peer".to_string(),
        parameters: vec![Parameter {
            name: "notify_type".to_string(),
            type_hint: "string".to_string(),
            description: "IKE notify message type (e.g., NO_PROPOSAL_CHOSEN, AUTHENTICATION_FAILED)".to_string(),
            required: false,
        }],
        example: json!({
            "type": "send_notify",
            "notify_type": "NO_PROPOSAL_CHOSEN"
        }),
    }
}

/// Action: Inspect ESP/tunnel traffic
fn inspect_traffic_action() -> ActionDefinition {
    ActionDefinition {
        name: "inspect_traffic".to_string(),
        description: "Enable/disable ESP traffic inspection for this SA".to_string(),
        parameters: vec![Parameter {
            name: "inspect".to_string(),
            type_hint: "boolean".to_string(),
            description: "Whether to inspect decrypted ESP traffic".to_string(),
            required: false,
        }],
        example: json!({
            "type": "inspect_traffic",
            "inspect": true
        }),
    }
}

/// Action: List all connections (async)
fn list_connections_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_connections".to_string(),
        description: "List all established IPSec Security Associations".to_string(),
        parameters: vec![],
        example: json!({
            "type": "list_connections"
        }),
    }
}

/// Action: Close connection (async)
fn close_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_connection".to_string(),
        description: "Delete a specific IPSec Security Association".to_string(),
        parameters: vec![Parameter {
            name: "peer_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Peer address to disconnect".to_string(),
            required: true,
        }],
        example: json!({
            "type": "close_connection",
            "peer_addr": "192.168.1.100:500"
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types() {
        let events = get_ipsec_event_types();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_action_definitions() {
        let protocol = IpsecProtocol::new(
            std::net::UdpSocket::bind("127.0.0.1:0")
                .unwrap()
                .try_into()
                .unwrap(),
            "127.0.0.1:500".parse().unwrap(),
        );

        let sync_actions = protocol.get_sync_actions();
        assert!(!sync_actions.is_empty());

        let async_actions = protocol.get_async_actions(&AppState::default());
        assert!(!async_actions.is_empty());
    }
}
