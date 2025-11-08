//! BitTorrent Tracker protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

pub struct TorrentTrackerProtocol;

impl TorrentTrackerProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for TorrentTrackerProtocol {
        fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
            Vec::new()
        }
        fn get_sync_actions(&self) -> Vec<ActionDefinition> {
            vec![
                SEND_ANNOUNCE_RESPONSE_ACTION.clone(),
                SEND_SCRAPE_RESPONSE_ACTION.clone(),
                SEND_ERROR_RESPONSE_ACTION.clone(),
            ]
        }
        fn protocol_name(&self) -> &'static str {
            "Torrent-Tracker"
        }
        fn get_event_types(&self) -> Vec<EventType> {
            vec![
                TRACKER_ANNOUNCE_REQUEST_EVENT.clone(),
                TRACKER_SCRAPE_REQUEST_EVENT.clone(),
            ]
        }
        fn stack_name(&self) -> &'static str {
            "ETH>IP>TCP>HTTP>BitTorrent-Tracker"
        }
        fn keywords(&self) -> Vec<&'static str> {
            vec!["torrent-tracker", "tracker", "bittorrent-tracker"]
        }
        fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
            use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState, PrivilegeRequirement};
    
            ProtocolMetadataV2::builder()
                .state(DevelopmentState::Experimental)
                .privilege_requirement(PrivilegeRequirement::None)
                .implementation("HTTP server with bencode response encoding (serde_bencode)")
                .llm_control("Peer list generation, announce/scrape responses")
                .e2e_testing("Real BitTorrent clients (transmission, aria2)")
                .notes("Bencode<->JSON conversion, compact peer format")
                .build()
        }
        fn description(&self) -> &'static str {
            "BitTorrent Tracker server for coordinating peers (announce/scrape)"
        }
        fn example_prompt(&self) -> &'static str {
            "start a bittorrent tracker on port 6969"
        }
        fn group_name(&self) -> &'static str {
            "P2P"
        }
}

// Implement Server trait (server-specific functionality)
impl Server for TorrentTrackerProtocol {
        fn spawn(
            &self,
            ctx: crate::protocol::SpawnContext,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
        > {
            Box::pin(async move {
                use crate::server::torrent_tracker::TorrentTrackerServer;
                TorrentTrackerServer::spawn_with_llm_actions(
                    ctx.listen_addr,
                    ctx.llm_client,
                    ctx.state,
                    ctx.status_tx,
                    ctx.server_id,
                ).await
            })
        }
        fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
            let action_type = action
                .get("type")
                .and_then(|v| v.as_str())
                .context("Missing 'type' field in action")?;
    
            match action_type {
                "send_announce_response" => self.execute_send_announce_response(action),
                "send_scrape_response" => self.execute_send_scrape_response(action),
                "send_error_response" => self.execute_send_error_response(action),
                _ => Err(anyhow::anyhow!("Unknown BitTorrent Tracker action: {}", action_type)),
            }
        }
}


impl TorrentTrackerProtocol {
    fn execute_send_announce_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let interval = action.get("interval").and_then(|v| v.as_u64()).unwrap_or(1800) as i64;
        let complete = action.get("complete").and_then(|v| v.as_u64()).unwrap_or(0) as i64;
        let incomplete = action.get("incomplete").and_then(|v| v.as_u64()).unwrap_or(0) as i64;

        let peers = action.get("peers").and_then(|v| v.as_array()).map(|arr| {
            arr.iter().filter_map(|peer| {
                let peer_id = peer.get("peer_id").and_then(|v| v.as_str())?.as_bytes().to_vec();
                let ip = peer.get("ip").and_then(|v| v.as_str())?.to_string();
                let port = peer.get("port").and_then(|v| v.as_u64())? as i64;

                let mut dict = std::collections::HashMap::new();
                dict.insert(b"peer id".to_vec(), serde_bencode::value::Value::Bytes(peer_id));
                dict.insert(b"ip".to_vec(), serde_bencode::value::Value::Bytes(ip.into_bytes()));
                dict.insert(b"port".to_vec(), serde_bencode::value::Value::Int(port));
                Some(serde_bencode::value::Value::Dict(dict))
            }).collect::<Vec<_>>()
        }).unwrap_or_default();

        let mut response_dict = std::collections::HashMap::new();
        response_dict.insert(b"interval".to_vec(), serde_bencode::value::Value::Int(interval));
        response_dict.insert(b"complete".to_vec(), serde_bencode::value::Value::Int(complete));
        response_dict.insert(b"incomplete".to_vec(), serde_bencode::value::Value::Int(incomplete));
        response_dict.insert(b"peers".to_vec(), serde_bencode::value::Value::List(peers));

        let bencode_data = serde_bencode::to_bytes(&serde_bencode::value::Value::Dict(response_dict))?;
        let http_response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n", bencode_data.len());
        let mut full_response = http_response.into_bytes();
        full_response.extend_from_slice(&bencode_data);

        Ok(ActionResult::Output(full_response))
    }

    fn execute_send_scrape_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let files = action.get("files").and_then(|v| v.as_object()).map(|obj| {
            obj.iter().filter_map(|(info_hash_hex, stats)| {
                let info_hash = hex::decode(info_hash_hex).ok()?;
                let complete = stats.get("complete").and_then(|v| v.as_u64()).unwrap_or(0) as i64;
                let downloaded = stats.get("downloaded").and_then(|v| v.as_u64()).unwrap_or(0) as i64;
                let incomplete = stats.get("incomplete").and_then(|v| v.as_u64()).unwrap_or(0) as i64;

                let mut stats_dict = std::collections::HashMap::new();
                stats_dict.insert(b"complete".to_vec(), serde_bencode::value::Value::Int(complete));
                stats_dict.insert(b"downloaded".to_vec(), serde_bencode::value::Value::Int(downloaded));
                stats_dict.insert(b"incomplete".to_vec(), serde_bencode::value::Value::Int(incomplete));
                Some((info_hash, serde_bencode::value::Value::Dict(stats_dict)))
            }).collect::<std::collections::HashMap<_, _>>()
        }).unwrap_or_default();

        let mut response_dict = std::collections::HashMap::new();
        response_dict.insert(b"files".to_vec(), serde_bencode::value::Value::Dict(files));

        let bencode_data = serde_bencode::to_bytes(&serde_bencode::value::Value::Dict(response_dict))?;
        let http_response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n", bencode_data.len());
        let mut full_response = http_response.into_bytes();
        full_response.extend_from_slice(&bencode_data);

        Ok(ActionResult::Output(full_response))
    }

    fn execute_send_error_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let error_message = action.get("error").and_then(|v| v.as_str()).unwrap_or("Unknown error");

        let mut response_dict = std::collections::HashMap::new();
        response_dict.insert(b"failure reason".to_vec(), serde_bencode::value::Value::Bytes(error_message.as_bytes().to_vec()));

        let bencode_data = serde_bencode::to_bytes(&serde_bencode::value::Value::Dict(response_dict))?;
        let http_response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n", bencode_data.len());
        let mut full_response = http_response.into_bytes();
        full_response.extend_from_slice(&bencode_data);

        Ok(ActionResult::Output(full_response))
    }
}

pub static TRACKER_ANNOUNCE_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("tracker_announce_request", "BitTorrent announce request")
});

pub static TRACKER_SCRAPE_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("tracker_scrape_request", "BitTorrent scrape request")
});

pub static SEND_ANNOUNCE_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "send_announce_response".to_string(),
        description: "Send announce response with peer list".to_string(),
        parameters: vec![
            Parameter {
                name: "interval".to_string(),
                type_hint: "number".to_string(),
                description: "Announce interval in seconds (default: 1800)".to_string(),
                required: false,
            },
            Parameter {
                name: "peers".to_string(),
                type_hint: "array".to_string(),
                description: "Array of peer objects with peer_id, ip, port".to_string(),
                required: false,
            },
        ],
        example: json!({"type": "send_announce_response", "interval": 1800, "complete": 10, "incomplete": 5, "peers": [{"peer_id": "-TR0001-xxxxxxxxxxxx", "ip": "192.168.1.100", "port": 51413}]}),
    }
});

pub static SEND_SCRAPE_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "send_scrape_response".to_string(),
        description: "Send scrape response with torrent statistics".to_string(),
        parameters: vec![
            Parameter {
                name: "files".to_string(),
                type_hint: "object".to_string(),
                description: "Dictionary mapping info_hash to stats".to_string(),
                required: false,
            },
        ],
        example: json!({"type": "send_scrape_response", "files": {"aabbccdd": {"complete": 10, "downloaded": 100, "incomplete": 5}}}),
    }
});

pub static SEND_ERROR_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "send_error_response".to_string(),
        description: "Send error response".to_string(),
        parameters: vec![
            Parameter {
                name: "error".to_string(),
                type_hint: "string".to_string(),
                description: "Error message".to_string(),
                required: true,
            },
        ],
        example: json!({"type": "send_error_response", "error": "Torrent not found"}),
    }
});
