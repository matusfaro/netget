//! WireGuard protocol actions implementation
//!
//! Defines LLM-controllable actions for WireGuard VPN server

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// WireGuard peer authorization request event
pub static WIREGUARD_PEER_REQUEST_EVENT: LazyLock<EventType> =
    LazyLock::new(|| EventType::new("wireguard_peer_request", "WireGuard peer requesting authorization"));

/// WireGuard peer connected event
pub static WIREGUARD_PEER_CONNECTED_EVENT: LazyLock<EventType> =
    LazyLock::new(|| EventType::new("wireguard_peer_connected", "WireGuard peer successfully connected"));

/// Get all WireGuard event types
pub fn get_wireguard_event_types() -> Vec<EventType> {
    vec![
        WIREGUARD_PEER_REQUEST_EVENT.clone(),
        WIREGUARD_PEER_CONNECTED_EVENT.clone(),
    ]
}

/// WireGuard protocol implementation
pub struct WireguardProtocol {
    // Protocol instance doesn't need state - server handle is managed separately
}

impl WireguardProtocol {
    pub fn new() -> Self {
        Self {}
    }
}

impl Server for WireguardProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::wireguard::WireguardServer;
            use std::sync::Arc;
            WireguardServer::spawn_with_llm_actions(
                ctx.listen_addr,
                Arc::new(ctx.llm_client),
                ctx.state,
                ctx.server_id,
                ctx.status_tx,
            ).await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            list_peers_action(),
            remove_peer_action(),
            get_server_info_action(),
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            authorize_peer_action(),
            reject_peer_action(),
            set_peer_traffic_limit_action(),
            disconnect_peer_action(),
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "authorize_peer" => self.execute_authorize_peer(action),
            "reject_peer" => self.execute_reject_peer(action),
            "set_peer_traffic_limit" => self.execute_set_peer_traffic_limit(action),
            "disconnect_peer" => self.execute_disconnect_peer(action),
            "list_peers" => Ok(ActionResult::NoAction), // Handled by async action executor
            "remove_peer" => Ok(ActionResult::NoAction), // Handled by async action executor
            "get_server_info" => Ok(ActionResult::NoAction), // Handled by async action executor
            _ => Err(anyhow::anyhow!("Unknown WireGuard action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "WireGuard"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_wireguard_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>WG"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["wireguard", "wg"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadata {
        crate::protocol::metadata::ProtocolMetadata::with_notes(
            crate::protocol::metadata::DevelopmentState::Implemented,
            "Full VPN server with actual tunnel support using defguard_wireguard_rs. Creates TUN interface and supports peer connections."
        )
    }
}

impl WireguardProtocol {
    /// Execute authorize_peer action - allow peer to connect and create tunnel
    fn execute_authorize_peer(&self, action: serde_json::Value) -> Result<ActionResult> {
        let public_key = action
            .get("public_key")
            .and_then(|v| v.as_str())
            .context("Missing public_key in authorize_peer")?
            .to_string();

        let allowed_ips = action
            .get("allowed_ips")
            .and_then(|v| v.as_array())
            .context("Missing allowed_ips in authorize_peer")?
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect::<Vec<String>>();

        if allowed_ips.is_empty() {
            return Err(anyhow::anyhow!("allowed_ips must not be empty"));
        }

        let endpoint = action
            .get("endpoint")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok());

        let message = action
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Peer authorized");

        // Return authorization details to be executed by server
        Ok(ActionResult::Output(
            serde_json::to_vec(&json!({
                "action": "authorize_peer",
                "public_key": public_key,
                "allowed_ips": allowed_ips,
                "endpoint": endpoint.map(|e: std::net::SocketAddr| e.to_string()),
                "message": message,
            }))?
        ))
    }

    /// Execute reject_peer action - deny peer connection
    fn execute_reject_peer(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _public_key = action
            .get("public_key")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let _reason = action
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("Unauthorized");

        // Return rejection notification (no actual action needed for honeypot-style rejection)
        Ok(ActionResult::NoAction)
    }

    /// Execute set_peer_traffic_limit action - configure traffic limits for peer
    fn execute_set_peer_traffic_limit(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _public_key = action
            .get("public_key")
            .and_then(|v| v.as_str())
            .context("Missing public_key")?;

        let _limit_mbps = action
            .get("limit_mbps")
            .and_then(|v| v.as_u64());

        let _limit_total_mb = action
            .get("limit_total_mb")
            .and_then(|v| v.as_u64());

        // Note: Traffic limiting would require iptables/tc configuration
        Ok(ActionResult::NoAction)
    }

    /// Execute disconnect_peer action - immediately disconnect a peer
    fn execute_disconnect_peer(&self, action: serde_json::Value) -> Result<ActionResult> {
        let public_key = action
            .get("public_key")
            .and_then(|v| v.as_str())
            .context("Missing public_key")?
            .to_string();

        let reason = action
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("Disconnected by admin");

        // Return disconnect command to be executed by server
        Ok(ActionResult::Output(
            serde_json::to_vec(&json!({
                "action": "disconnect_peer",
                "public_key": public_key,
                "reason": reason,
            }))?
        ))
    }
}

#[allow(dead_code)]
fn min(a: usize, b: usize) -> usize {
    if a < b { a } else { b }
}

/// Action: Authorize peer to connect
fn authorize_peer_action() -> ActionDefinition {
    ActionDefinition {
        name: "authorize_peer".to_string(),
        description: "Authorize a WireGuard peer to connect and establish VPN tunnel".to_string(),
        parameters: vec![
            Parameter {
                name: "public_key".to_string(),
                type_hint: "string".to_string(),
                description: "Peer's public key (base64)".to_string(),
                required: true,
            },
            Parameter {
                name: "allowed_ips".to_string(),
                type_hint: "array".to_string(),
                description: "List of allowed IP ranges for this peer (CIDR notation, e.g. 10.20.30.2/32)".to_string(),
                required: true,
            },
            Parameter {
                name: "endpoint".to_string(),
                type_hint: "string".to_string(),
                description: "Optional peer endpoint address (IP:port)".to_string(),
                required: false,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Optional authorization message".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "authorize_peer",
            "public_key": "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=",
            "allowed_ips": ["10.20.30.2/32"],
            "endpoint": "203.0.113.45:51820",
            "message": "Legitimate VPN client authorized"
        }),
    }
}

/// Action: Reject peer connection request
fn reject_peer_action() -> ActionDefinition {
    ActionDefinition {
        name: "reject_peer".to_string(),
        description: "Reject a WireGuard peer connection request".to_string(),
        parameters: vec![
            Parameter {
                name: "public_key".to_string(),
                type_hint: "string".to_string(),
                description: "Peer's public key (base64)".to_string(),
                required: true,
            },
            Parameter {
                name: "reason".to_string(),
                type_hint: "string".to_string(),
                description: "Reason for rejection".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "reject_peer",
            "public_key": "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=",
            "reason": "Unauthorized client - unknown public key"
        }),
    }
}

/// Action: Set traffic limits for a peer
fn set_peer_traffic_limit_action() -> ActionDefinition {
    ActionDefinition {
        name: "set_peer_traffic_limit".to_string(),
        description: "Configure traffic rate limiting for a specific peer".to_string(),
        parameters: vec![
            Parameter {
                name: "public_key".to_string(),
                type_hint: "string".to_string(),
                description: "Peer's public key (base64)".to_string(),
                required: true,
            },
            Parameter {
                name: "limit_mbps".to_string(),
                type_hint: "number".to_string(),
                description: "Maximum bandwidth in Mbps".to_string(),
                required: false,
            },
            Parameter {
                name: "limit_total_mb".to_string(),
                type_hint: "number".to_string(),
                description: "Total data transfer limit in MB".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "set_peer_traffic_limit",
            "public_key": "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=",
            "limit_mbps": 100,
            "limit_total_mb": 10000
        }),
    }
}

/// Action: Disconnect peer immediately
fn disconnect_peer_action() -> ActionDefinition {
    ActionDefinition {
        name: "disconnect_peer".to_string(),
        description: "Immediately disconnect a WireGuard peer".to_string(),
        parameters: vec![
            Parameter {
                name: "public_key".to_string(),
                type_hint: "string".to_string(),
                description: "Peer's public key (base64)".to_string(),
                required: true,
            },
            Parameter {
                name: "reason".to_string(),
                type_hint: "string".to_string(),
                description: "Reason for disconnection".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "disconnect_peer",
            "public_key": "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=",
            "reason": "Suspicious traffic detected"
        }),
    }
}

/// Action: List all connected peers (async)
fn list_peers_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_peers".to_string(),
        description: "List all currently connected WireGuard peers".to_string(),
        parameters: vec![],
        example: json!({
            "type": "list_peers"
        }),
    }
}

/// Action: Remove peer permanently (async)
fn remove_peer_action() -> ActionDefinition {
    ActionDefinition {
        name: "remove_peer".to_string(),
        description: "Permanently remove a peer from the server configuration".to_string(),
        parameters: vec![
            Parameter {
                name: "public_key".to_string(),
                type_hint: "string".to_string(),
                description: "Peer's public key (base64)".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "remove_peer",
            "public_key": "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg="
        }),
    }
}

/// Action: Get server information (async)
fn get_server_info_action() -> ActionDefinition {
    ActionDefinition {
        name: "get_server_info".to_string(),
        description: "Get WireGuard server public key and configuration details".to_string(),
        parameters: vec![],
        example: json!({
            "type": "get_server_info"
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::app_state::AppState;

    #[test]
    fn test_event_types() {
        let events = get_wireguard_event_types();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_action_definitions() {
        let protocol = WireguardProtocol::new();

        let sync_actions = protocol.get_sync_actions();
        assert!(!sync_actions.is_empty());

        let async_actions = protocol.get_async_actions(&AppState::default());
        assert!(!async_actions.is_empty());
    }

    #[test]
    fn test_authorize_peer_action() {
        let protocol = WireguardProtocol::new();

        let action = json!({
            "type": "authorize_peer",
            "public_key": "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=",
            "allowed_ips": ["10.20.30.2/32"],
        });

        let result = protocol.execute_action(action);
        assert!(result.is_ok());
    }
}
