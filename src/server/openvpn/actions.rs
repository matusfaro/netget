//! OpenVPN protocol actions implementation
//!
//! Defines LLM-controllable actions for OpenVPN honeypot

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// OpenVPN peer connected event
pub static OPENVPN_PEER_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "openvpn_peer_connected",
        "OpenVPN peer successfully connected and authenticated",
        json!({
            "type": "inspect_traffic",
            "inspect": true
        }),
    )
});

/// OpenVPN peer request event (for LLM authorization)
pub static OPENVPN_PEER_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "openvpn_peer_request",
        "OpenVPN peer requesting connection authorization",
        json!({
            "type": "authorize_peer",
            "peer_addr": "203.0.113.45:1194",
            "vpn_ip": "10.8.0.5"
        }),
    )
});

/// Get all OpenVPN event types
pub fn get_openvpn_event_types() -> Vec<EventType> {
    vec![
        OPENVPN_PEER_CONNECTED_EVENT.clone(),
        OPENVPN_PEER_REQUEST_EVENT.clone(),
    ]
}

/// OpenVPN protocol implementation
pub struct OpenvpnProtocol;

impl OpenvpnProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for OpenvpnProtocol {
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
            set_peer_limit_action(),
            inspect_traffic_action(),
        ]
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
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Stable)
            .implementation(
                "Full OpenVPN server with TUN interface, AES-256-GCM/ChaCha20-Poly1305 encryption",
            )
            .llm_control("Peer authorization, traffic inspection, connection limits")
            .e2e_testing("OpenVPN client (full tunnel support)")
            .notes("Production-ready VPN server with simplified TLS for MVP")
            .build()
    }
    fn description(&self) -> &'static str {
        "OpenVPN VPN server"
    }
    fn example_prompt(&self) -> &'static str {
        "Start an OpenVPN VPN server on port 1194"
    }
    fn group_name(&self) -> &'static str {
        "VPN & Routing"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        StartupExamples::new(
            // LLM mode: LLM handles peer authorization
            json!({
                "type": "open_server",
                "port": 1194,
                "base_stack": "openvpn",
                "instruction": "OpenVPN server. Authorize all incoming peers and assign VPN IPs from 10.8.0.0/24 pool. Log connection events."
            }),
            // Script mode: Scripted peer handling
            json!({
                "type": "open_server",
                "port": 1194,
                "base_stack": "openvpn",
                "event_handlers": [{
                    "event_pattern": "openvpn_peer_request",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "return {type='authorize_peer', peer_addr=event.peer_addr}"
                    }
                }]
            }),
            // Static mode: Fixed authorization response
            json!({
                "type": "open_server",
                "port": 1194,
                "base_stack": "openvpn",
                "event_handlers": [{
                    "event_pattern": "openvpn_peer_connected",
                    "handler": {
                        "type": "static",
                        "actions": [{"type": "no_action"}]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
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
            )
            .await
        })
    }
    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "authorize_peer" => self.execute_authorize_peer(action),
            "reject_peer" => self.execute_reject_peer(action),
            "set_peer_limit" => self.execute_set_peer_limit(action),
            "inspect_traffic" => self.execute_inspect_traffic(action),
            "list_peers" => Ok(ActionResult::NoAction), // Async action
            "remove_peer" => Ok(ActionResult::NoAction), // Async action
            "get_server_info" => Ok(ActionResult::NoAction), // Async action
            _ => Err(anyhow::anyhow!("Unknown OpenVPN action: {}", action_type)),
        }
    }
}

impl OpenvpnProtocol {
    /// Execute authorize_peer action - allow peer to connect and establish tunnel
    fn execute_authorize_peer(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _peer_addr = action
            .get("peer_addr")
            .and_then(|v| v.as_str())
            .context("Missing 'peer_addr' field")?;

        let _vpn_ip = action.get("vpn_ip").and_then(|v| v.as_str());

        // Authorization handled in server handshake logic
        Ok(ActionResult::NoAction)
    }

    /// Execute reject_peer action - deny peer connection
    fn execute_reject_peer(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _peer_addr = action
            .get("peer_addr")
            .and_then(|v| v.as_str())
            .context("Missing 'peer_addr' field")?;

        let _reason = action
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("Unauthorized");

        // Rejection handled in server
        Ok(ActionResult::NoAction)
    }

    /// Execute set_peer_limit action - configure bandwidth/data limits
    fn execute_set_peer_limit(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _peer_addr = action
            .get("peer_addr")
            .and_then(|v| v.as_str())
            .context("Missing 'peer_addr' field")?;

        let _limit_mbps = action.get("limit_mbps").and_then(|v| v.as_u64());

        // Placeholder for MVP
        Ok(ActionResult::NoAction)
    }

    /// Execute inspect_traffic action - enable traffic inspection
    fn execute_inspect_traffic(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _inspect = action
            .get("inspect")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        // Traffic inspection logged via tracing in server
        Ok(ActionResult::NoAction)
    }
}

/// Action: Authorize peer connection
fn authorize_peer_action() -> ActionDefinition {
    ActionDefinition {
        name: "authorize_peer".to_string(),
        description: "Authorize a peer to connect and establish VPN tunnel".to_string(),
        parameters: vec![
            Parameter {
                name: "peer_addr".to_string(),
                type_hint: "string".to_string(),
                description: "Peer address requesting connection".to_string(),
                required: true,
            },
            Parameter {
                name: "vpn_ip".to_string(),
                type_hint: "string".to_string(),
                description: "VPN IP to assign to peer (optional, auto-assigned if not specified)"
                    .to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "authorize_peer",
            "peer_addr": "203.0.113.45:1194",
            "vpn_ip": "10.8.0.5"
        }),
    }
}

/// Action: Reject peer connection
fn reject_peer_action() -> ActionDefinition {
    ActionDefinition {
        name: "reject_peer".to_string(),
        description: "Reject a peer connection request".to_string(),
        parameters: vec![
            Parameter {
                name: "peer_addr".to_string(),
                type_hint: "string".to_string(),
                description: "Peer address to reject".to_string(),
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
            "peer_addr": "203.0.113.45:1194",
            "reason": "Unauthorized"
        }),
    }
}

/// Action: Set peer traffic limit
fn set_peer_limit_action() -> ActionDefinition {
    ActionDefinition {
        name: "set_peer_limit".to_string(),
        description: "Configure bandwidth or data limits for a peer".to_string(),
        parameters: vec![
            Parameter {
                name: "peer_addr".to_string(),
                type_hint: "string".to_string(),
                description: "Peer address".to_string(),
                required: true,
            },
            Parameter {
                name: "limit_mbps".to_string(),
                type_hint: "number".to_string(),
                description: "Bandwidth limit in Mbps".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "set_peer_limit",
            "peer_addr": "203.0.113.45:1194",
            "limit_mbps": 10
        }),
    }
}

/// Action: Inspect tunnel traffic
fn inspect_traffic_action() -> ActionDefinition {
    ActionDefinition {
        name: "inspect_traffic".to_string(),
        description: "Enable/disable traffic inspection for decrypted VPN traffic".to_string(),
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

/// Action: List all peers (async)
fn list_peers_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_peers".to_string(),
        description: "List all connected OpenVPN peers".to_string(),
        parameters: vec![],
        example: json!({
            "type": "list_peers"
        }),
    }
}

/// Action: Remove peer (async)
fn remove_peer_action() -> ActionDefinition {
    ActionDefinition {
        name: "remove_peer".to_string(),
        description: "Permanently remove a peer from the VPN".to_string(),
        parameters: vec![Parameter {
            name: "peer_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Peer address to remove".to_string(),
            required: true,
        }],
        example: json!({
            "type": "remove_peer",
            "peer_addr": "203.0.113.45:1194"
        }),
    }
}

/// Action: Get server info (async)
fn get_server_info_action() -> ActionDefinition {
    ActionDefinition {
        name: "get_server_info".to_string(),
        description: "Get OpenVPN server configuration and status".to_string(),
        parameters: vec![],
        example: json!({
            "type": "get_server_info"
        }),
    }
}
