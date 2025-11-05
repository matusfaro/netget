//! BitTorrent DHT protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

pub struct TorrentDhtProtocol;

impl TorrentDhtProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Server for TorrentDhtProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::torrent_dht::TorrentDhtServer;
            TorrentDhtServer::spawn_with_llm_actions(
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
            SEND_PING_RESPONSE_ACTION.clone(),
            SEND_FIND_NODE_RESPONSE_ACTION.clone(),
            SEND_GET_PEERS_RESPONSE_ACTION.clone(),
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action.get("type").and_then(|v| v.as_str()).context("Missing 'type' field")?;

        match action_type {
            "send_ping_response" => self.execute_send_ping_response(action),
            "send_find_node_response" => self.execute_send_find_node_response(action),
            "send_get_peers_response" => self.execute_send_get_peers_response(action),
            _ => Err(anyhow::anyhow!("Unknown DHT action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "Torrent-DHT"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            DHT_PING_QUERY_EVENT.clone(),
            DHT_FIND_NODE_QUERY_EVENT.clone(),
            DHT_GET_PEERS_QUERY_EVENT.clone(),
        ]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>BitTorrent-DHT"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["torrent-dht", "dht", "kademlia"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState, PrivilegeRequirement};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .privilege_requirement(PrivilegeRequirement::None)
            .implementation("UDP KRPC protocol with bencode encoding")
            .llm_control("DHT query responses (ping, find_node, get_peers)")
            .e2e_testing("Real BitTorrent clients with DHT")
            .notes("Kademlia DHT, BEP 5")
            .build()
    }

    fn description(&self) -> &'static str {
        "BitTorrent DHT server for distributed peer discovery"
    }

    fn example_prompt(&self) -> &'static str {
        "start a bittorrent dht node on port 6881"
    }

    fn group_name(&self) -> &'static str {
        "P2P"
    }
}

impl TorrentDhtProtocol {
    fn execute_send_ping_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let transaction_id = hex::decode(action.get("transaction_id").and_then(|v| v.as_str()).context("Missing transaction_id")?)?;
        let node_id = hex::decode(action.get("node_id").and_then(|v| v.as_str()).unwrap_or("0000000000000000000000000000000000000000"))?;

        let mut response = std::collections::HashMap::new();
        response.insert(b"t".to_vec(), serde_bencode::value::Value::Bytes(transaction_id));
        response.insert(b"y".to_vec(), serde_bencode::value::Value::Bytes(b"r".to_vec()));

        let mut r_dict = std::collections::HashMap::new();
        r_dict.insert(b"id".to_vec(), serde_bencode::value::Value::Bytes(node_id));
        response.insert(b"r".to_vec(), serde_bencode::value::Value::Dict(r_dict));

        let bencode_data = serde_bencode::to_bytes(&serde_bencode::value::Value::Dict(response))?;
        Ok(ActionResult::Output(bencode_data))
    }

    fn execute_send_find_node_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let transaction_id = hex::decode(action.get("transaction_id").and_then(|v| v.as_str()).context("Missing transaction_id")?)?;
        let node_id = hex::decode(action.get("node_id").and_then(|v| v.as_str()).unwrap_or("0000000000000000000000000000000000000000"))?;

        let nodes = action.get("nodes").and_then(|v| v.as_array()).map(|arr| {
            arr.iter().filter_map(|node| {
                let id = hex::decode(node.get("id")?.as_str()?).ok()?;
                let ip = node.get("ip")?.as_str()?;
                let port = node.get("port")?.as_u64()? as u16;
                let ip_parts: Vec<u8> = ip.split('.').filter_map(|s| s.parse().ok()).collect();
                if ip_parts.len() != 4 || id.len() != 20 { return None; }

                let mut compact = id;
                compact.extend_from_slice(&ip_parts);
                compact.extend_from_slice(&port.to_be_bytes());
                Some(compact)
            }).collect::<Vec<_>>()
        }).unwrap_or_default();

        let nodes_bytes: Vec<u8> = nodes.into_iter().flatten().collect();

        let mut response = std::collections::HashMap::new();
        response.insert(b"t".to_vec(), serde_bencode::value::Value::Bytes(transaction_id));
        response.insert(b"y".to_vec(), serde_bencode::value::Value::Bytes(b"r".to_vec()));

        let mut r_dict = std::collections::HashMap::new();
        r_dict.insert(b"id".to_vec(), serde_bencode::value::Value::Bytes(node_id));
        r_dict.insert(b"nodes".to_vec(), serde_bencode::value::Value::Bytes(nodes_bytes));
        response.insert(b"r".to_vec(), serde_bencode::value::Value::Dict(r_dict));

        let bencode_data = serde_bencode::to_bytes(&serde_bencode::value::Value::Dict(response))?;
        Ok(ActionResult::Output(bencode_data))
    }

    fn execute_send_get_peers_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let transaction_id = hex::decode(action.get("transaction_id").and_then(|v| v.as_str()).context("Missing transaction_id")?)?;
        let node_id = hex::decode(action.get("node_id").and_then(|v| v.as_str()).unwrap_or("0000000000000000000000000000000000000000"))?;
        let token = action.get("token").and_then(|v| v.as_str()).unwrap_or("token").as_bytes().to_vec();

        let mut response = std::collections::HashMap::new();
        response.insert(b"t".to_vec(), serde_bencode::value::Value::Bytes(transaction_id));
        response.insert(b"y".to_vec(), serde_bencode::value::Value::Bytes(b"r".to_vec()));

        let mut r_dict = std::collections::HashMap::new();
        r_dict.insert(b"id".to_vec(), serde_bencode::value::Value::Bytes(node_id));
        r_dict.insert(b"token".to_vec(), serde_bencode::value::Value::Bytes(token));

        if let Some(peers_arr) = action.get("peers").and_then(|v| v.as_array()) {
            let peers_bytes: Vec<u8> = peers_arr.iter().filter_map(|peer| {
                let ip = peer.get("ip")?.as_str()?;
                let port = peer.get("port")?.as_u64()? as u16;
                let ip_parts: Vec<u8> = ip.split('.').filter_map(|s| s.parse().ok()).collect();
                if ip_parts.len() != 4 { return None; }
                let mut compact = ip_parts;
                compact.extend_from_slice(&port.to_be_bytes());
                Some(compact)
            }).flatten().collect();
            r_dict.insert(b"values".to_vec(), serde_bencode::value::Value::Bytes(peers_bytes));
        }

        response.insert(b"r".to_vec(), serde_bencode::value::Value::Dict(r_dict));

        let bencode_data = serde_bencode::to_bytes(&serde_bencode::value::Value::Dict(response))?;
        Ok(ActionResult::Output(bencode_data))
    }
}

pub static DHT_PING_QUERY_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("dht_ping_query", "DHT ping query")
});

pub static DHT_FIND_NODE_QUERY_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("dht_find_node_query", "DHT find_node query")
});

pub static DHT_GET_PEERS_QUERY_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("dht_get_peers_query", "DHT get_peers query")
});

pub static SEND_PING_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "send_ping_response".to_string(),
        description: "Send DHT ping response".to_string(),
        parameters: vec![
            Parameter {
                name: "transaction_id".to_string(),
                type_hint: "string".to_string(),
                description: "Transaction ID (hex)".to_string(),
                required: true,
            },
        ],
        example: json!({"type": "send_ping_response", "transaction_id": "aa", "node_id": "0123456789abcdef0123456789abcdef01234567"}),
    }
});

pub static SEND_FIND_NODE_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "send_find_node_response".to_string(),
        description: "Send DHT find_node response".to_string(),
        parameters: vec![
            Parameter {
                name: "transaction_id".to_string(),
                type_hint: "string".to_string(),
                description: "Transaction ID (hex)".to_string(),
                required: true,
            },
            Parameter {
                name: "nodes".to_string(),
                type_hint: "array".to_string(),
                description: "Array of node objects with id, ip, port".to_string(),
                required: false,
            },
        ],
        example: json!({"type": "send_find_node_response", "transaction_id": "aa", "node_id": "0123456789abcdef0123456789abcdef01234567", "nodes": [{"id": "0123456789abcdef0123456789abcdef01234567", "ip": "192.168.1.100", "port": 6881}]}),
    }
});

pub static SEND_GET_PEERS_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "send_get_peers_response".to_string(),
        description: "Send DHT get_peers response".to_string(),
        parameters: vec![
            Parameter {
                name: "transaction_id".to_string(),
                type_hint: "string".to_string(),
                description: "Transaction ID (hex)".to_string(),
                required: true,
            },
            Parameter {
                name: "peers".to_string(),
                type_hint: "array".to_string(),
                description: "Array of peer objects with ip, port".to_string(),
                required: false,
            },
        ],
        example: json!({"type": "send_get_peers_response", "transaction_id": "aa", "node_id": "0123456789abcdef0123456789abcdef01234567", "token": "aoeusnth", "peers": [{"ip": "192.168.1.100", "port": 51413}]}),
    }
});
