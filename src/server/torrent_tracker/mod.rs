//! BitTorrent Tracker server implementation
//!
//! HTTP-based tracker for coordinating BitTorrent peers. Handles announce and scrape requests.

pub mod actions;

use anyhow::Result;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::server::connection::ConnectionId;
use actions::TorrentTrackerProtocol;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// BitTorrent Tracker server
pub struct TorrentTrackerServer;

impl TorrentTrackerServer {
    /// Spawn BitTorrent Tracker server with LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let listener = TcpListener::bind(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("BitTorrent Tracker server (action-based) listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] BitTorrent Tracker server listening on {}", local_addr));

        let protocol = Arc::new(TorrentTrackerProtocol::new());

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();

                        debug!("BitTorrent Tracker accepted connection from {}", peer_addr);
                        let _ = status_clone.send(format!("[DEBUG] BitTorrent Tracker accepted connection from {}", peer_addr));

                        // Add connection to ServerInstance
                        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
                        let now = std::time::Instant::now();
                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr: peer_addr,
                            local_addr,
                            bytes_sent: 0,
                            bytes_received: 0,
                            packets_sent: 0,
                            packets_received: 0,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::empty(),
                        };
                        state_clone.add_connection_to_server(server_id, conn_state).await;
                        let _ = status_clone.send("__UPDATE_UI__".to_string());

                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_connection(
                                stream,
                                peer_addr,
                                local_addr,
                                connection_id,
                                llm_clone,
                                state_clone,
                                status_clone,
                                server_id,
                                protocol_clone,
                            ).await {
                                error!("BitTorrent Tracker connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "BitTorrent Tracker accept error: {}", e);
                    }
                }
            }
        });

        Ok(local_addr)
    }

    async fn handle_connection(
        stream: tokio::net::TcpStream,
        peer_addr: SocketAddr,
        _local_addr: SocketAddr,
        connection_id: ConnectionId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        protocol: Arc<TorrentTrackerProtocol>,
    ) -> Result<()> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let (mut read_half, mut write_half) = tokio::io::split(stream);
        let mut buffer = vec![0u8; 8192];

        // Read HTTP request
        let n = read_half.read(&mut buffer).await?;
        if n == 0 {
            debug!("BitTorrent Tracker connection closed by peer");
            let _ = status_tx.send("[DEBUG] BitTorrent Tracker connection closed by peer".to_string());
            return Ok(());
        }

        let request_data = buffer[..n].to_vec();

        // DEBUG: Log summary
        console_debug!(status_tx, "BitTorrent Tracker received {} bytes from {}", n, peer_addr);

        // TRACE: Log full request
        if let Ok(request_str) = std::str::from_utf8(&request_data) {
            console_trace!(status_tx, "BitTorrent Tracker request: {}", request_str);
        }

        // Parse HTTP request
        let request_str = String::from_utf8_lossy(&request_data);
        let (request_type, request_params) = Self::parse_http_request(&request_str)?;

        console_debug!(status_tx, "BitTorrent Tracker request type: {}", request_type);

        // Create event for LLM
        let event_type = match request_type.as_str() {
            "announce" => &actions::TRACKER_ANNOUNCE_REQUEST_EVENT,
            "scrape" => &actions::TRACKER_SCRAPE_REQUEST_EVENT,
            _ => &actions::TRACKER_ANNOUNCE_REQUEST_EVENT, // Default to announce
        };
        let event = Event::new(event_type, serde_json::json!(request_params));

        console_debug!(status_tx, "BitTorrent Tracker calling LLM for {} request", request_type);

        // Call LLM
        match call_llm(
            &llm_client,
            &app_state,
            server_id,
            Some(connection_id),
            &event,
            protocol.as_ref(),
        ).await {
            Ok(execution_result) => {
                // Display messages from LLM
                for message in &execution_result.messages {
                    console_info!(status_tx, "{}", message);
                }

                console_debug!(status_tx, "BitTorrent Tracker got {} protocol results", execution_result.protocol_results.len());

                // Send responses
                for protocol_result in execution_result.protocol_results {
                    if let Some(output_data) = protocol_result.get_all_output().first() {
                        write_half.write_all(output_data).await?;

                        console_debug!(status_tx, "BitTorrent Tracker sent {} bytes to {}", output_data.len(), peer_addr);

                        // TRACE: Log full response
                        if let Ok(response_str) = std::str::from_utf8(output_data) {
                            console_trace!(status_tx, "BitTorrent Tracker response: {}", response_str);
                        }
                    }
                }
            }
            Err(e) => {
                error!("BitTorrent Tracker LLM call failed: {}", e);
                let _ = status_tx.send(format!("✗ BitTorrent Tracker LLM error: {}", e));

                // Send error response
                let error_response = b"HTTP/1.1 500 Internal Server Error\r\n\r\n";
                write_half.write_all(error_response).await?;
            }
        }

        Ok(())
    }

    fn parse_http_request(request: &str) -> Result<(String, HashMap<String, serde_json::Value>)> {
        // Parse HTTP request line
        let first_line = request.lines().next()
            .ok_or_else(|| anyhow::anyhow!("Empty request"))?;

        let parts: Vec<&str> = first_line.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(anyhow::anyhow!("Invalid HTTP request"));
        }

        let path = parts[1];

        // Determine request type (announce or scrape)
        let request_type = if path.starts_with("/announce") {
            "announce"
        } else if path.starts_with("/scrape") {
            "scrape"
        } else {
            "unknown"
        };

        // Parse query parameters
        let mut params = HashMap::new();
        if let Some(query_start) = path.find('?') {
            let query = &path[query_start + 1..];
            for param in query.split('&') {
                if let Some(eq_pos) = param.find('=') {
                    let key = urlencoding::decode(&param[..eq_pos])?.into_owned();
                    let value = urlencoding::decode(&param[eq_pos + 1..])?.into_owned();

                    // Special handling for binary fields (info_hash, peer_id)
                    if key == "info_hash" || key == "peer_id" {
                        params.insert(key, serde_json::json!(hex::encode(value.as_bytes())));
                    } else if key == "port" || key == "uploaded" || key == "downloaded" || key == "left" || key == "numwant" || key == "compact" {
                        // Numeric fields
                        if let Ok(num) = value.parse::<u64>() {
                            params.insert(key, serde_json::json!(num));
                        } else {
                            params.insert(key, serde_json::json!(value));
                        }
                    } else {
                        params.insert(key, serde_json::json!(value));
                    }
                }
            }
        }

        Ok((request_type.to_string(), params))
    }
}
