//! OpenVPN protocol actions implementation
//!
//! Defines the actions for the OpenVPN-shaped tunnel server.
//!
//! **The protocol is marked `DevelopmentState::Incomplete` and is therefore hidden
//! from the LLM.** Peers are auto-accepted with no authentication and all share a
//! hardcoded encryption key - see `src/server/openvpn/mod.rs` for the full
//! explanation. The actions below are observation/logging hooks; none of them
//! gates a connection.

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::log_template::LogTemplate;
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// OpenVPN peer connected event.
///
/// Raised after a peer sends a HARD_RESET and is assigned a VPN IP. The peer is
/// **not authenticated** - no handshake verifies it - so treat this as "a host
/// started talking to us", not "a trusted client connected". Data fields:
/// `peer_addr`, `vpn_ip`, `session_id`, `authenticated` (always `false`).
pub static OPENVPN_PEER_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "openvpn_peer_connected",
        "An OpenVPN peer was accepted and assigned a VPN IP. The peer is NOT \
         authenticated (no TLS handshake is performed). Respond with inspect_traffic \
         or show_message to record it; the connection cannot be refused from here.",
        json!({
            "type": "inspect_traffic",
            "inspect": true
        }),
    )
    .with_log_template(
        LogTemplate::new()
            .with_info("{client_ip} OpenVPN connected -> {vpn_ip}")
            .with_debug("OpenVPN peer connected: {client_ip} -> VPN IP {vpn_ip}")
            .with_trace("OpenVPN peer: {json_pretty(.)}"),
    )
});

/// Get all OpenVPN event types.
///
/// Only `openvpn_peer_connected` is emitted. An `openvpn_peer_request`
/// authorization event used to be declared here, but nothing ever raised it -
/// peers are auto-accepted inside `handle_handshake_initiation` - so handlers
/// subscribed to it would never fire. It was removed rather than left as a false
/// promise of pre-connection authorization.
pub fn get_openvpn_event_types() -> Vec<EventType> {
    vec![OPENVPN_PEER_CONNECTED_EVENT.clone()]
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
    /// No user-triggered actions are advertised.
    ///
    /// `list_peers`, `remove_peer` and `get_server_info` were previously listed.
    /// The action executor calls `Server::execute_action()` on a stateless
    /// `OpenvpnProtocol` struct with no handle to the running `OpenvpnServer`, so
    /// they returned `NoAction` unconditionally and could never list or remove a
    /// peer. They remain accepted by `execute_action` for backwards compatibility.
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }
    /// Observation actions only.
    ///
    /// `authorize_peer` and `reject_peer` are kept because scripts may already
    /// emit them, but their descriptions now say plainly that they do not gate
    /// anything: peers are accepted before this event is raised.
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
        use crate::protocol::metadata::{
            DevelopmentState, PrivilegeRequirement, ProtocolMetadataV2,
        };

        // Incomplete, and deliberately so: `is_available_to_llm()` returns false
        // for this state, which keeps the protocol out of LLM prompts. Advertising
        // it as usable would invite someone to route real traffic over a tunnel
        // whose keys are public constants. See src/server/openvpn/mod.rs.
        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Incomplete)
            .privilege_requirement(PrivilegeRequirement::Root)
            .implementation(
                "Partial: OpenVPN packet format, TUN interface, IP pool and real AEAD \
                 encrypt/decrypt are implemented. The TLS control channel is NOT - \
                 create_tls_config() is never used, no handshake occurs, no peer is \
                 authenticated, and handle_control_message/handle_ack_packet are stubs.",
            )
            .llm_control(
                "openvpn_peer_connected event only (observation/logging). Peers are \
                 auto-accepted; authorize_peer and reject_peer do not gate anything.",
            )
            .e2e_testing(
                "Not interoperable with the real openvpn client - it cannot complete a \
                 handshake against a server that has no TLS control channel.",
            )
            .notes(
                "INSECURE - NOT A VPN. Every peer derives the same data-channel key from \
                 hardcoded constants committed to this repository, so the tunnel offers no \
                 confidentiality. Never carry real traffic over it. Use WireGuard instead.",
            )
            .build()
    }
    fn description(&self) -> &'static str {
        "OpenVPN-shaped tunnel server (INCOMPLETE/INSECURE: no TLS handshake, hardcoded keys)"
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
                "instruction": "OpenVPN server. Log every peer that connects, including its assigned VPN IP from the 10.8.0.0/24 pool."
            }),
            // Script mode: Scripted peer handling
            json!({
                "type": "open_server",
                "port": 1194,
                "base_stack": "openvpn",
                "event_handlers": [{
                    "event_pattern": "openvpn_peer_connected",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "return {type='inspect_traffic', inspect=True}"
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
                ctx.legacy_listen_addr(),
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
        description: "Record that a peer is considered authorized. NOT ENFORCED: peers are \
                      auto-accepted and given a VPN IP before openvpn_peer_connected is \
                      raised, so this does not grant access that was withheld."
            .to_string(),
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
        log_template: Some(
            LogTemplate::new()
                .with_info("-> OpenVPN authorize {peer_addr} -> {vpn_ip}")
                .with_debug("OpenVPN authorize peer: {peer_addr} -> VPN IP {vpn_ip}"),
        ),
    }
}

/// Action: Reject peer connection
fn reject_peer_action() -> ActionDefinition {
    ActionDefinition {
        name: "reject_peer".to_string(),
        description: "Record that a peer should be refused. NOT ENFORCED: the peer already \
                      holds a VPN IP and an active data channel by the time this runs, and \
                      nothing tears it down. Logging only."
            .to_string(),
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
        log_template: Some(
            LogTemplate::new()
                .with_info("-> OpenVPN reject {peer_addr}: {reason}")
                .with_debug("OpenVPN reject peer: {peer_addr}, reason: {reason}"),
        ),
    }
}

/// Action: Set peer traffic limit
fn set_peer_limit_action() -> ActionDefinition {
    ActionDefinition {
        name: "set_peer_limit".to_string(),
        description: "Record an intended bandwidth limit for a peer. NOT ENFORCED: no traffic \
                      shaping is configured, so the peer's throughput is unaffected."
            .to_string(),
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
        log_template: Some(
            LogTemplate::new()
                .with_info("-> OpenVPN limit {peer_addr} {limit_mbps}Mbps")
                .with_debug("OpenVPN set peer limit: {peer_addr} -> {limit_mbps} Mbps"),
        ),
    }
}

/// Action: Inspect tunnel traffic
fn inspect_traffic_action() -> ActionDefinition {
    ActionDefinition {
        name: "inspect_traffic".to_string(),
        description: "Flag this peer's decrypted tunnel traffic for logging. The tunnel \
                      payload is already logged at TRACE level; this records the intent."
            .to_string(),
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
        log_template: Some(
            LogTemplate::new()
                .with_info("-> OpenVPN inspect={inspect}")
                .with_debug("OpenVPN traffic inspection: {inspect}"),
        ),
    }
}
