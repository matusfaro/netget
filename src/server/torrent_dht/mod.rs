//! BitTorrent DHT server implementation
//!
//! UDP-based Kademlia DHT for distributed peer discovery (BEP 5)

pub mod actions;

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use crate::{console_debug, console_error, console_info, console_trace, console_warn};
use actions::TorrentDhtProtocol;

/// BitTorrent DHT server
pub struct TorrentDhtServer;

impl TorrentDhtServer {
    /// Spawn BitTorrent DHT server with LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        info!(
            "BitTorrent DHT server (action-based) listening on {}",
            local_addr
        );
        let _ = status_tx.send(format!(
            "[INFO] BitTorrent DHT server listening on {}",
            local_addr
        ));

        let protocol = Arc::new(TorrentDhtProtocol::new());

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 65535];

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);

                        // Add connection to ServerInstance
                        use crate::state::server::{
                            ConnectionState as ServerConnectionState, ConnectionStatus,
                            ProtocolConnectionInfo,
                        };
                        let now = std::time::Instant::now();
                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr: peer_addr,
                            local_addr,
                            bytes_sent: 0,
                            bytes_received: n as u64,
                            packets_sent: 0,
                            packets_received: 1,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::empty(),
                        };
                        app_state
                            .add_connection_to_server(server_id, conn_state)
                            .await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        console_debug!(
                            status_tx,
                            "BitTorrent DHT received {} bytes from {}",
                            n,
                            peer_addr
                        );

                        // TRACE: Log full payload
                        let hex_str = hex::encode(&data);
                        console_trace!(status_tx, "BitTorrent DHT data (hex): {}", hex_str);

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            // Parse bencode KRPC message
                            match Self::parse_krpc_message(&data) {
                                Ok((query_type, params)) => {
                                    debug!("BitTorrent DHT query type: {}", query_type);
                                    let _ = status_clone.send(format!(
                                        "[DEBUG] BitTorrent DHT query type: {}",
                                        query_type
                                    ));

                                    // Create event for LLM
                                    let event_type = match query_type.as_str() {
                                        "ping" => &actions::DHT_PING_QUERY_EVENT,
                                        "find_node" => &actions::DHT_FIND_NODE_QUERY_EVENT,
                                        "get_peers" => &actions::DHT_GET_PEERS_QUERY_EVENT,
                                        _ => &actions::DHT_PING_QUERY_EVENT, // Default to ping
                                    };
                                    let event = Event::new(event_type, serde_json::json!(params));

                                    debug!("BitTorrent DHT calling LLM for {} query", query_type);
                                    let _ = status_clone.send(format!(
                                        "[DEBUG] BitTorrent DHT calling LLM for {} query",
                                        query_type
                                    ));

                                    // Call LLM
                                    match call_llm(
                                        &llm_clone,
                                        &state_clone,
                                        server_id,
                                        None,
                                        &event,
                                        protocol_clone.as_ref(),
                                    )
                                    .await
                                    {
                                        Ok(execution_result) => {
                                            for message in &execution_result.messages {
                                                info!("{}", message);
                                                let _ = status_clone
                                                    .send(format!("[INFO] {}", message));
                                            }

                                            debug!(
                                                "BitTorrent DHT got {} protocol results",
                                                execution_result.protocol_results.len()
                                            );
                                            let _ = status_clone.send(format!(
                                                "[DEBUG] BitTorrent DHT got {} protocol results",
                                                execution_result.protocol_results.len()
                                            ));

                                            for protocol_result in execution_result.protocol_results
                                            {
                                                if let Some(output_data) =
                                                    protocol_result.get_all_output().first()
                                                {
                                                    if let Err(e) = socket_clone
                                                        .send_to(output_data, peer_addr)
                                                        .await
                                                    {
                                                        error!(
                                                            "Failed to send DHT response: {}",
                                                            e
                                                        );
                                                    } else {
                                                        debug!(
                                                            "BitTorrent DHT sent {} bytes to {}",
                                                            output_data.len(),
                                                            peer_addr
                                                        );
                                                        let _ = status_clone.send(format!("[DEBUG] BitTorrent DHT sent {} bytes to {}", output_data.len(), peer_addr));

                                                        let hex_str = hex::encode(output_data);
                                                        trace!(
                                                            "BitTorrent DHT sent (hex): {}",
                                                            hex_str
                                                        );
                                                        let _ = status_clone.send(format!(
                                                            "[TRACE] BitTorrent DHT sent (hex): {}",
                                                            hex_str
                                                        ));
                                                    }
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            error!("BitTorrent DHT LLM call failed: {}", e);
                                            let _ = status_clone
                                                .send(format!("✗ BitTorrent DHT LLM error: {}", e));
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to parse KRPC message: {}", e);
                                    let _ = status_clone.send(format!(
                                        "[ERROR] Failed to parse KRPC message: {}",
                                        e
                                    ));
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("BitTorrent DHT receive error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    fn parse_krpc_message(data: &[u8]) -> Result<(String, serde_json::Value)> {
        use serde_bencode::value::Value;

        // Decode bencode
        let value: Value = serde_bencode::from_bytes(data)?;

        if let Value::Dict(dict) = value {
            // Get message type (q = query, r = response, e = error)
            let msg_type = dict
                .get::<[u8]>(b"y")
                .and_then(|v| {
                    if let Value::Bytes(bytes) = v {
                        String::from_utf8(bytes.clone()).ok()
                    } else {
                        None
                    }
                })
                .ok_or_else(|| anyhow::anyhow!("Missing 'y' field"))?;

            if msg_type == "q" {
                // Query message
                let query_type = dict
                    .get::<[u8]>(b"q")
                    .and_then(|v| {
                        if let Value::Bytes(bytes) = v {
                            String::from_utf8(bytes.clone()).ok()
                        } else {
                            None
                        }
                    })
                    .ok_or_else(|| anyhow::anyhow!("Missing 'q' field"))?;

                // Get transaction ID
                let transaction_id = dict.get::<[u8]>(b"t").and_then(|v| {
                    if let Value::Bytes(bytes) = v {
                        Some(hex::encode(bytes))
                    } else {
                        None
                    }
                });

                // Get arguments
                let mut params = serde_json::Map::new();
                if let Some(transaction_id) = transaction_id {
                    params.insert(
                        "transaction_id".to_string(),
                        serde_json::json!(transaction_id),
                    );
                }

                if let Some(Value::Dict(args)) = dict.get::<[u8]>(b"a") {
                    for (k, v) in args {
                        let key = String::from_utf8_lossy(k).to_string();
                        let value = Self::bencode_to_json(v);
                        params.insert(key, value);
                    }
                }

                Ok((query_type, serde_json::Value::Object(params)))
            } else {
                Err(anyhow::anyhow!("Not a query message"))
            }
        } else {
            Err(anyhow::anyhow!("Invalid KRPC message"))
        }
    }

    fn bencode_to_json(value: &serde_bencode::value::Value) -> serde_json::Value {
        use serde_bencode::value::Value;

        match value {
            Value::Int(i) => serde_json::json!(i),
            Value::Bytes(bytes) => {
                // Try to decode as UTF-8 string, otherwise hex encode
                if let Ok(s) = String::from_utf8(bytes.clone()) {
                    if s.chars()
                        .all(|c| c.is_ascii_graphic() || c.is_ascii_whitespace())
                    {
                        serde_json::json!(s)
                    } else {
                        serde_json::json!(hex::encode(bytes))
                    }
                } else {
                    serde_json::json!(hex::encode(bytes))
                }
            }
            Value::List(list) => {
                let json_list: Vec<_> = list.iter().map(|v| Self::bencode_to_json(v)).collect();
                serde_json::json!(json_list)
            }
            Value::Dict(dict) => {
                let mut json_obj = serde_json::Map::new();
                for (k, v) in dict {
                    let key = String::from_utf8_lossy(k).to_string();
                    json_obj.insert(key, Self::bencode_to_json(v));
                }
                serde_json::Value::Object(json_obj)
            }
        }
    }
}
