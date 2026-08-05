//! WebRTC server protocol actions implementation
//!
//! # Status: Incomplete — this server cannot carry a byte
//!
//! `WebRtcServer::spawn_with_llm_actions` binds nothing, spawns nothing and registers no
//! task. The only entry point that could create a peer connection is
//! [`crate::server::webrtc::WebRtcServerData::accept_offer`], and **nothing calls it**:
//! there is no code path anywhere in NetGet that delivers an SDP offer to this protocol.
//! `webrtc_offer_received` therefore never fires, no `RTCPeerConnection` is ever created,
//! no data channel ever opens, and `webrtc_peer_connected` / `webrtc_message_received`
//! are unreachable for the same reason.
//!
//! Four async actions (`accept_offer`, `send_to_peer`, `close_peer`, `list_peers`) used to
//! be advertised here. Each returned `ActionResult::Custom`, and — unlike postgresql, s3,
//! grpc or couchdb, which match on `Custom` in their own server loop — this protocol has
//! no loop to match on it, so the result was constructed and dropped. They have been
//! removed rather than left advertising a capability that never existed.
//!
//! `DevelopmentState` is `Incomplete`, so the protocol is hidden from LLM prompts unless
//! `--include-disabled-protocols` is passed. Making it real needs, at minimum: a signaling
//! transport that feeds offers in (the sibling `webrtc_signaling` server, or a startup
//! parameter), a spawned task that owns `WebRtcServerData` and is registered via
//! `AppState::register_server_task`, and an async-action path that can reach that task.

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::log_template::LogTemplate;
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// WebRTC peer connected event (data channel opened)
///
/// Unreachable: see the module header. Kept because the (also unreachable) data-channel
/// callback in `mod.rs` references it.
pub static WEBRTC_PEER_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "webrtc_peer_connected",
        "WebRTC data channel opened and ready to send messages",
        json!({
            "type": "send_message",
            "message": "Welcome to NetGet WebRTC server!"
        }),
    )
    .with_actions(sync_actions())
    .with_parameters(vec![
        Parameter {
            name: "peer_id".to_string(),
            type_hint: "string".to_string(),
            description: "Unique peer identifier".to_string(),
            required: true,
        },
        Parameter {
            name: "channel_label".to_string(),
            type_hint: "string".to_string(),
            description: "Data channel label".to_string(),
            required: true,
        },
    ])
    .with_log_template(
        LogTemplate::new()
            .with_info("WebRTC peer {peer_id} connected (channel: {channel_label})")
            .with_debug("WebRTC peer_id={peer_id} channel={channel_label}")
            .with_trace("WebRTC connected: {json_pretty(.)}"),
    )
});

/// WebRTC message received event
///
/// Unreachable: see the module header.
pub static WEBRTC_MESSAGE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "webrtc_message_received",
        "Message received from WebRTC peer",
        json!({
            "type": "send_message",
            "message": "Message received"
        }),
    )
    .with_actions(sync_actions())
    .with_parameters(vec![
        Parameter {
            name: "peer_id".to_string(),
            type_hint: "string".to_string(),
            description: "Unique peer identifier".to_string(),
            required: true,
        },
        Parameter {
            name: "message".to_string(),
            type_hint: "string".to_string(),
            description: "Received message text".to_string(),
            required: true,
        },
        Parameter {
            name: "is_binary".to_string(),
            type_hint: "boolean".to_string(),
            description: "Whether message is binary".to_string(),
            required: true,
        },
    ])
    .with_log_template(
        LogTemplate::new()
            .with_info("WebRTC {peer_id} <- {preview(message,50)}")
            .with_debug("WebRTC message from peer_id={peer_id} binary={is_binary}")
            .with_trace("WebRTC message: {json_pretty(.)}"),
    )
});

// `WEBRTC_OFFER_RECEIVED_EVENT` and `WEBRTC_PEER_DISCONNECTED_EVENT` used to live here.
// Neither could ever fire: no code path delivers an SDP offer, and the
// `on_peer_connection_state_change` handler cleans up without calling the LLM. The
// disconnect event's response example was `{"type": "no_action"}`, an action that exists
// nowhere in NetGet — response examples are rendered verbatim into the prompt, so it was
// teaching the model a call it could only be rejected for making.
//
// The actions a reachable data-channel event could use. Declared once so
// `get_sync_actions` and every event's `with_actions` cannot drift apart.
fn sync_actions() -> Vec<ActionDefinition> {
    vec![
        ActionDefinition {
            name: "send_message".to_string(),
            description: "Send a message in response to received data".to_string(),
            parameters: vec![Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Message text to send".to_string(),
                required: true,
            }],
            example: json!({
                "type": "send_message",
                "message": "Reply message"
            }),
            log_template: Some(
                LogTemplate::new()
                    .with_info("-> WebRTC message")
                    .with_debug("WebRTC send_message"),
            ),
        },
        ActionDefinition {
            name: "disconnect".to_string(),
            description: "Close the peer connection".to_string(),
            parameters: vec![],
            example: json!({
                "type": "disconnect"
            }),
            log_template: Some(
                LogTemplate::new()
                    .with_info("-> WebRTC disconnect")
                    .with_debug("WebRTC disconnect"),
            ),
        },
        ActionDefinition {
            name: "wait_for_more".to_string(),
            description: "Wait for more messages before responding".to_string(),
            parameters: vec![],
            example: json!({
                "type": "wait_for_more"
            }),
            log_template: Some(
                LogTemplate::new()
                    .with_info("-> WebRTC wait")
                    .with_debug("WebRTC wait_for_more"),
            ),
        },
    ]
}

/// WebRTC server protocol action handler
pub struct WebRtcProtocol;

impl WebRtcProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebRtcProtocol {
    fn default() -> Self {
        Self::new()
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for WebRtcProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "ice_servers".to_string(),
                description: "STUN/TURN servers for ICE (default: Google STUN)".to_string(),
                type_hint: "array".to_string(),
                required: false,
                example: json!(["stun:stun.l.google.com:19302", "turn:turn.example.com:3478"]),
            },
            ParameterDefinition {
                name: "signaling_mode".to_string(),
                description: "Signaling mode: 'manual' (default) or 'websocket'".to_string(),
                type_hint: "string".to_string(),
                required: false,
                example: json!("manual"),
            },
        ]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // Deliberately empty. accept_offer, send_to_peer, close_peer and list_peers were
        // advertised here; each built an `ActionResult::Custom` that no code in this
        // protocol ever consumed, so invoking one did precisely nothing while reporting
        // success. See the module header.
        Vec::new()
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        sync_actions()
    }

    fn protocol_name(&self) -> &'static str {
        "WebRTC"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        // This used to build four *fresh* EventTypes with `{"type": "placeholder"}`
        // response examples, entirely disconnected from the LazyLock statics the server
        // actually fires. Both copies were wrong in different ways; return the real ones.
        vec![
            WEBRTC_PEER_CONNECTED_EVENT.clone(),
            WEBRTC_MESSAGE_RECEIVED_EVENT.clone(),
        ]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>DTLS>SCTP>DataChannel"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec![
            "webrtc",
            "webrtc server",
            "data channel",
            "peer to peer",
            "p2p",
        ]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Incomplete)
            .implementation(
                "webrtc-rs data-channel scaffolding that is never started: spawn() binds \
                 nothing and no code path delivers an SDP offer",
            )
            .llm_control("None reachable - no event can fire")
            .e2e_testing("None - the E2E suite only asserts that the process starts")
            .notes(
                "Not a working WebRTC server. WebRtcServerData::accept_offer has no \
                 caller, so no peer connection or data channel is ever created and no \
                 message can be sent or received.",
            )
            .build()
    }

    fn description(&self) -> &'static str {
        "WebRTC data-channel scaffolding (INCOMPLETE - no signaling path, cannot connect)"
    }

    fn example_prompt(&self) -> &'static str {
        "Open WebRTC server accepting peer connections (manual SDP exchange)"
    }

    fn group_name(&self) -> &'static str {
        "Real-time"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        StartupExamples::new(
            // LLM mode
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "webrtc",
                "instruction": "WebRTC data channel server. Accept peer connections via manual SDP exchange. Echo messages back to connected peers."
            }),
            // Script mode
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "webrtc",
                "event_handlers": [{
                    "event_pattern": "webrtc_message_received",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<webrtc_handler>"
                    }
                }]
            }),
            // Static mode
            json!({
                "type": "open_server",
                "port": 0,
                "base_stack": "webrtc",
                "event_handlers": [{
                    "event_pattern": "webrtc_peer_connected",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "send_message",
                            "message": "Welcome to NetGet WebRTC server!"
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for WebRtcProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::webrtc::WebRtcServer;
            WebRtcServer::spawn_with_llm_actions(
                ctx.legacy_listen_addr(),
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
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
            // accept_offer / send_to_peer / close_peer / list_peers used to be handled
            // here, each returning an ActionResult::Custom that nothing consumed.
            "send_message" => {
                // This is a sync action for connection context
                let message = action
                    .get("message")
                    .and_then(|v| v.as_str())
                    .context("Missing 'message' field")?
                    .to_string();

                Ok(ActionResult::Output(message.into_bytes()))
            }
            "disconnect" => Ok(ActionResult::CloseConnection),
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!(
                "Unknown WebRTC server action: {}",
                action_type
            )),
        }
    }
}
