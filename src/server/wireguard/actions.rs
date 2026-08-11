//! WireGuard protocol actions implementation
//!
//! Defines LLM-controllable actions for WireGuard VPN server

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

/// WireGuard peer connected event.
///
/// Raised by the peer monitoring loop the first time a peer shows up on the
/// WireGuard interface, i.e. *after* its cryptographic handshake has already
/// succeeded (WireGuard performs the handshake in the kernel/userspace backend,
/// so it cannot be gated beforehand). Event data fields:
/// `public_key`, `endpoint`, `allowed_ips`, `server_public_key`, `listen_port`.
pub static WIREGUARD_PEER_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "wireguard_peer_connected",
        "WireGuard peer completed its handshake and appeared on the interface. \
         Respond with authorize_peer to (re)configure its allowed IPs, or with \
         disconnect_peer/reject_peer to remove it. Return no actions at all to leave the peer \
         as-is.",
        json!({
            "type": "authorize_peer",
            "public_key": "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=",
            "allowed_ips": ["10.0.0.2/32"]
        }),
    )
    .with_alternative_example(json!({
        "type": "disconnect_peer",
        "public_key": "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=",
        "reason": "not on the allow list"
    }))
    // The three actions that actually change the interface. `set_peer_traffic_limit` is
    // deliberately left out: it is a documented no-op (see mod.rs and CLAUDE.md - no tc/iptables
    // configuration is performed), so advertising it would promise enforcement that never
    // happens. Without this list `call_llm` offered the model none of them, because it builds
    // the model's tool list from the event type rather than from get_sync_actions().
    .with_actions(vec![
        authorize_peer_action(),
        reject_peer_action(),
        disconnect_peer_action(),
    ])
    .with_log_template(
        LogTemplate::new()
            .with_info("WireGuard {client_ip} connected")
            .with_debug("WireGuard peer connected from {client_ip}:{client_port}")
            .with_trace("WireGuard: {json_pretty(.)}"),
    )
});

/// Get all WireGuard event types
pub fn get_wireguard_event_types() -> Vec<EventType> {
    vec![WIREGUARD_PEER_CONNECTED_EVENT.clone()]
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

// Implement Protocol trait (base trait for all protocols)
impl Protocol for WireguardProtocol {
    /// No user-triggered actions are advertised.
    ///
    /// `list_peers`, `remove_peer` and `get_server_info` used to be listed here,
    /// but the action executor calls [`Server::execute_action`] on a *stateless*
    /// protocol struct that has no handle to the running `WireguardServer`, so
    /// they could never return peer data or touch the interface. They are still
    /// accepted by `execute_action` (as no-ops) for backwards compatibility, but
    /// advertising them to the LLM would promise data that never arrives.
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            authorize_peer_action(),
            reject_peer_action(),
            set_peer_traffic_limit_action(),
            disconnect_peer_action(),
        ]
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

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{
            DevelopmentState, PrivilegeRequirement, ProtocolMetadataV2,
        };

        ProtocolMetadataV2::builder()
            // Beta, not Stable. Demoted from Stable because it has NEVER been validated
            // end-to-end against a real WireGuard client, and by the project's own rule
            // Stable requires exactly that. See the notes below and tests/server/wireguard/.
            .state(DevelopmentState::Beta)
            .privilege_requirement(PrivilegeRequirement::Root)
            .implementation(
                "Thin orchestration layer over defguard_wireguard_rs v0.7 - NetGet \
                 implements NONE of the WireGuard protocol itself (no Noise_IK, no crypto, \
                 no packet handling). All of that lives in the platform backend: the kernel \
                 module on Linux/FreeBSD/Windows, and the EXTERNAL wireguard-go binary on \
                 macOS (defguard shells out to `wireguard-go`, which must be installed).",
            )
            .llm_control(
                "Post-handshake peer policy: on wireguard_peer_connected the LLM can \
                 set allowed IPs (authorize_peer) or remove the peer \
                 (disconnect_peer/reject_peer). The handshake itself happens inside the \
                 backend and cannot be gated.",
            )
            .e2e_testing(
                "Non-root tests exercise NetGet's own logic (the authorize/reject/disconnect \
                 action executors and the event's action declarations). A real handshake \
                 needs root + a WireGuard backend (kernel, or wireguard-go on macOS) and has \
                 never been run; it is a root-gated #[ignore]d harness only.",
            )
            .notes(
                "Real data plane (via the platform backend), but Beta because it has never \
                 been validated against a real client and cannot be in CI: creating the \
                 interface needs root, and on macOS also the external wireguard-go binary. \
                 Two substantive caveats found while resolving the rating: (1) The reactive \
                 authorization model is backwards for WireGuard. A responder drops a handshake \
                 whose static public key is not ALREADY a configured peer, but NetGet only \
                 learns of a peer by polling read_interface_data() AFTER it appears - and an \
                 unconfigured peer never appears. There is also no user-triggered action to \
                 pre-add a peer (get_async_actions is empty). So wireguard_peer_connected can \
                 effectively never fire for a genuinely new peer, leaving the LLM \
                 authorize/reject flow unreachable in practice; it works only for peers \
                 configured out-of-band. (2) set_peer_traffic_limit is recorded but NOT \
                 enforced (no tc/iptables). Still the closest thing to a working tunnel in \
                 NetGet - openvpn/ipsec do not carry traffic at all.",
            )
            .build()
    }

    fn description(&self) -> &'static str {
        "WireGuard VPN server"
    }

    fn example_prompt(&self) -> &'static str {
        "Start a WireGuard VPN server on port 51820"
    }

    fn group_name(&self) -> &'static str {
        "VPN & Routing"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        use serde_json::json;

        StartupExamples::new(
            // LLM mode: LLM controls peer authorization
            json!({
                "type": "open_server",
                "port": 51820,
                "base_stack": "wireguard",
                "instruction": "Accept VPN clients and authorize peers with 10.20.30.x addresses"
            }),
            // Script mode: Code-based peer authorization
            json!({
                "type": "open_server",
                "port": 51820,
                "base_stack": "wireguard",
                "event_handlers": [{
                    "event_pattern": "wireguard_peer_connected",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<wireguard_server_handler>"
                    }
                }]
            }),
            // Static mode: Fixed peer authorization
            json!({
                "type": "open_server",
                "port": 51820,
                "base_stack": "wireguard",
                "event_handlers": [
                    {
                        "event_pattern": "wireguard_peer_connected",
                        "handler": {
                            "type": "static",
                            "actions": [{
                                "type": "authorize_peer",
                                "public_key": "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=",
                                "allowed_ips": ["10.20.30.2/32"],
                                "message": "Peer authorized"
                            }]
                        }
                    }
                ]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
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
            "set_peer_traffic_limit" => self.execute_set_peer_traffic_limit(action),
            "disconnect_peer" => self.execute_disconnect_peer(action),
            "list_peers" => Ok(ActionResult::NoAction), // Handled by async action executor
            "remove_peer" => Ok(ActionResult::NoAction), // Handled by async action executor
            "get_server_info" => Ok(ActionResult::NoAction), // Handled by async action executor
            _ => Err(anyhow::anyhow!("Unknown WireGuard action: {}", action_type)),
        }
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
        Ok(ActionResult::Output(serde_json::to_vec(&json!({
            "action": "authorize_peer",
            "public_key": public_key,
            "allowed_ips": allowed_ips,
            "endpoint": endpoint.map(|e: std::net::SocketAddr| e.to_string()),
            "message": message,
        }))?))
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

        let _limit_mbps = action.get("limit_mbps").and_then(|v| v.as_u64());

        let _limit_total_mb = action.get("limit_total_mb").and_then(|v| v.as_u64());

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
        Ok(ActionResult::Output(serde_json::to_vec(&json!({
            "action": "disconnect_peer",
            "public_key": public_key,
            "reason": reason,
        }))?))
    }
}

/// Action: Authorize peer to connect
fn authorize_peer_action() -> ActionDefinition {
    ActionDefinition {
        name: "authorize_peer".to_string(),
        description: "Configure a WireGuard peer on the live interface with the given allowed \
                      IPs. Use this in response to wireguard_peer_connected to pin which VPN \
                      addresses the peer may use. Note: the peer's handshake has already \
                      succeeded by the time this runs - this sets policy, it does not grant \
                      or deny the initial connection."
            .to_string(),
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
                description:
                    "List of allowed IP ranges for this peer (CIDR notation, e.g. 10.20.30.2/32)"
                        .to_string(),
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
        log_template: Some(
            LogTemplate::new()
                .with_info("-> WG authorize {public_key}")
                .with_debug("WG authorize_peer: pubkey={public_key} ips={allowed_ips}"),
        ),
    }
}

/// Action: Reject peer connection request
fn reject_peer_action() -> ActionDefinition {
    ActionDefinition {
        name: "reject_peer".to_string(),
        description: "Remove a WireGuard peer from the interface, refusing it access to the \
                      VPN. Identical in effect to disconnect_peer; use this when the peer \
                      should never have been allowed on."
            .to_string(),
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
        log_template: Some(
            LogTemplate::new()
                .with_info("-> WG reject {public_key}: {reason}")
                .with_debug("WG reject_peer: pubkey={public_key} reason={reason}"),
        ),
    }
}

/// Action: Set traffic limits for a peer
fn set_peer_traffic_limit_action() -> ActionDefinition {
    ActionDefinition {
        name: "set_peer_traffic_limit".to_string(),
        description: "Record an intended traffic limit for a peer. NOT ENFORCED: NetGet only \
                      logs the limit, it does not configure tc/iptables, so the peer's traffic \
                      is unaffected. Use disconnect_peer if the peer must actually be stopped."
            .to_string(),
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
        log_template: Some(
            LogTemplate::new()
                .with_info("-> WG traffic limit {public_key}")
                .with_debug("WG set_peer_traffic_limit: pubkey={public_key} mbps={limit_mbps}"),
        ),
    }
}

/// Action: Disconnect peer immediately
fn disconnect_peer_action() -> ActionDefinition {
    ActionDefinition {
        name: "disconnect_peer".to_string(),
        description: "Immediately remove a WireGuard peer from the interface, tearing down its \
                      tunnel. The peer can re-handshake unless it is also kept out by policy."
            .to_string(),
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
        log_template: Some(
            LogTemplate::new()
                .with_info("-> WG disconnect {public_key}")
                .with_debug("WG disconnect_peer: pubkey={public_key} reason={reason}"),
        ),
    }
}
