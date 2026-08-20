//! OpenVPN control-plane responder: events and actions.
//!
//! The server owns the wire format; the model owns exactly one policy decision —
//! whether to answer a peer's session reset at all. That decision is enforced:
//! `accept_peer` is the only thing that causes a
//! `P_CONTROL_HARD_RESET_SERVER_V2` to be sent, and `reject_peer`, an empty
//! answer, or an LLM error all leave the peer unanswered.
//!
//! Nothing here promises a tunnel, because this server cannot build one: it has
//! no TLS control channel, no key exchange and no data channel. See
//! `src/server/openvpn/mod.rs`.

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

/// Name of the `ActionResult::Custom` the server looks for when deciding
/// whether to answer a peer. Kept next to the actions that produce it so the
/// producer and the consumer cannot drift apart.
pub const PEER_DECISION_RESULT: &str = "openvpn_peer_decision";

/// A client began an OpenVPN handshake. The one point where policy applies.
pub static OPENVPN_PEER_RESET_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "openvpn_peer_reset",
        "An OpenVPN client sent a session reset (P_CONTROL_HARD_RESET_CLIENT_V1/V2) - the \
         first packet of a handshake. Decide whether to answer it. Reply with accept_peer to \
         send P_CONTROL_HARD_RESET_SERVER_V2 and begin tracking the peer, or reject_peer to \
         stay silent and drop it. If you reply with neither, the peer is left unanswered. \
         Note this server is a control-plane responder: even an accepted peer never gets a \
         tunnel, because there is no TLS control channel, no key exchange and no data \
         channel.",
        json!({
            "type": "accept_peer",
            "reason": "Answer the handshake and observe what the client sends next"
        }),
    )
    .with_actions(vec![accept_peer_action(), reject_peer_action()])
    .with_log_template(
        LogTemplate::new()
            .with_info("{client_ip} OpenVPN reset (session {client_session_id})")
            .with_debug(
                "OpenVPN {reset_type} from {client_ip}, session {client_session_id}, \
                 key_id {key_id}",
            )
            .with_trace("OpenVPN peer reset: {json_pretty(.)}"),
    )
});

/// Get all OpenVPN event types.
///
/// One event, and it is the only one raised. Control packets after the reset
/// are acknowledged by the server without consulting the model: a client
/// retransmits them, so an event per packet would spend model calls on
/// duplicates while changing nothing this server is able to do.
pub fn get_openvpn_event_types() -> Vec<EventType> {
    vec![OPENVPN_PEER_RESET_EVENT.clone()]
}

/// OpenVPN protocol implementation.
pub struct OpenvpnProtocol;

impl OpenvpnProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Default for OpenvpnProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl Protocol for OpenvpnProtocol {
    /// No user-triggered actions.
    ///
    /// The action executor builds a stateless `OpenvpnProtocol` with no handle
    /// to the running server, so anything listed here could only return
    /// `NoAction`. Listing peer management the executor cannot perform would be
    /// a promise the protocol cannot keep.
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![accept_peer_action(), reject_peer_action()]
    }

    fn protocol_name(&self) -> &'static str {
        "OpenVPN"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_openvpn_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>OPENVPN"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["openvpn"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{
            DevelopmentState, PrivilegeRequirement, ProtocolMetadataV2,
        };

        ProtocolMetadataV2::builder()
            .connectionless()
            // Experimental, not Incomplete: what it claims to do is implemented
            // and was checked against a real client. It is not Beta because it
            // implements only the front of the protocol - no client can use it
            // as a VPN.
            .state(DevelopmentState::Experimental)
            // No TUN device and no privileged port by default (1194 is
            // unprivileged), so nothing here needs elevation. Declaring Root, as
            // this protocol used to, made it unstartable for no benefit.
            .privilege_requirement(PrivilegeRequirement::None)
            .implementation(
                "Control plane only. Decodes the OpenVPN UDP wire format (P_CONTROL_*, \
                 P_ACK_V1, P_DATA_V1/V2), answers P_CONTROL_HARD_RESET_CLIENT_V1/V2 with \
                 P_CONTROL_HARD_RESET_SERVER_V2, and ACKs the client's control packets. \
                 There is NO TLS control channel, NO key exchange, NO data channel and NO \
                 TUN device, so no tunnel is ever established and no traffic is carried. \
                 --tls-auth, --tls-crypt and --tls-crypt-v2 clients are detected and \
                 refused rather than mis-parsed.",
            )
            .llm_control(
                "One event, openvpn_peer_reset, raised once per handshake. accept_peer sends \
                 the reset reply; reject_peer stays silent. The decision is enforced, and no \
                 decision means no reply.",
            )
            .e2e_testing(
                "Wire format pinned to frames captured from OpenVPN 2.7.4 and decoded by a \
                 hand-written decoder in the test, not by this codec. A live openvpn client \
                 is driven against the server and must log 'TLS: Initial packet from', which \
                 it emits only after accepting our reset reply. That client then times out, \
                 as expected: the handshake cannot proceed past the reset.",
            )
            .notes(
                "NOT A VPN - it never carries traffic. Useful as an OpenVPN honeypot and \
                 protocol observatory: it identifies who probes UDP/1194 and captures the \
                 client's TLS ClientHello. Use WireGuard for a working tunnel.",
            )
            .build()
    }

    fn description(&self) -> &'static str {
        "OpenVPN control-plane responder / honeypot (answers the session reset; no tunnel)"
    }

    fn example_prompt(&self) -> &'static str {
        "Start an OpenVPN honeypot on port 1194 and log everyone who probes it"
    }

    fn group_name(&self) -> &'static str {
        "VPN & Routing"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        StartupExamples::new(
            // LLM mode: the model decides per peer.
            json!({
                "type": "open_server",
                "port": 1194,
                "base_stack": "openvpn",
                "instruction": "OpenVPN honeypot. Answer every handshake with accept_peer and record the peer address and session id so the probe is logged."
            }),
            // Script mode: deterministic accept, no model call per peer.
            json!({
                "type": "open_server",
                "port": 1194,
                "base_stack": "openvpn",
                "event_handlers": [{
                    "event_pattern": "openvpn_peer_reset",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "return {'type': 'accept_peer', 'reason': 'honeypot'}"
                    }
                }]
            }),
            // Static mode: fixed decision.
            json!({
                "type": "open_server",
                "port": 1194,
                "base_stack": "openvpn",
                "event_handlers": [{
                    "event_pattern": "openvpn_peer_reset",
                    "handler": {
                        "type": "static",
                        "actions": [{"type": "accept_peer"}]
                    }
                }]
            }),
        )
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

        let reason = action
            .get("reason")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        match action_type {
            "accept_peer" => Ok(decision_result(true, reason)),
            "reject_peer" => Ok(decision_result(false, reason)),
            _ => Err(anyhow::anyhow!("Unknown OpenVPN action: {}", action_type)),
        }
    }
}

/// Encode a peer decision for the server loop to act on.
fn decision_result(accept: bool, reason: Option<String>) -> ActionResult {
    ActionResult::Custom {
        name: PEER_DECISION_RESULT.to_string(),
        data: json!({ "accept": accept, "reason": reason }),
    }
}

/// Action: answer the handshake.
fn accept_peer_action() -> ActionDefinition {
    ActionDefinition {
        name: "accept_peer".to_string(),
        description: "Answer this peer's session reset with P_CONTROL_HARD_RESET_SERVER_V2 and \
                      start tracking it, acknowledging the control packets it sends next. This \
                      is enforced: without it, nothing is sent to the peer. It does not create \
                      a tunnel - this server has no TLS control channel or data channel, so an \
                      accepted client's handshake stalls after its ClientHello."
            .to_string(),
        parameters: vec![Parameter {
            name: "reason".to_string(),
            type_hint: "string".to_string(),
            description: "Why this peer is being answered (recorded in the log)".to_string(),
            required: false,
        }],
        example: json!({
            "type": "accept_peer",
            "reason": "Observe what the client sends after the reset"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> OpenVPN answer reset ({reason})")
                .with_debug("OpenVPN accept_peer: {reason}"),
        ),
    }
}

/// Action: stay silent.
fn reject_peer_action() -> ActionDefinition {
    ActionDefinition {
        name: "reject_peer".to_string(),
        description: "Refuse this peer: send nothing at all and drop it. This is enforced. \
                      OpenVPN has no reject packet at reset time, so silence is the refusal; \
                      the client will retry and then give up."
            .to_string(),
        parameters: vec![Parameter {
            name: "reason".to_string(),
            type_hint: "string".to_string(),
            description: "Why this peer is being refused (recorded in the log)".to_string(),
            required: false,
        }],
        example: json!({
            "type": "reject_peer",
            "reason": "Source address is not on the allow list"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> OpenVPN refuse peer ({reason})")
                .with_debug("OpenVPN reject_peer: {reason}"),
        ),
    }
}
