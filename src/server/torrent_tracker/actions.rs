//! BitTorrent Tracker protocol actions implementation

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
        use crate::protocol::metadata::{
            DevelopmentState, PrivilegeRequirement, ProtocolMetadataV2,
        };

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

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        use serde_json::json;

        StartupExamples::new(
            // LLM mode: LLM handles BitTorrent tracker
            json!({
                "type": "open_server",
                "port": 6969,
                "base_stack": "torrent-tracker",
                "instruction": "Act as BitTorrent tracker. Track peers and respond to announce/scrape requests"
            }),
            // Script mode: Code-based tracker handling
            json!({
                "type": "open_server",
                "port": 6969,
                "base_stack": "torrent-tracker",
                "event_handlers": [{
                    "event_pattern": "tracker_announce_request",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<tracker_handler>"
                    }
                }]
            }),
            // Static mode: Fixed tracker responses
            json!({
                "type": "open_server",
                "port": 6969,
                "base_stack": "torrent-tracker",
                "event_handlers": [{
                    "event_pattern": "tracker_announce_request",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "send_announce_response",
                            "interval": 1800,
                            "complete": 10,
                            "incomplete": 5,
                            "compact": "{{event.compact}}",
                            "peers": [{"ip": "127.0.0.1", "port": 51413}]
                        }]
                    }
                }]
            }),
        )
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
            "send_announce_response" => self.execute_send_announce_response(action),
            "send_scrape_response" => self.execute_send_scrape_response(action),
            "send_error_response" => self.execute_send_error_response(action),
            _ => Err(anyhow::anyhow!(
                "Unknown BitTorrent Tracker action: {}",
                action_type
            )),
        }
    }
}

/// Interpret a JSON value as a BitTorrent "compact" flag.
///
/// Clients send `compact=1` in the query string, so the value reaching an action via
/// `{{event.compact}}` is the number 1, not `true`. Accept both spellings (and the
/// string forms a model is liable to produce) rather than silently falling back to the
/// dictionary format that most clients reject.
fn is_compact(value: Option<&serde_json::Value>) -> bool {
    match value {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::Number(n)) => n.as_u64().is_some_and(|n| n != 0),
        Some(serde_json::Value::String(s)) => {
            matches!(s.as_str(), "1" | "true" | "yes" | "on")
        }
        _ => false,
    }
}

impl TorrentTrackerProtocol {
    fn execute_send_announce_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let interval = action
            .get("interval")
            .and_then(|v| v.as_u64())
            .unwrap_or(1800) as i64;
        let complete = action.get("complete").and_then(|v| v.as_u64()).unwrap_or(0) as i64;
        let incomplete = action
            .get("incomplete")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as i64;
        let compact = is_compact(action.get("compact"));

        let peer_entries = action
            .get("peers")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        // BEP 23: `compact=1` replaces the list of dictionaries with a byte string of
        // 6-byte entries (4-byte IPv4 + 2-byte big-endian port). Nearly every real client
        // (transmission, libtorrent, aria2) asks for compact and several refuse the
        // dictionary form outright, so honouring the flag is what makes this tracker
        // usable by anything other than a hand-written test client.
        let peers_value = if compact {
            let mut bytes = Vec::with_capacity(peer_entries.len() * 6);
            for peer in &peer_entries {
                let Some(ip) = peer.get("ip").and_then(|v| v.as_str()) else {
                    continue;
                };
                let Some(port) = peer.get("port").and_then(|v| v.as_u64()) else {
                    continue;
                };
                let Ok(std::net::IpAddr::V4(addr)) = ip.parse::<std::net::IpAddr>() else {
                    // IPv6 peers need BEP 7's separate `peers6` key; skip rather than
                    // emit a malformed 6-byte entry.
                    continue;
                };
                bytes.extend_from_slice(&addr.octets());
                bytes.extend_from_slice(&(port as u16).to_be_bytes());
            }
            serde_bencode::value::Value::Bytes(bytes)
        } else {
            let peers = peer_entries
                .iter()
                .filter_map(|peer| {
                    let ip = peer.get("ip").and_then(|v| v.as_str())?.to_string();
                    let port = peer.get("port").and_then(|v| v.as_u64())? as i64;

                    let mut dict = std::collections::HashMap::new();
                    // `peer id` is optional in the dictionary model (BEP 3) and a model
                    // that omits it should still get a well-formed peer entry.
                    if let Some(peer_id) = peer.get("peer_id").and_then(|v| v.as_str()) {
                        dict.insert(
                            b"peer id".to_vec(),
                            serde_bencode::value::Value::Bytes(peer_id.as_bytes().to_vec()),
                        );
                    }
                    dict.insert(
                        b"ip".to_vec(),
                        serde_bencode::value::Value::Bytes(ip.into_bytes()),
                    );
                    dict.insert(b"port".to_vec(), serde_bencode::value::Value::Int(port));
                    Some(serde_bencode::value::Value::Dict(dict))
                })
                .collect::<Vec<_>>();
            serde_bencode::value::Value::List(peers)
        };

        let mut response_dict = std::collections::HashMap::new();
        response_dict.insert(
            b"interval".to_vec(),
            serde_bencode::value::Value::Int(interval),
        );
        response_dict.insert(
            b"complete".to_vec(),
            serde_bencode::value::Value::Int(complete),
        );
        response_dict.insert(
            b"incomplete".to_vec(),
            serde_bencode::value::Value::Int(incomplete),
        );
        response_dict.insert(b"peers".to_vec(), peers_value);

        let bencode_data =
            serde_bencode::to_bytes(&serde_bencode::value::Value::Dict(response_dict))?;
        let http_response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bencode_data.len()
        );
        let mut full_response = http_response.into_bytes();
        full_response.extend_from_slice(&bencode_data);

        Ok(ActionResult::Output(full_response))
    }

    fn execute_send_scrape_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        // `files` is documented and modelled two ways and both appear in the wild:
        //   {"<info_hash hex>": {complete, downloaded, incomplete}, ...}
        //   [{"info_hash": "<hex>", complete, downloaded, incomplete}, ...]
        // The executor used to accept only the first, so the array form documented in
        // CLAUDE.md (and used by the E2E test) silently produced an empty `files` dict.
        let entries: Vec<(String, &serde_json::Value)> = match action.get("files") {
            Some(serde_json::Value::Object(obj)) => {
                obj.iter().map(|(k, v)| (k.clone(), v)).collect()
            }
            Some(serde_json::Value::Array(arr)) => arr
                .iter()
                .filter_map(|entry| {
                    let hash = entry.get("info_hash").and_then(|v| v.as_str())?;
                    Some((hash.to_string(), entry))
                })
                .collect(),
            _ => Vec::new(),
        };

        let mut files = std::collections::HashMap::new();
        for (info_hash_hex, stats) in entries {
            let Ok(info_hash) = hex::decode(&info_hash_hex) else {
                continue;
            };
            let complete = stats.get("complete").and_then(|v| v.as_u64()).unwrap_or(0) as i64;
            let downloaded = stats
                .get("downloaded")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as i64;
            let incomplete = stats
                .get("incomplete")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as i64;

            let mut stats_dict = std::collections::HashMap::new();
            stats_dict.insert(
                b"complete".to_vec(),
                serde_bencode::value::Value::Int(complete),
            );
            stats_dict.insert(
                b"downloaded".to_vec(),
                serde_bencode::value::Value::Int(downloaded),
            );
            stats_dict.insert(
                b"incomplete".to_vec(),
                serde_bencode::value::Value::Int(incomplete),
            );
            files.insert(info_hash, serde_bencode::value::Value::Dict(stats_dict));
        }

        let mut response_dict = std::collections::HashMap::new();
        response_dict.insert(b"files".to_vec(), serde_bencode::value::Value::Dict(files));

        let bencode_data =
            serde_bencode::to_bytes(&serde_bencode::value::Value::Dict(response_dict))?;
        let http_response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bencode_data.len()
        );
        let mut full_response = http_response.into_bytes();
        full_response.extend_from_slice(&bencode_data);

        Ok(ActionResult::Output(full_response))
    }

    fn execute_send_error_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        // The action definition, CLAUDE.md and the E2E test all said `failure_reason`
        // (matching the bencode key BEP 3 defines) while the executor read `error`, so
        // every documented use produced the literal string "Unknown error". Accept both.
        let error_message = action
            .get("failure_reason")
            .or_else(|| action.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");

        let mut response_dict = std::collections::HashMap::new();
        response_dict.insert(
            b"failure reason".to_vec(),
            serde_bencode::value::Value::Bytes(error_message.as_bytes().to_vec()),
        );

        let bencode_data =
            serde_bencode::to_bytes(&serde_bencode::value::Value::Dict(response_dict))?;
        let http_response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bencode_data.len()
        );
        let mut full_response = http_response.into_bytes();
        full_response.extend_from_slice(&bencode_data);

        Ok(ActionResult::Output(full_response))
    }
}

/// Fields every tracker request event carries.
///
/// `parse_http_request` guarantees `request_type`, `path` and `compact` are always
/// present so that a static handler can reference `{{event.compact}}` without the
/// interpolator failing on a client that omitted the query parameter.
fn common_request_parameters() -> Vec<Parameter> {
    vec![
        Parameter {
            name: "request_type".to_string(),
            type_hint: "string".to_string(),
            description: "\"announce\", \"scrape\" or \"unknown\"".to_string(),
            required: true,
        },
        Parameter {
            name: "path".to_string(),
            type_hint: "string".to_string(),
            description: "Request path including the query string".to_string(),
            required: true,
        },
        Parameter {
            name: "info_hash".to_string(),
            type_hint: "string".to_string(),
            description: "Torrent info hash, hex-encoded (40 chars). Echo it back as the \
                          key of a scrape `files` entry."
                .to_string(),
            required: false,
        },
        Parameter {
            name: "compact".to_string(),
            type_hint: "number".to_string(),
            description: "1 if the client wants BEP 23 compact peers, 0 otherwise. \
                          Always present; pass it straight through to the response action."
                .to_string(),
            required: true,
        },
    ]
}

pub static TRACKER_ANNOUNCE_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    let mut parameters = common_request_parameters();
    parameters.extend(vec![
        Parameter {
            name: "peer_id".to_string(),
            type_hint: "string".to_string(),
            description: "Announcing peer's ID, hex-encoded (40 chars)".to_string(),
            required: false,
        },
        Parameter {
            name: "port".to_string(),
            type_hint: "number".to_string(),
            description: "Port the announcing peer listens on".to_string(),
            required: false,
        },
        Parameter {
            name: "uploaded".to_string(),
            type_hint: "number".to_string(),
            description: "Bytes uploaded so far".to_string(),
            required: false,
        },
        Parameter {
            name: "downloaded".to_string(),
            type_hint: "number".to_string(),
            description: "Bytes downloaded so far".to_string(),
            required: false,
        },
        Parameter {
            name: "left".to_string(),
            type_hint: "number".to_string(),
            description: "Bytes still needed. 0 means the peer is a seeder.".to_string(),
            required: false,
        },
        Parameter {
            name: "event".to_string(),
            type_hint: "string".to_string(),
            description: "\"started\", \"completed\", \"stopped\" or absent".to_string(),
            required: false,
        },
        Parameter {
            name: "numwant".to_string(),
            type_hint: "number".to_string(),
            description: "How many peers the client would like back".to_string(),
            required: false,
        },
    ]);

    EventType::new(
        "tracker_announce_request",
        "BitTorrent client announced itself and is asking for peers",
        json!({
            "type": "send_announce_response",
            "interval": 1800,
            "complete": 1,
            "incomplete": 0,
            "compact": "{{event.compact}}",
            "peers": [{"ip": "127.0.0.1", "port": 51413}]
        }),
    )
    .with_parameters(parameters)
    .with_actions(vec![
        SEND_ANNOUNCE_RESPONSE_ACTION.clone(),
        SEND_ERROR_RESPONSE_ACTION.clone(),
    ])
    .with_alternative_example(json!({
        "type": "send_error_response",
        "failure_reason": "This tracker does not serve that torrent"
    }))
    .with_log_template(
        LogTemplate::new()
            .with_info("{client_ip} BT announce {event} ({duration_ms}ms)")
            .with_debug("BT tracker announce from {client_ip}: event={event}")
            .with_trace("BT announce: {json_pretty(.)}"),
    )
});

pub static TRACKER_SCRAPE_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "tracker_scrape_request",
        "BitTorrent client asked for torrent statistics",
        json!({
            "type": "send_scrape_response",
            "files": {
                "{{event.info_hash}}": {"complete": 1, "downloaded": 1, "incomplete": 0}
            }
        }),
    )
    .with_parameters(common_request_parameters())
    .with_actions(vec![
        SEND_SCRAPE_RESPONSE_ACTION.clone(),
        SEND_ERROR_RESPONSE_ACTION.clone(),
    ])
    .with_log_template(
        LogTemplate::new()
            .with_info("{client_ip} BT scrape ({duration_ms}ms)")
            .with_debug("BT tracker scrape from {client_ip}")
            .with_trace("BT scrape: {json_pretty(.)}"),
    )
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
                name: "complete".to_string(),
                type_hint: "number".to_string(),
                description: "Number of seeders (default: 0)".to_string(),
                required: false,
            },
            Parameter {
                name: "incomplete".to_string(),
                type_hint: "number".to_string(),
                description: "Number of leechers (default: 0)".to_string(),
                required: false,
            },
            Parameter {
                name: "compact".to_string(),
                type_hint: "boolean".to_string(),
                description: "Encode peers in BEP 23 compact form (4-byte IPv4 + 2-byte \
                              port). Pass the request's own `compact` value through as \
                              \"{{event.compact}}\"; most real clients require it. \
                              Compact form carries no peer_id and IPv6 peers are dropped."
                    .to_string(),
                required: false,
            },
            Parameter {
                name: "peers".to_string(),
                type_hint: "array".to_string(),
                description: "Array of peer objects with ip and port (and optional \
                              peer_id, used only in non-compact form)"
                    .to_string(),
                required: false,
            },
        ],
        example: json!({"type": "send_announce_response", "interval": 1800, "complete": 10, "incomplete": 5, "compact": "{{event.compact}}", "peers": [{"peer_id": "-TR0001-xxxxxxxxxxxx", "ip": "192.168.1.100", "port": 51413}]}),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BT announce: {peers_len} peers, interval={interval}s")
                .with_debug("BT announce response: {peers_len} peers, complete={complete}, incomplete={incomplete}"),
        ),
    }
});

pub static SEND_SCRAPE_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "send_scrape_response".to_string(),
        description: "Send scrape response with torrent statistics".to_string(),
        parameters: vec![Parameter {
            name: "files".to_string(),
            type_hint: "object".to_string(),
            description: "Either an object keyed by hex info_hash -> \
                          {complete, downloaded, incomplete}, or an array of objects each \
                          carrying an `info_hash` field plus those counts. Keys that are \
                          not valid hex are dropped."
                .to_string(),
            required: false,
        }],
        example: json!({"type": "send_scrape_response", "files": {"{{event.info_hash}}": {"complete": 10, "downloaded": 100, "incomplete": 5}}}),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BT scrape: {files_len} torrents")
                .with_debug("BT scrape response: {files_len} torrents"),
        ),
    }
});

pub static SEND_ERROR_RESPONSE_ACTION: LazyLock<ActionDefinition> =
    LazyLock::new(|| ActionDefinition {
        name: "send_error_response".to_string(),
        description: "Refuse the request with a bencode `failure reason` (BEP 3). Clients \
                      display this and stop announcing to this tracker."
            .to_string(),
        parameters: vec![Parameter {
            name: "failure_reason".to_string(),
            type_hint: "string".to_string(),
            description: "Message shown to the client (alias: `error`)".to_string(),
            required: true,
        }],
        example: json!({"type": "send_error_response", "failure_reason": "Torrent not found"}),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> BT error: {failure_reason}")
                .with_debug("BT tracker error: {failure_reason}"),
        ),
    });
