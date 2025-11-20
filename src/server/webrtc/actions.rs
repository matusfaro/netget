//! WebRTC server protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::{Arc, LazyLock};

/// WebRTC peer connected event (data channel opened)
pub static WEBRTC_PEER_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "webrtc_peer_connected",
        "WebRTC data channel opened and ready to send messages",
    )
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
});

/// WebRTC message received event
pub static WEBRTC_MESSAGE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "webrtc_message_received",
        "Message received from WebRTC peer",
    )
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
});

/// WebRTC offer received event (manual signaling mode)
pub static WEBRTC_OFFER_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "webrtc_offer_received",
        "SDP offer received from peer (manual signaling)",
    )
    .with_parameters(vec![
        Parameter {
            name: "peer_id".to_string(),
            type_hint: "string".to_string(),
            description: "Unique peer identifier".to_string(),
            required: true,
        },
        Parameter {
            name: "sdp_offer".to_string(),
            type_hint: "string".to_string(),
            description: "SDP offer JSON from peer".to_string(),
            required: true,
        },
    ])
});

/// WebRTC peer disconnected event
pub static WEBRTC_PEER_DISCONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "webrtc_peer_disconnected",
        "WebRTC peer connection closed",
    )
    .with_parameters(vec![
        Parameter {
            name: "peer_id".to_string(),
            type_hint: "string".to_string(),
            description: "Unique peer identifier".to_string(),
            required: true,
        },
        Parameter {
            name: "reason".to_string(),
            type_hint: "string".to_string(),
            description: "Disconnect reason".to_string(),
            required: false,
        },
    ])
});

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
        vec![
            ActionDefinition {
                name: "accept_offer".to_string(),
                description: "Accept an SDP offer from a peer and generate an answer".to_string(),
                parameters: vec![
                    Parameter {
                        name: "peer_id".to_string(),
                        type_hint: "string".to_string(),
                        description: "Unique identifier for this peer".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "sdp_offer".to_string(),
                        type_hint: "string".to_string(),
                        description: "SDP offer JSON from peer".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "accept_offer",
                    "peer_id": "peer-abc123",
                    "sdp_offer": "{\"type\":\"offer\",\"sdp\":\"...\"}"
                }),
            },
            ActionDefinition {
                name: "send_to_peer".to_string(),
                description: "Send a message to a specific peer".to_string(),
                parameters: vec![
                    Parameter {
                        name: "peer_id".to_string(),
                        type_hint: "string".to_string(),
                        description: "Target peer identifier".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "message".to_string(),
                        type_hint: "string".to_string(),
                        description: "Message text to send".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "send_to_peer",
                    "peer_id": "peer-abc123",
                    "message": "Hello from NetGet server!"
                }),
            },
            ActionDefinition {
                name: "close_peer".to_string(),
                description: "Close connection to a specific peer".to_string(),
                parameters: vec![Parameter {
                    name: "peer_id".to_string(),
                    type_hint: "string".to_string(),
                    description: "Target peer identifier".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "close_peer",
                    "peer_id": "peer-abc123"
                }),
            },
            ActionDefinition {
                name: "list_peers".to_string(),
                description: "List all active peer connections".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "list_peers"
                }),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
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
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Close the peer connection".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "disconnect"
                }),
            },
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait for more messages before responding".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "wait_for_more"
                }),
            },
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "WebRTC"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType {
                id: "webrtc_peer_connected".to_string(),
                description: "Triggered when a WebRTC peer's data channel opens".to_string(),
                actions: vec![],
                parameters: vec![],
            },
            EventType {
                id: "webrtc_message_received".to_string(),
                description: "Triggered when a message is received from a peer".to_string(),
                actions: vec![],
                parameters: vec![],
            },
            EventType {
                id: "webrtc_offer_received".to_string(),
                description: "Triggered when an SDP offer is received (manual mode)".to_string(),
                actions: vec![],
                parameters: vec![],
            },
            EventType {
                id: "webrtc_peer_disconnected".to_string(),
                description: "Triggered when a peer connection closes".to_string(),
                actions: vec![],
                parameters: vec![],
            },
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
            .state(DevelopmentState::Experimental)
            .implementation("webrtc-rs for data channels (no media)")
            .llm_control("Full control over peer connections and data channel messages")
            .e2e_testing("Manual SDP exchange with local peer or test server")
            .build()
    }

    fn description(&self) -> &'static str {
        "WebRTC server for peer-to-peer data channels (text messaging, no audio/video)"
    }

    fn example_prompt(&self) -> &'static str {
        "Open WebRTC server accepting peer connections (manual SDP exchange)"
    }

    fn group_name(&self) -> &'static str {
        "Real-time"
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
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            )
            .await
        })
    }

    fn execute_action(&self, action: serde_json::Value, ctx: &crate::protocol::ExecutionContext) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "accept_offer" => {
                let peer_id = action
                    .get("peer_id")
                    .and_then(|v| v.as_str())
                    .context("Missing 'peer_id' field")?
                    .to_string();

                let sdp_offer = action
                    .get("sdp_offer")
                    .and_then(|v| v.as_str())
                    .context("Missing 'sdp_offer' field")?
                    .to_string();

                // Return custom action for async processing
                Ok(ActionResult::Custom {
                    name: "accept_offer".to_string(),
                    data: json!({
                        "peer_id": peer_id,
                        "sdp_offer": sdp_offer,
                    }),
                })
            }
            "send_to_peer" => {
                let peer_id = action
                    .get("peer_id")
                    .and_then(|v| v.as_str())
                    .context("Missing 'peer_id' field")?;

                let message = action
                    .get("message")
                    .and_then(|v| v.as_str())
                    .context("Missing 'message' field")?;

                // Get server data from context
                let server_data_ptr = ctx.state
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
                    Arc::from_raw(server_data_ptr as *const crate::server::webrtc::WebRtcServerData)
                };
                let server_data_clone = Arc::clone(&server_data);
                // Prevent drop
                let _ = Arc::into_raw(server_data);

                // Send message (spawn async task)
                let peer_id = peer_id.to_string();
                let message = message.to_string();
                tokio::spawn(async move {
                    if let Err(e) = server_data_clone.send_to_peer(&peer_id, message).await {
                        tracing::error!("Failed to send to peer {}: {}", peer_id, e);
                    }
                });

                Ok(ActionResult::NoOp)
            }
            "close_peer" => {
                let peer_id = action
                    .get("peer_id")
                    .and_then(|v| v.as_str())
                    .context("Missing 'peer_id' field")?;

                // Get server data from context
                let server_data_ptr = ctx.state
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
                    Arc::from_raw(server_data_ptr as *const crate::server::webrtc::WebRtcServerData)
                };
                let server_data_clone = Arc::clone(&server_data);
                // Prevent drop
                let _ = Arc::into_raw(server_data);

                // Close peer (spawn async task)
                let peer_id = peer_id.to_string();
                tokio::spawn(async move {
                    if let Err(e) = server_data_clone.close_peer(&peer_id).await {
                        tracing::error!("Failed to close peer {}: {}", peer_id, e);
                    }
                });

                Ok(ActionResult::NoOp)
            }
            "list_peers" => {
                // Get server data from context
                let server_data_ptr = ctx.state
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
                    Arc::from_raw(server_data_ptr as *const crate::server::webrtc::WebRtcServerData)
                };
                let server_data_clone = Arc::clone(&server_data);
                // Prevent drop
                let _ = Arc::into_raw(server_data);

                // List peers (spawn async task and print to console)
                tokio::spawn(async move {
                    let peers = server_data_clone.list_peers().await;
                    tracing::info!("Active WebRTC peers: {:?}", peers);
                });

                Ok(ActionResult::NoOp)
            }
            "send_message" => {
                // This is a sync action for connection context
                let message = action
                    .get("message")
                    .and_then(|v| v.as_str())
                    .context("Missing 'message' field")?
                    .to_string();

                Ok(ActionResult::SendData(message.into_bytes()))
            }
            "disconnect" => Ok(ActionResult::Disconnect),
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!(
                "Unknown WebRTC server action: {}",
                action_type
            )),
        }
    }
}
