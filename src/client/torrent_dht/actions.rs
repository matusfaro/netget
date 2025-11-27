//! BitTorrent DHT client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    protocol_trait::Protocol,
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// DHT response event
pub static DHT_RESPONSE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("dht_response", "Received response from DHT node", json!({"type": "placeholder", "event_id": "dht_response"})).with_parameters(vec![
        Parameter {
            name: "message_type".to_string(),
            type_hint: "string".to_string(),
            description: "Message type: q (query), r (response), e (error)".to_string(),
            required: true,
        },
        Parameter {
            name: "query_type".to_string(),
            type_hint: "string".to_string(),
            description: "Query type: ping, find_node, get_peers, announce_peer".to_string(),
            required: false,
        },
        Parameter {
            name: "response".to_string(),
            type_hint: "string".to_string(),
            description: "Response data from node".to_string(),
            required: false,
        },
        Parameter {
            name: "error".to_string(),
            type_hint: "string".to_string(),
            description: "Error information if any".to_string(),
            required: false,
        },
        Parameter {
            name: "peer".to_string(),
            type_hint: "string".to_string(),
            description: "Address of responding peer".to_string(),
            required: false,
        },
    ])
});

/// BitTorrent DHT client protocol action handler
pub struct TorrentDhtClientProtocol;

impl TorrentDhtClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for TorrentDhtClientProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "dht_ping".to_string(),
                description: "Ping a DHT node".to_string(),
                parameters: vec![
                    Parameter {
                        name: "node_id".to_string(),
                        type_hint: "string".to_string(),
                        description: "Our node ID (20 bytes hex)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "transaction_id".to_string(),
                        type_hint: "string".to_string(),
                        description: "Transaction ID".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "dht_query",
                    "query_type": "ping",
                    "node_id": "abcdefghij0123456789",
                    "transaction_id": "aa"
                }),
            },
            ActionDefinition {
                name: "dht_find_node".to_string(),
                description: "Find nodes close to a target ID".to_string(),
                parameters: vec![
                    Parameter {
                        name: "node_id".to_string(),
                        type_hint: "string".to_string(),
                        description: "Our node ID (20 bytes hex)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "target".to_string(),
                        type_hint: "string".to_string(),
                        description: "Target node ID to find (20 bytes hex)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "transaction_id".to_string(),
                        type_hint: "string".to_string(),
                        description: "Transaction ID".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "dht_query",
                    "query_type": "find_node",
                    "node_id": "abcdefghij0123456789",
                    "target": "mnopqrstuv0123456789",
                    "transaction_id": "aa"
                }),
            },
            ActionDefinition {
                name: "dht_get_peers".to_string(),
                description: "Get peers for an info_hash".to_string(),
                parameters: vec![
                    Parameter {
                        name: "node_id".to_string(),
                        type_hint: "string".to_string(),
                        description: "Our node ID (20 bytes hex)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "info_hash".to_string(),
                        type_hint: "string".to_string(),
                        description: "Info hash to query (20 bytes hex)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "transaction_id".to_string(),
                        type_hint: "string".to_string(),
                        description: "Transaction ID".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "dht_query",
                    "query_type": "get_peers",
                    "node_id": "abcdefghij0123456789",
                    "info_hash": "0123456789abcdefghij",
                    "transaction_id": "aa"
                }),
            },
            ActionDefinition {
                name: "dht_announce_peer".to_string(),
                description: "Announce that we have a torrent".to_string(),
                parameters: vec![
                    Parameter {
                        name: "node_id".to_string(),
                        type_hint: "string".to_string(),
                        description: "Our node ID (20 bytes hex)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "info_hash".to_string(),
                        type_hint: "string".to_string(),
                        description: "Info hash to announce (20 bytes hex)".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "transaction_id".to_string(),
                        type_hint: "string".to_string(),
                        description: "Transaction ID".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "dht_query",
                    "query_type": "announce_peer",
                    "node_id": "abcdefghij0123456789",
                    "info_hash": "0123456789abcdefghij",
                    "transaction_id": "aa"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from DHT".to_string(),
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
    fn protocol_name(&self) -> &'static str {
        "BitTorrent DHT"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![EventType::new("dht_response", "Received response from DHT node", json!({"type": "placeholder", "event_id": "dht_response"}))]
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>BitTorrent-DHT"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["bittorrent", "dht", "kademlia", "distributed hash table"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("UDP-based Kademlia DHT with bencode messages")
            .llm_control(
                "Full control over DHT queries (ping, find_node, get_peers, announce_peer)",
            )
            .e2e_testing("Mock DHT node")
            .build()
    }
    fn description(&self) -> &'static str {
        "BitTorrent DHT client for distributed peer discovery"
    }
    fn example_prompt(&self) -> &'static str {
        "Connect to DHT node at router.bittorrent.com:6881 and find peers for info_hash xyz"
    }
    fn group_name(&self) -> &'static str {
        "P2P"
    }
}

// Implement Client trait (client-specific functionality)
impl Client for TorrentDhtClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::torrent_dht::TorrentDhtClient;
            TorrentDhtClient::connect_with_llm_actions(
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
            "dht_query" => Ok(ClientActionResult::Custom {
                name: "dht_query".to_string(),
                data: action,
            }),
            "disconnect" => Ok(ClientActionResult::Disconnect),
            _ => Err(anyhow::anyhow!(
                "Unknown DHT client action: {}",
                action_type
            )),
        }
    }
}
