//! OpenVPN protocol actions implementation
//!
//! Defines LLM-controllable actions for OpenVPN honeypot

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// OpenVPN handshake initiation event
pub static OPENVPN_HANDSHAKE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "openvpn_handshake",
        "OpenVPN client initiated handshake",
    )
});

/// OpenVPN data packet event
pub static OPENVPN_DATA_EVENT: LazyLock<EventType> =
    LazyLock::new(|| EventType::new("openvpn_data", "OpenVPN data packet received"));

/// Get all OpenVPN event types
pub fn get_openvpn_event_types() -> Vec<EventType> {
    vec![
        OPENVPN_HANDSHAKE_EVENT.clone(),
        OPENVPN_DATA_EVENT.clone(),
    ]
}

/// OpenVPN protocol implementation
pub struct OpenvpnProtocol;

impl OpenvpnProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Server for OpenvpnProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::openvpn::OpenvpnServer;
            use std::sync::Arc;
            OpenvpnServer::spawn_with_llm_actions(
                ctx.listen_addr,
                Arc::new(ctx.llm_client),
                ctx.state,
                ctx.server_id,
                ctx.status_tx,
            ).await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![list_connections_action(), close_connection_action()]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            accept_connection_action(),
            reject_connection_action(),
            log_handshake_action(),
            send_reset_action(),
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
            "send_reset" => self.execute_send_reset(action),
            "inspect_traffic" => self.execute_inspect_traffic(action),
            "list_connections" => Ok(ActionResult::NoAction), // Async action
            "close_connection" => Ok(ActionResult::NoAction),  // Async action
            _ => Err(anyhow::anyhow!("Unknown OpenVPN action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "OpenVPN"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_openvpn_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP/UDP>OPENVPN"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["openvpn"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadata {
        crate::protocol::metadata::ProtocolMetadata::with_notes(
            crate::protocol::metadata::DevelopmentState::Disabled,
            "No actual VPN tunnels. Full OpenVPN implementation is infeasible: no viable Rust library exists, protocol is extremely complex (500K+ lines in C++). Use WireGuard for production VPN. OpenVPN honeypot sufficient for detection/logging reconnaissance attempts."
        )
    }

    fn description(&self) -> &'static str {
        "OpenVPN honeypot server"
    }

    fn example_prompt(&self) -> &'static str {
        "Start an OpenVPN honeypot on port 1194"
    }
}

impl OpenvpnProtocol {
    /// Execute accept_connection action - allow handshake to proceed
    fn execute_accept_connection(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Connection accepted");

        // In a full implementation, this would generate handshake response
        // For honeypot, just log the decision (logged via tracing in server)
        Ok(ActionResult::NoAction)
    }

    /// Execute reject_connection action - deny handshake
    fn execute_reject_connection(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _reason = action
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("Unauthorized");

        // In full implementation, send reset or simply drop packet
        // For honeypot, just log (logged via tracing in server)
        Ok(ActionResult::NoAction)
    }

    /// Execute log_handshake action - capture handshake details for honeypot
    fn execute_log_handshake(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _details = action
            .get("details")
            .and_then(|v| v.as_str())
            .unwrap_or("Handshake logged");

        // Logged via tracing in server
        Ok(ActionResult::NoAction)
    }

    /// Execute send_reset action - send connection reset
    fn execute_send_reset(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _reason = action
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("Connection reset");

        // For honeypot, just log (logged via tracing in server)
        Ok(ActionResult::NoAction)
    }

    /// Execute inspect_traffic action - log decrypted packet info
    fn execute_inspect_traffic(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _inspect = action
            .get("inspect")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        // Logged via tracing in server
        Ok(ActionResult::NoAction)
    }
}

/// Action: Accept connection handshake
fn accept_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "accept_connection".to_string(),
        description: "Accept OpenVPN connection handshake and establish tunnel".to_string(),
        parameters: vec![Parameter {
            name: "message".to_string(),
            type_hint: "string".to_string(),
            description: "Optional message to log".to_string(),
            required: false,
        }],
        example: json!({
            "type": "accept_connection",
            "message": "Legitimate connection accepted"
        }),
    }
}

/// Action: Reject connection handshake
fn reject_connection_action() -> ActionDefinition {
    ActionDefinition {
        name: "reject_connection".to_string(),
        description: "Reject OpenVPN connection handshake (honeypot decision)".to_string(),
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

/// Action: Log handshake details
fn log_handshake_action() -> ActionDefinition {
    ActionDefinition {
        name: "log_handshake".to_string(),
        description: "Log OpenVPN handshake details for analysis".to_string(),
        parameters: vec![Parameter {
            name: "details".to_string(),
            type_hint: "string".to_string(),
            description: "Additional details to log".to_string(),
            required: false,
        }],
        example: json!({
            "type": "log_handshake",
            "details": "Port scan attempt detected"
        }),
    }
}

/// Action: Send connection reset
fn send_reset_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_reset".to_string(),
        description: "Send connection reset packet to client".to_string(),
        parameters: vec![Parameter {
            name: "reason".to_string(),
            type_hint: "string".to_string(),
            description: "Reason for reset".to_string(),
            required: false,
        }],
        example: json!({
            "type": "send_reset",
            "reason": "Invalid handshake"
        }),
    }
}

/// Action: Inspect tunnel traffic
fn inspect_traffic_action() -> ActionDefinition {
    ActionDefinition {
        name: "inspect_traffic".to_string(),
        description: "Enable/disable traffic inspection for this connection".to_string(),
        parameters: vec![Parameter {
            name: "inspect".to_string(),
            type_hint: "boolean".to_string(),
            description: "Whether to inspect decrypted traffic".to_string(),
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
        description: "List all connected OpenVPN clients".to_string(),
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
        description: "Close connection to a specific client".to_string(),
        parameters: vec![Parameter {
            name: "peer_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Client address to disconnect".to_string(),
            required: true,
        }],
        example: json!({
            "type": "close_connection",
            "peer_addr": "192.168.1.100:1194"
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types() {
        let events = get_openvpn_event_types();
        assert_eq!(events.len(), 2);
    }

    #[tokio::test]
    async fn test_action_definitions() {
        let protocol = OpenvpnProtocol::new();

        let sync_actions = protocol.get_sync_actions();
        assert!(!sync_actions.is_empty());

        let async_actions = protocol.get_async_actions(&AppState::default());
        assert!(!async_actions.is_empty());
    }
}
