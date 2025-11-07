//! BitTorrent Peer Wire Protocol client actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// Peer handshake event
pub static PEER_HANDSHAKE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "peer_handshake",
        "Received handshake from BitTorrent peer"
    )
    .with_parameters(vec![
        Parameter {
            name: "info_hash".to_string(),
            type_hint: "string".to_string(),
            description: "Info hash (hex)".to_string(),
            required: true,
        },
        Parameter {
            name: "peer_id".to_string(),
            type_hint: "string".to_string(),
            description: "Peer ID (hex)".to_string(),
            required: true,
        },
        Parameter {
            name: "reserved".to_string(),
            type_hint: "string".to_string(),
            description: "Reserved bytes (hex)".to_string(),
            required: false,
        },
    ])
});

/// Peer message event
pub static PEER_MESSAGE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "peer_message",
        "Received message from BitTorrent peer"
    )
    .with_parameters(vec![
        Parameter {
            name: "message_type".to_string(),
            type_hint: "number".to_string(),
            description: "Message type (0=choke, 1=unchoke, 2=interested, 3=not_interested, 4=have, 5=bitfield, 6=request, 7=piece, 8=cancel, 9=port)".to_string(),
            required: true,
        },
        Parameter {
            name: "payload_len".to_string(),
            type_hint: "number".to_string(),
            description: "Payload length".to_string(),
            required: true,
        },
        Parameter {
            name: "payload_hex".to_string(),
            type_hint: "string".to_string(),
            description: "Payload data (hex)".to_string(),
            required: false,
        },
    ])
});

/// BitTorrent Peer Wire Protocol client protocol action handler
pub struct TorrentPeerClientProtocol;

impl TorrentPeerClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Client for TorrentPeerClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::torrent_peer::TorrentPeerClient;
            TorrentPeerClient::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.client_id,
            )
            .await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "peer_handshake".to_string(),
                description: "Send handshake to peer".to_string(),
                parameters: vec![
                    Parameter {
                        name: "info_hash".to_string(),
                        type_hint: "string".to_string(),
                        description: "Info hash (40 char hex)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "peer_id".to_string(),
                        type_hint: "string".to_string(),
                        description: "Our peer ID (40 char hex)".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "peer_handshake",
                    "info_hash": "0123456789abcdef0123456789abcdef01234567",
                    "peer_id": "abcdef0123456789abcdef0123456789abcdef01"
                }),
            },
            ActionDefinition {
                name: "peer_interested".to_string(),
                description: "Send interested message".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "peer_message",
                    "message_type": 2,
                    "payload": ""
                }),
            },
            ActionDefinition {
                name: "peer_not_interested".to_string(),
                description: "Send not interested message".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "peer_message",
                    "message_type": 3,
                    "payload": ""
                }),
            },
            ActionDefinition {
                name: "peer_request_piece".to_string(),
                description: "Request a piece from peer".to_string(),
                parameters: vec![
                    Parameter {
                        name: "index".to_string(),
                        type_hint: "number".to_string(),
                        description: "Piece index".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "begin".to_string(),
                        type_hint: "number".to_string(),
                        description: "Byte offset within piece".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "length".to_string(),
                        type_hint: "number".to_string(),
                        description: "Block length to request".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "peer_message",
                    "message_type": 6,
                    "payload": "00000000000000000000400"
                }),
            },
            ActionDefinition {
                name: "peer_send_piece".to_string(),
                description: "Send a piece to peer".to_string(),
                parameters: vec![
                    Parameter {
                        name: "index".to_string(),
                        type_hint: "number".to_string(),
                        description: "Piece index".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "begin".to_string(),
                        type_hint: "number".to_string(),
                        description: "Byte offset within piece".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "block".to_string(),
                        type_hint: "string".to_string(),
                        description: "Block data (hex)".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "peer_message",
                    "message_type": 7,
                    "payload": "0000000000000000abcdef..."
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from peer".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "disconnect"
                }),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "peer_handshake" => {
                Ok(ClientActionResult::Custom {
                    name: "peer_handshake".to_string(),
                    data: action,
                })
            }
            "peer_message" => {
                Ok(ClientActionResult::Custom {
                    name: "peer_message".to_string(),
                    data: action,
                })
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            _ => Err(anyhow::anyhow!("Unknown Peer client action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "BitTorrent Peer Wire"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType {
                id: "peer_handshake".to_string(),
                description: "Received handshake from peer".to_string(),
                actions: vec![],
                parameters: vec![],
            },
            EventType {
                id: "peer_message".to_string(),
                description: "Received message from peer".to_string(),
                actions: vec![],
                parameters: vec![],
            },
        ]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>BitTorrent-PeerWire"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["bittorrent", "peer", "peer wire", "torrent peer"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("TCP-based peer wire protocol with handshake and message framing")
            .llm_control("Full control over peer messages (choke, unchoke, interested, request, piece, etc.)")
            .e2e_testing("Mock peer server")
            .build()
    }

    fn description(&self) -> &'static str {
        "BitTorrent Peer Wire Protocol client for peer-to-peer data transfer"
    }

    fn example_prompt(&self) -> &'static str {
        "Connect to peer at 192.168.1.100:6881 and exchange pieces for info_hash xyz"
    }

    fn group_name(&self) -> &'static str {
        "P2P"
    }
}
