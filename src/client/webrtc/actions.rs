//! WebRTC client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    protocol_trait::Protocol,
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// WebRTC client connected event (data channel opened)
pub static WEBRTC_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "webrtc_connected",
        "WebRTC data channel opened and ready to send messages",
    )
    .with_parameters(vec![Parameter {
        name: "channel_label".to_string(),
        type_hint: "string".to_string(),
        description: "Data channel label".to_string(),
        required: true,
    }])
});

/// WebRTC client message received event
pub static WEBRTC_CLIENT_MESSAGE_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "webrtc_message_received",
        "Message received from WebRTC peer",
    )
    .with_parameters(vec![
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

/// WebRTC client protocol action handler
pub struct WebRtcClientProtocol;

impl WebRtcClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for WebRtcClientProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![ParameterDefinition {
            name: "ice_servers".to_string(),
            description: "STUN/TURN servers for ICE (default: Google STUN)".to_string(),
            type_hint: "array".to_string(),
            required: false,
            example: json!(["stun:stun.l.google.com:19302", "turn:turn.example.com:3478"]),
        }]
    }
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "send_message".to_string(),
                description: "Send a text message over the data channel".to_string(),
                parameters: vec![Parameter {
                    name: "message".to_string(),
                    type_hint: "string".to_string(),
                    description: "Message text to send".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "send_message",
                    "message": "Hello, WebRTC peer!"
                }),
            },
            ActionDefinition {
                name: "apply_answer".to_string(),
                description: "Apply the SDP answer from the remote peer to complete connection"
                    .to_string(),
                parameters: vec![Parameter {
                    name: "answer_json".to_string(),
                    type_hint: "string".to_string(),
                    description: "SDP answer JSON from peer".to_string(),
                    required: true,
                }],
                example: json!({
                    "type": "apply_answer",
                    "answer_json": "{\"type\":\"answer\",\"sdp\":\"...\"}"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Close the WebRTC data channel and peer connection".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "disconnect"
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
                description: "Close the connection".to_string(),
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
                id: "webrtc_connected".to_string(),
                description: "Triggered when WebRTC data channel opens".to_string(),
                actions: vec![],
                parameters: vec![],
            },
            EventType {
                id: "webrtc_message_received".to_string(),
                description: "Triggered when a message is received".to_string(),
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
            "webrtc client",
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
            .llm_control("Full control over data channel messages and SDP exchange")
            .e2e_testing("Manual SDP exchange with local peer or test server")
            .build()
    }
    fn description(&self) -> &'static str {
        "WebRTC client for peer-to-peer data channels (text messaging, no audio/video)"
    }
    fn example_prompt(&self) -> &'static str {
        "Open WebRTC client for peer-to-peer messaging (you'll need to exchange SDP with peer)"
    }
    fn group_name(&self) -> &'static str {
        "Real-time"
    }
}

// Implement Client trait (client-specific functionality)
impl Client for WebRtcClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::webrtc::WebRtcClient;
            WebRtcClient::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.client_id,
            )
            .await
        })
    }
    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "send_message" => {
                let message = action
                    .get("message")
                    .and_then(|v| v.as_str())
                    .context("Missing 'message' field")?
                    .to_string();

                // Return text as bytes
                Ok(ClientActionResult::SendData(message.into_bytes()))
            }
            "apply_answer" => {
                let answer_json = action
                    .get("answer_json")
                    .and_then(|v| v.as_str())
                    .context("Missing 'answer_json' field")?
                    .to_string();

                // Return custom result for async processing
                Ok(ClientActionResult::Custom {
                    name: "apply_answer".to_string(),
                    data: json!({
                        "answer_json": answer_json,
                    }),
                })
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            "wait_for_more" => Ok(ClientActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!(
                "Unknown WebRTC client action: {}",
                action_type
            )),
        }
    }
}
