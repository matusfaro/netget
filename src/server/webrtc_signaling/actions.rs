//! WebRTC Signaling Server protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// WebRTC signaling peer connected event
pub static WEBRTC_SIGNALING_PEER_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "webrtc_signaling_peer_connected",
        "WebRTC signaling peer registered with peer ID",
        json!({
            "type": "no_action"
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "peer_id".to_string(),
            type_hint: "string".to_string(),
            description: "Unique peer identifier".to_string(),
            required: true,
        },
        Parameter {
            name: "remote_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Remote address of signaling peer".to_string(),
            required: true,
        },
    ])
});

/// WebRTC signaling peer disconnected event
pub static WEBRTC_SIGNALING_PEER_DISCONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "webrtc_signaling_peer_disconnected",
        "WebRTC signaling peer disconnected",
        json!({
            "type": "no_action"
        }),
    )
    .with_parameters(vec![Parameter {
        name: "peer_id".to_string(),
        type_hint: "string".to_string(),
        description: "Unique peer identifier".to_string(),
        required: true,
    }])
});

/// WebRTC signaling message received event
pub static WEBRTC_SIGNALING_MESSAGE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "webrtc_signaling_message_received",
        "Signaling message received from peer",
        json!({
            "type": "no_action"
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "peer_id".to_string(),
            type_hint: "string".to_string(),
            description: "Sender peer identifier".to_string(),
            required: true,
        },
        Parameter {
            name: "message_type".to_string(),
            type_hint: "string".to_string(),
            description: "Message type (offer, answer, ice_candidate)".to_string(),
            required: true,
        },
        Parameter {
            name: "target_peer".to_string(),
            type_hint: "string".to_string(),
            description: "Target peer identifier".to_string(),
            required: false,
        },
    ])
});

/// WebRTC Signaling Server protocol action handler
pub struct WebRtcSignalingProtocol;

impl WebRtcSignalingProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebRtcSignalingProtocol {
    fn default() -> Self {
        Self::new()
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for WebRtcSignalingProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "list_signaling_peers".to_string(),
                description: "List all connected signaling peers".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "list_signaling_peers"
                }),
            },
            ActionDefinition {
                name: "broadcast_message".to_string(),
                description: "Broadcast a message to all connected signaling peers".to_string(),
                parameters: vec![Parameter {
                    name: "message".to_string(),
                    type_hint: "object".to_string(),
                    description: "Message to broadcast (JSON object)".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "broadcast_message",
                    "message": {"type": "announcement", "text": "Server restarting in 5 minutes"}
                }),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![]
    }

    fn protocol_name(&self) -> &'static str {
        "WebRTC Signaling"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType::new("webrtc_signaling_peer_connected", "Triggered when a peer registers with the signaling server", json!({"type": "placeholder", "event_id": "webrtc_signaling_peer_connected"})),
            EventType::new("webrtc_signaling_peer_disconnected", "Triggered when a peer disconnects from the signaling server", json!({"type": "placeholder", "event_id": "webrtc_signaling_peer_disconnected"})),
            EventType::new("webrtc_signaling_message_received", "Triggered when a signaling message is received", json!({"type": "placeholder", "event_id": "webrtc_signaling_message_received"})),
        ]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>WebSocket"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec![
            "webrtc signaling",
            "signaling server",
            "sdp relay",
            "websocket signaling",
        ]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("WebSocket-based signaling server for WebRTC SDP exchange")
            .llm_control("Monitor peer connections and signaling message flow")
            .e2e_testing("WebSocket client connections with SDP offer/answer relay")
            .build()
    }

    fn description(&self) -> &'static str {
        "WebRTC signaling server for automatic SDP and ICE candidate exchange via WebSocket"
    }

    fn example_prompt(&self) -> &'static str {
        "Open WebRTC signaling server to help WebRTC peers exchange SDP offers and answers"
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
                "port": 8080,
                "base_stack": "WebRTC Signaling",
                "instruction": "WebRTC signaling server. Relay SDP offers and answers between peers. Log all peer connections and signaling messages."
            }),
            // Script mode
            json!({
                "type": "open_server",
                "port": 8080,
                "base_stack": "WebRTC Signaling",
                "event_handlers": [{
                    "event_pattern": "webrtc_signaling_peer_connected",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<protocol_handler>"
                    }
                }, {
                    "event_pattern": "webrtc_signaling_message_received",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<protocol_handler>"
                    }
                }]
            }),
            // Static mode
            json!({
                "type": "open_server",
                "port": 8080,
                "base_stack": "WebRTC Signaling",
                "event_handlers": [{
                    "event_pattern": "webrtc_signaling_peer_connected",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "no_action"
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for WebRtcSignalingProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::webrtc_signaling::WebRtcSignalingServer;
            WebRtcSignalingServer::spawn_with_llm_actions(
                ctx.listen_addr,
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
            "list_signaling_peers" => {
                // Return custom action for manual processing
                Ok(ActionResult::Custom {
                    name: "list_signaling_peers".to_string(),
                    data: json!({}),
                })
            }
            "broadcast_message" => {
                let message = action
                    .get("message")
                    .context("Missing 'message' field")?
                    .clone();

                // Return custom action for manual processing
                Ok(ActionResult::Custom {
                    name: "broadcast_message".to_string(),
                    data: json!({
                        "message": message,
                    }),
                })
            }
            _ => Err(anyhow::anyhow!(
                "Unknown WebRTC Signaling action: {}",
                action_type
            )),
        }
    }
}
