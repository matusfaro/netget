//! HTTP3 server implementation using Quinn
pub mod actions;

use anyhow::{Context, Result};
use bytes::Bytes;
use quinn::{Endpoint, ServerConfig};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};

use super::connection::ConnectionId;
use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ActionResult;
use actions::{Http3Protocol, HTTP3_CONNECTION_OPENED_EVENT, HTTP3_DATA_RECEIVED_EVENT, HTTP3_STREAM_OPENED_EVENT};
use crate::protocol::Event;
use crate::state::app_state::AppState;

/// Stream state for LLM processing
#[derive(Debug, Clone, PartialEq)]
enum StreamState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-stream data for LLM handling
struct StreamData {
    state: StreamState,
    queued_data: Vec<u8>,
    memory: String,
    send_stream: Arc<Mutex<quinn::SendStream>>,
}

/// HTTP3 server that listens for incoming connections
pub struct Http3Server;

impl Http3Server {
    /// Spawn the HTTP3 server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        tls_config: Option<Arc<rustls::ServerConfig>>,
    ) -> Result<SocketAddr> {
        // Use provided TLS config or generate default
        let mut server_crypto = match tls_config {
            Some(config) => (*config).clone(),
            None => {
                // Generate default self-signed certificate
                let config = crate::server::tls_cert_manager::generate_default_tls_config()
                    .context("Failed to generate default TLS config")?;
                (*config).clone()
            }
        };

        // Ensure ALPN protocols include h3
        server_crypto.alpn_protocols = vec![b"h3".to_vec()];

        // Create HTTP3 server configuration
        let mut server_config = ServerConfig::with_crypto(Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)
                .context("Failed to create HTTP3 crypto config")?
        ));

        // Configure transport parameters
        let mut transport_config = quinn::TransportConfig::default();
        transport_config.max_concurrent_bidi_streams(100_u32.into());
        transport_config.max_concurrent_uni_streams(100_u32.into());
        server_config.transport_config(Arc::new(transport_config));

        // Bind endpoint
        let endpoint = Endpoint::server(server_config, listen_addr)
            .context("Failed to create HTTP3 endpoint")?;
        let local_addr = endpoint.local_addr()
            .context("Failed to get local address")?;

        info!("HTTP3 server (action-based) listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] HTTP3 server listening on {}", local_addr));

        let protocol = Arc::new(Http3Protocol::new());

        // Spawn accept loop
        tokio::spawn(async move {
            while let Some(connecting) = endpoint.accept().await {
                let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
                let llm_client_clone = llm_client.clone();
                let app_state_clone = app_state.clone();
                let status_tx_clone = status_tx.clone();
                let protocol_clone = protocol.clone();

                tokio::spawn(async move {
                    match connecting.await {
                        Ok(connection) => {
                            let remote_addr = connection.remote_address();
                            info!("Accepted HTTP3 connection {} from {}", connection_id, remote_addr);
                            let _ = status_tx_clone.send(format!("✓ HTTP3 connection {} from {}", connection_id, remote_addr));

                            // Add connection to ServerInstance
                            use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
                            let now = std::time::Instant::now();
                            let conn_state = ServerConnectionState {
                                id: connection_id,
                                remote_addr,
                                local_addr,
                                bytes_sent: 0,
                                bytes_received: 0,
                                packets_sent: 0,
                                packets_received: 0,
                                last_activity: now,
                                status: ConnectionStatus::Active,
                                status_changed_at: now,
                                protocol_info: ProtocolConnectionInfo::new(serde_json::json!({
                                    "stream_count": 0
                                })),
                            };
                            app_state_clone.add_connection_to_server(server_id, conn_state).await;
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());

                            // Notify LLM of new connection
                            let event = Event::new(&HTTP3_CONNECTION_OPENED_EVENT, serde_json::json!({}));
                            match call_llm(
                                &llm_client_clone,
                                &app_state_clone,
                                server_id,
                                Some(connection_id),
                                &event,
                                protocol_clone.as_ref(),
                            ).await {
                                Ok(execution_result) => {
                                    for msg in execution_result.messages {
                                        let _ = status_tx_clone.send(msg);
                                    }
                                }
                                Err(e) => {
                                    error!("LLM error on connection opened: {}", e);
                                    let _ = status_tx_clone.send(format!("✗ LLM error: {e}"));
                                }
                            }

                            // Handle streams on this connection
                            let streams = Arc::new(Mutex::new(HashMap::new()));
                            loop {
                                match connection.accept_bi().await {
                                    Ok((send_stream, recv_stream)) => {
                                        let stream_id = ConnectionId::new(
                                            app_state_clone.get_next_unified_id().await
                                        );
                                        info!("Accepted HTTP3 stream {} on connection {}", stream_id, connection_id);
                                        let _ = status_tx_clone.send(format!("→ Stream {} opened on connection {}", stream_id, connection_id));

                                        let llm_clone = llm_client_clone.clone();
                                        let state_clone = app_state_clone.clone();
                                        let status_clone = status_tx_clone.clone();
                                        let streams_clone = streams.clone();
                                        let protocol_clone = protocol_clone.clone();

                                        tokio::spawn(async move {
                                            Self::handle_stream_with_actions(
                                                stream_id,
                                                connection_id,
                                                server_id,
                                                send_stream,
                                                recv_stream,
                                                llm_clone,
                                                state_clone,
                                                status_clone,
                                                streams_clone,
                                                protocol_clone,
                                            ).await;
                                        });
                                    }
                                    Err(quinn::ConnectionError::ApplicationClosed(_)) => {
                                        info!("HTTP3 connection {} closed by peer", connection_id);
                                        let _ = status_tx_clone.send(format!("✗ HTTP3 connection {} closed", connection_id));
                                        break;
                                    }
                                    Err(e) => {
                                        error!("Error accepting stream on {}: {}", connection_id, e);
                                        let _ = status_tx_clone.send(format!("✗ Stream accept error on {}: {}", connection_id, e));
                                        break;
                                    }
                                }
                            }

                            // Connection closed
                            app_state_clone.close_connection_on_server(server_id, connection_id).await;
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        }
                        Err(e) => {
                            error!("Connection error on {}: {}", connection_id, e);
                            let _ = status_tx_clone.send(format!("✗ Connection error on {}: {}", connection_id, e));
                        }
                    }
                        });
            }
        });

        Ok(local_addr)
    }

    /// Handle a HTTP3 stream with LLM actions
    #[allow(clippy::too_many_arguments)]
    async fn handle_stream_with_actions(
        stream_id: ConnectionId,
        _connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        send_stream: quinn::SendStream,
        mut recv_stream: quinn::RecvStream,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        streams: Arc<Mutex<HashMap<ConnectionId, StreamData>>>,
        protocol: Arc<Http3Protocol>,
    ) {
        let send_stream_arc = Arc::new(Mutex::new(send_stream));

        // Add stream to tracking
        streams.lock().await.insert(stream_id, StreamData {
            state: StreamState::Idle,
            queued_data: Vec::new(),
            memory: String::new(),
            send_stream: send_stream_arc.clone(),
        });

        // Notify LLM of new stream
        let event = Event::new(&HTTP3_STREAM_OPENED_EVENT, serde_json::json!({
            "stream_id": stream_id.to_string()
        }));

        match call_llm(
            &llm_client,
            &app_state,
            server_id,
            Some(stream_id),
            &event,
            protocol.as_ref(),
        ).await {
            Ok(execution_result) => {
                for msg in execution_result.messages {
                    let _ = status_tx.send(msg);
                }

                // Handle any initial actions
                for protocol_result in execution_result.protocol_results {
                    if let ActionResult::Output(output_data) = protocol_result {
                        let mut send = send_stream_arc.lock().await;
                        if let Err(e) = send.write_all(&output_data).await {
                            error!("Failed to send initial data on stream {}: {}", stream_id, e);
                        } else {
                            debug!("HTTP3 sent {} bytes on stream {}", output_data.len(), stream_id);
                            let _ = status_tx.send(format!("[DEBUG] HTTP3 sent {} bytes on stream {}", output_data.len(), stream_id));
                        }
                    }
                }
            }
            Err(e) => {
                error!("LLM error on stream opened: {}", e);
                let _ = status_tx.send(format!("✗ LLM error: {e}"));
            }
        }

        // Read loop
        let mut buffer = vec![0u8; 65536];
        loop {
            match recv_stream.read(&mut buffer).await {
                Ok(Some(n)) => {
                    let data = Bytes::copy_from_slice(&buffer[..n]);

                    // DEBUG: Log summary with data preview
                    if data.iter().all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace()) {
                        let data_str = String::from_utf8_lossy(&data);
                        let preview = if data_str.len() > 100 {
                            format!("{}...", &data_str[..100])
                        } else {
                            data_str.to_string()
                        };
                        debug!("HTTP3 received {} bytes on stream {}: {}", n, stream_id, preview);
                        let _ = status_tx.send(format!("[DEBUG] HTTP3 received {} bytes on stream {}: {}", n, stream_id, preview));

                        // TRACE: Log full text payload
                        trace!("HTTP3 data (text): {:?}", data_str);
                        let _ = status_tx.send(format!("[TRACE] HTTP3 data (text): {:?}", data_str));
                    } else {
                        debug!("HTTP3 received {} bytes on stream {} (binary data)", n, stream_id);
                        let _ = status_tx.send(format!("[DEBUG] HTTP3 received {} bytes on stream {} (binary data)", n, stream_id));

                        // TRACE: Log full hex payload
                        let hex_str = hex::encode(&data);
                        trace!("HTTP3 data (hex): {}", hex_str);
                        let _ = status_tx.send(format!("[TRACE] HTTP3 data (hex): {}", hex_str));
                    }

                    // Handle data in separate task
                    let llm_clone = llm_client.clone();
                    let state_clone = app_state.clone();
                    let status_clone = status_tx.clone();
                    let streams_clone = streams.clone();
                    let protocol_clone = protocol.clone();
                    tokio::spawn(async move {
                        Self::handle_data_with_actions(
                            stream_id,
                            server_id,
                            data,
                            llm_clone,
                            state_clone,
                            status_clone,
                            streams_clone,
                            protocol_clone,
                        ).await;
                    });
                }
                Ok(None) => {
                    // Stream finished
                    info!("HTTP3 stream {} finished", stream_id);
                    let _ = status_tx.send(format!("✗ HTTP3 stream {} closed", stream_id));
                    streams.lock().await.remove(&stream_id);
                    break;
                }
                Err(e) => {
                    error!("Read error on stream {}: {}", stream_id, e);
                    let _ = status_tx.send(format!("✗ Read error on stream {}: {}", stream_id, e));
                    streams.lock().await.remove(&stream_id);
                    break;
                }
            }
        }
    }

    /// Handle data received on a stream with LLM actions
    #[allow(clippy::too_many_arguments)]
    async fn handle_data_with_actions(
        stream_id: ConnectionId,
        server_id: crate::state::ServerId,
        data: Bytes,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        streams: Arc<Mutex<HashMap<ConnectionId, StreamData>>>,
        protocol: Arc<Http3Protocol>,
    ) {
        // Check stream state
        let current_state = {
            let streams_lock = streams.lock().await;
            if let Some(stream_data) = streams_lock.get(&stream_id) {
                stream_data.state.clone()
            } else {
                return; // Stream not found
            }
        };

        // If processing, queue the data
        if current_state == StreamState::Processing {
            streams.lock().await
                .entry(stream_id)
                .and_modify(|s| {
                    s.queued_data.extend_from_slice(&data);
                });
            let _ = status_tx.send(format!("⏸ Queued {} bytes for stream {}", data.len(), stream_id));
            return;
        }

        // Merge any queued data with new data
        let all_data = {
            let mut streams_lock = streams.lock().await;
            let stream_data = streams_lock.get_mut(&stream_id).unwrap();
            stream_data.state = StreamState::Processing;
            let mut merged = stream_data.queued_data.clone();
            merged.extend_from_slice(&data);
            stream_data.queued_data.clear();
            Bytes::from(merged)
        };

        loop {
            // Get memory
            let memory = {
                let streams_lock = streams.lock().await;
                streams_lock.get(&stream_id).map(|s| s.memory.clone()).unwrap_or_default()
            };

            // Get send_stream for context
            let send_stream = {
                let streams_lock = streams.lock().await;
                streams_lock.get(&stream_id).map(|s| s.send_stream.clone())
            };

            let Some(send_stream) = send_stream else {
                return; // Stream not found
            };

            // Format data for event parameter
            let data_str = if all_data.iter().all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace()) {
                String::from_utf8_lossy(&all_data).to_string()
            } else {
                hex::encode(&all_data)
            };

            // Create data received event
            let event = Event::new(&HTTP3_DATA_RECEIVED_EVENT, serde_json::json!({
                "stream_id": stream_id.to_string(),
                "data": data_str
            }));

            // Call LLM
            match call_llm(
                &llm_client,
                &app_state,
                server_id,
                Some(stream_id),
                &event,
                protocol.as_ref(),
            ).await {
                Ok(execution_result) => {
                    debug!("LLM HTTP3 response received");

                    // Update memory
                    streams.lock().await
                        .entry(stream_id)
                        .and_modify(|s| s.memory = memory.clone());

                    // Display messages
                    for msg in execution_result.messages {
                        let _ = status_tx.send(msg);
                    }

                    // Handle protocol results
                    let mut should_close = false;
                    let mut should_wait = false;

                    for protocol_result in execution_result.protocol_results {
                        match protocol_result {
                            ActionResult::Output(output_data) => {
                                let mut send = send_stream.lock().await;
                                if let Err(e) = send.write_all(&output_data).await {
                                    error!("Failed to send response on stream {}: {}", stream_id, e);
                                } else {
                                    // DEBUG: Log summary with data preview
                                    if output_data.iter().all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace()) {
                                        let data_str = String::from_utf8_lossy(&output_data);
                                        let preview = if data_str.len() > 100 {
                                            format!("{}...", &data_str[..100])
                                        } else {
                                            data_str.to_string()
                                        };
                                        debug!("HTTP3 sent {} bytes on stream {}: {}", output_data.len(), stream_id, preview);
                                        let _ = status_tx.send(format!("[DEBUG] HTTP3 sent {} bytes on stream {}: {}", output_data.len(), stream_id, preview));

                                        // TRACE: Log full text payload
                                        trace!("HTTP3 sent (text): {:?}", data_str);
                                        let _ = status_tx.send(format!("[TRACE] HTTP3 sent (text): {:?}", data_str));
                                    } else {
                                        debug!("HTTP3 sent {} bytes on stream {} (binary data)", output_data.len(), stream_id);
                                        let _ = status_tx.send(format!("[DEBUG] HTTP3 sent {} bytes on stream {} (binary data)", output_data.len(), stream_id));

                                        // TRACE: Log full hex payload
                                        let hex_str = hex::encode(&output_data);
                                        trace!("HTTP3 sent (hex): {}", hex_str);
                                        let _ = status_tx.send(format!("[TRACE] HTTP3 sent (hex): {}", hex_str));
                                    }
                                    let _ = status_tx.send(format!("→ Sent {} bytes on stream {}", output_data.len(), stream_id));
                                }
                            }
                            ActionResult::CloseConnection => {
                                should_close = true;
                            }
                            ActionResult::WaitForMore => {
                                should_wait = true;
                            }
                            _ => {}
                        }
                    }

                    // Handle wait_for_more
                    if should_wait {
                        streams.lock().await
                            .entry(stream_id)
                            .and_modify(|s| s.state = StreamState::Accumulating);
                        let _ = status_tx.send(format!("⏳ Waiting for more data on stream {}", stream_id));
                        return;
                    }

                    // Handle close_stream
                    if should_close {
                        streams.lock().await.remove(&stream_id);
                        let _ = status_tx.send(format!("✗ Closed stream {}", stream_id));
                        return;
                    }

                    // Check for queued data
                    let has_queued = {
                        let streams_lock = streams.lock().await;
                        streams_lock.get(&stream_id)
                            .map(|s| !s.queued_data.is_empty())
                            .unwrap_or(false)
                    };

                    if has_queued {
                        let _ = status_tx.send(format!("▶ Processing queued data for stream {}", stream_id));
                        // Loop continues to process queued data
                    } else {
                        // Go to Idle state
                        streams.lock().await
                            .entry(stream_id)
                            .and_modify(|s| s.state = StreamState::Idle);
                        return;
                    }
                }
                Err(e) => {
                    error!("LLM error for HTTP3 data: {}", e);
                    let _ = status_tx.send(format!("✗ LLM error: {e}"));
                    streams.lock().await
                        .entry(stream_id)
                        .and_modify(|s| s.state = StreamState::Idle);
                    return;
                }
            }
        }
    }
}
