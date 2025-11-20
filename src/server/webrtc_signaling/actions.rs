//! WebRTC Signaling Server protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::{Arc, LazyLock};

/// WebRTC signaling peer connected event
pub static WEBRTC_SIGNALING_PEER_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "webrtc_signaling_peer_connected",
        "WebRTC signaling peer registered with peer ID",
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
            EventType {
                id: "webrtc_signaling_peer_connected".to_string(),
                description: "Triggered when a peer registers with the signaling server"
                    .to_string(),
                actions: vec![],
                parameters: vec![],
            },
            EventType {
                id: "webrtc_signaling_peer_disconnected".to_string(),
                description: "Triggered when a peer disconnects from the signaling server"
                    .to_string(),
                actions: vec![],
                parameters: vec![],
            },
            EventType {
                id: "webrtc_signaling_message_received".to_string(),
                description: "Triggered when a signaling message is received".to_string(),
                actions: vec![],
                parameters: vec![],
            },
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

    fn execute_action(
        &self,
        action: serde_json::Value,
        ctx: &crate::protocol::ExecutionContext,
    ) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "list_signaling_peers" => {
                // Get server data from context
                let server_data_ptr = ctx
                    .state
                    .with_server(ctx.server_id, |server| {
                        server
                            .get_protocol_field("server_data_ptr")
                            .and_then(|v| v.as_u64())
                            .map(|p| p as usize)
                    })
                    .context("Server not found")?
                    .context("server_data_ptr not found")?;

                // Reconstruct Arc (temporarily)
                let server_data = unsafe {
                    Arc::from_raw(
                        server_data_ptr
                            as *const crate::server::webrtc_signaling::WebRtcSignalingServerData,
                    )
                };
                let server_data_clone = Arc::clone(&server_data);
                // Prevent drop
                let _ = Arc::into_raw(server_data);

                // List peers (spawn async task and print to console)
                tokio::spawn(async move {
                    let peers = server_data_clone.list_peers().await;
                    tracing::info!("Connected signaling peers: {:?}", peers);
                });

                Ok(ActionResult::NoOp)
            }
            "broadcast_message" => {
                let _message = action
                    .get("message")
                    .context("Missing 'message' field")?;

                // TODO: Implement broadcast functionality
                tracing::warn!("Broadcast message action not yet implemented");
                Ok(ActionResult::NoOp)
            }
            _ => Err(anyhow::anyhow!(
                "Unknown WebRTC Signaling action: {}",
                action_type
            )),
        }
    }
}
