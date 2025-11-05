//! BitTorrent Peer Wire Protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

pub struct TorrentPeerProtocol;

impl TorrentPeerProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Server for TorrentPeerProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::torrent_peer::TorrentPeerServer;
            TorrentPeerServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            ).await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        Vec::new()
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            SEND_HANDSHAKE_ACTION.clone(),
            SEND_CHOKE_ACTION.clone(),
            SEND_UNCHOKE_ACTION.clone(),
            SEND_BITFIELD_ACTION.clone(),
            SEND_HAVE_ACTION.clone(),
            SEND_PIECE_ACTION.clone(),
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action.get("type").and_then(|v| v.as_str()).context("Missing 'type' field")?;

        match action_type {
            "send_handshake" => self.execute_send_handshake(action),
            "send_choke" => Ok(ActionResult::Output(vec![vec![0, 0, 0, 1, 0]])),
            "send_unchoke" => Ok(ActionResult::Output(vec![vec![0, 0, 0, 1, 1]])),
            "send_interested" => Ok(ActionResult::Output(vec![vec![0, 0, 0, 1, 2]])),
            "send_not_interested" => Ok(ActionResult::Output(vec![vec![0, 0, 0, 1, 3]])),
            "send_have" => self.execute_send_have(action),
            "send_bitfield" => self.execute_send_bitfield(action),
            "send_piece" => self.execute_send_piece(action),
            "send_keepalive" => Ok(ActionResult::Output(vec![vec![0, 0, 0, 0]])),
            _ => Err(anyhow::anyhow!("Unknown Peer action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "Torrent-Peer"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            PEER_HANDSHAKE_EVENT.clone(),
            PEER_CHOKE_MESSAGE_EVENT.clone(),
            PEER_REQUEST_MESSAGE_EVENT.clone(),
            PEER_BITFIELD_MESSAGE_EVENT.clone(),
        ]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>BitTorrent-Peer"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["torrent-peer", "peer", "seeder"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState, PrivilegeRequirement};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .privilege_requirement(PrivilegeRequirement::None)
            .implementation("TCP peer wire protocol with binary encoding")
            .llm_control("Piece transfer, choke/unchoke, bitfield")
            .e2e_testing("Real BitTorrent clients")
            .notes("Binary protocol, peer-to-peer data transfer")
            .build()
    }

    fn description(&self) -> &'static str {
        "BitTorrent Peer Wire Protocol for peer-to-peer file sharing"
    }

    fn example_prompt(&self) -> &'static str {
        "start a bittorrent peer on port 51413"
    }

    fn group_name(&self) -> &'static str {
        "P2P"
    }
}

impl TorrentPeerProtocol {
    fn execute_send_handshake(&self, action: serde_json::Value) -> Result<ActionResult> {
        let info_hash = hex::decode(action.get("info_hash").and_then(|v| v.as_str()).context("Missing info_hash")?)?;
        let peer_id = action.get("peer_id").and_then(|v| v.as_str()).unwrap_or("-NT0001-xxxxxxxxxxxx");

        if info_hash.len() != 20 {
            return Err(anyhow::anyhow!("info_hash must be 20 bytes"));
        }
        if peer_id.len() != 20 {
            return Err(anyhow::anyhow!("peer_id must be 20 characters"));
        }

        let mut handshake = Vec::new();
        handshake.push(19u8);
        handshake.extend_from_slice(b"BitTorrent protocol");
        handshake.extend_from_slice(&[0u8; 8]);
        handshake.extend_from_slice(&info_hash);
        handshake.extend_from_slice(peer_id.as_bytes());

        Ok(ActionResult::Output(vec![handshake]))
    }

    fn execute_send_have(&self, action: serde_json::Value) -> Result<ActionResult> {
        let piece_index = action.get("piece_index").and_then(|v| v.as_u64()).context("Missing piece_index")? as u32;

        let mut message = Vec::new();
        message.extend_from_slice(&5u32.to_be_bytes());
        message.push(4);
        message.extend_from_slice(&piece_index.to_be_bytes());

        Ok(ActionResult::Output(vec![message]))
    }

    fn execute_send_bitfield(&self, action: serde_json::Value) -> Result<ActionResult> {
        let bitfield_hex = action.get("bitfield").and_then(|v| v.as_str()).context("Missing bitfield")?;
        let bitfield = hex::decode(bitfield_hex)?;

        let length = (1 + bitfield.len()) as u32;
        let mut message = Vec::new();
        message.extend_from_slice(&length.to_be_bytes());
        message.push(5);
        message.extend_from_slice(&bitfield);

        Ok(ActionResult::Output(vec![message]))
    }

    fn execute_send_piece(&self, action: serde_json::Value) -> Result<ActionResult> {
        let index = action.get("index").and_then(|v| v.as_u64()).context("Missing index")? as u32;
        let begin = action.get("begin").and_then(|v| v.as_u64()).context("Missing begin")? as u32;
        let block_hex = action.get("block_hex").and_then(|v| v.as_str()).context("Missing block_hex")?;
        let block = hex::decode(block_hex)?;

        let length = (9 + block.len()) as u32;
        let mut message = Vec::new();
        message.extend_from_slice(&length.to_be_bytes());
        message.push(7);
        message.extend_from_slice(&index.to_be_bytes());
        message.extend_from_slice(&begin.to_be_bytes());
        message.extend_from_slice(&block);

        Ok(ActionResult::Output(vec![message]))
    }
}

pub static PEER_HANDSHAKE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("peer_handshake", "BitTorrent peer handshake received")
});

pub static PEER_CHOKE_MESSAGE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("peer_choke_message", "Peer choke message")
});

pub static PEER_REQUEST_MESSAGE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("peer_request_message", "Peer piece request")
});

pub static PEER_BITFIELD_MESSAGE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("peer_bitfield_message", "Peer bitfield message")
});

pub static SEND_HANDSHAKE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "send_handshake".to_string(),
        description: "Send BitTorrent handshake".to_string(),
        parameters: vec![
            Parameter {
                name: "info_hash".to_string(),
                type_hint: "string".to_string(),
                description: "Torrent info hash (hex, 20 bytes)".to_string(),
                required: true,
            },
            Parameter {
                name: "peer_id".to_string(),
                type_hint: "string".to_string(),
                description: "Peer ID (20 characters)".to_string(),
                required: false,
            },
        ],
        example: json!({"type": "send_handshake", "info_hash": "0123456789abcdef0123456789abcdef01234567", "peer_id": "-NT0001-xxxxxxxxxxxx"}),
    }
});

pub static SEND_CHOKE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "send_choke".to_string(),
        description: "Send choke message".to_string(),
        parameters: vec![],
        example: json!({"type": "send_choke"}),
    }
});

pub static SEND_UNCHOKE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "send_unchoke".to_string(),
        description: "Send unchoke message".to_string(),
        parameters: vec![],
        example: json!({"type": "send_unchoke"}),
    }
});

pub static SEND_BITFIELD_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "send_bitfield".to_string(),
        description: "Send bitfield message".to_string(),
        parameters: vec![
            Parameter {
                name: "bitfield".to_string(),
                type_hint: "string".to_string(),
                description: "Bitfield (hex)".to_string(),
                required: true,
            },
        ],
        example: json!({"type": "send_bitfield", "bitfield": "ff"}),
    }
});

pub static SEND_HAVE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "send_have".to_string(),
        description: "Send have message".to_string(),
        parameters: vec![
            Parameter {
                name: "piece_index".to_string(),
                type_hint: "number".to_string(),
                description: "Piece index".to_string(),
                required: true,
            },
        ],
        example: json!({"type": "send_have", "piece_index": 0}),
    }
});

pub static SEND_PIECE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "send_piece".to_string(),
        description: "Send piece data".to_string(),
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
                description: "Byte offset".to_string(),
                required: true,
            },
            Parameter {
                name: "block_hex".to_string(),
                type_hint: "string".to_string(),
                description: "Block data (hex)".to_string(),
                required: true,
            },
        ],
        example: json!({"type": "send_piece", "index": 0, "begin": 0, "block_hex": "00112233"}),
    }
});
