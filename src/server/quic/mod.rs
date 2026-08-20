//! QUIC server implementation using Quinn
pub mod actions;

use anyhow::{Context, Result};
use bytes::Bytes;
use quinn::{Endpoint, ServerConfig};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error};

use super::connection::ConnectionId;
use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ActionResult;
use crate::logging::emit::Log;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use actions::{
    QuicProtocol, QUIC_CONNECTION_OPENED_EVENT, QUIC_DATA_RECEIVED_EVENT, QUIC_STREAM_OPENED_EVENT,
};

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

/// QUIC server that listens for incoming connections
pub struct QuicServer;

impl QuicServer {
    /// Spawn the QUIC server with integrated LLM actions
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

        // Create QUIC server configuration
        let mut server_config = ServerConfig::with_crypto(Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)
                .context("Failed to create QUIC crypto config")?,
        ));

        // Configure transport parameters
        let mut transport_config = quinn::TransportConfig::default();
        transport_config.max_concurrent_bidi_streams(100_u32.into());
        transport_config.max_concurrent_uni_streams(100_u32.into());
        server_config.transport_config(Arc::new(transport_config));

        // Bind endpoint
        let endpoint = Endpoint::server(server_config, listen_addr)
            .context("Failed to create QUIC endpoint")?;
        let local_addr = endpoint
            .local_addr()
            .context("Failed to get local address")?;

        Log::new(Some(&status_tx)).info(format!(
            "QUIC server (action-based) listening on {}",
            local_addr
        ));

        let protocol = Arc::new(QuicProtocol::new());

        // Spawn accept loop
        let task_registrar = app_state.clone();
        let accept_handle = tokio::spawn(async move {
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
                            Log::new(Some(&status_tx_clone)).info(format!(
                                "Accepted QUIC connection {} from {}",
                                connection_id, remote_addr
                            ));

                            // Add connection to ServerInstance
                            use crate::state::server::{
                                ConnectionState as ServerConnectionState, ConnectionStatus,
                                ProtocolConnectionInfo,
                            };
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
                            app_state_clone
                                .add_connection_to_server(server_id, conn_state)
                                .await;
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());

                            // Notify LLM of new connection
                            let event =
                                Event::new(&QUIC_CONNECTION_OPENED_EVENT, serde_json::json!({}));
                            match call_llm(
                                &llm_client_clone,
                                &app_state_clone,
                                server_id,
                                Some(connection_id),
                                &event,
                                protocol_clone.as_ref(),
                            )
                            .await
                            {
                                Ok(execution_result) => {
                                    for msg in execution_result.messages {
                                        let _ = status_tx_clone.send(msg);
                                    }
                                }
                                Err(e) => {
                                    // Non-fatal: the connection carries on, so WARN.
                                    Log::new(Some(&status_tx_clone))
                                        .warn(format!("LLM error on connection opened: {}", e));
                                }
                            }

                            // Handle streams on this connection
                            let streams = Arc::new(Mutex::new(HashMap::new()));
                            loop {
                                match connection.accept_bi().await {
                                    Ok((send_stream, recv_stream)) => {
                                        let stream_id = ConnectionId::new(
                                            app_state_clone.get_next_unified_id().await,
                                        );
                                        Log::new(Some(&status_tx_clone)).info(format!(
                                            "Accepted QUIC stream {} on connection {}",
                                            stream_id, connection_id
                                        ));

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
                                            )
                                            .await;
                                        });
                                    }
                                    Err(quinn::ConnectionError::ApplicationClosed(_)) => {
                                        Log::new(Some(&status_tx_clone)).info(format!(
                                            "QUIC connection {} closed by peer",
                                            connection_id
                                        ));
                                        break;
                                    }
                                    Err(e) => {
                                        Log::new(Some(&status_tx_clone)).error(format!(
                                            "Error accepting stream on {}: {}",
                                            connection_id, e
                                        ));
                                        break;
                                    }
                                }
                            }

                            // Connection closed
                            app_state_clone
                                .close_connection_on_server(server_id, connection_id)
                                .await;
                            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        }
                        Err(e) => {
                            Log::new(Some(&status_tx_clone))
                                .error(format!("Connection error on {}: {}", connection_id, e));
                        }
                    }
                });
            }
        });

        task_registrar
            .register_server_task(server_id, accept_handle)
            .await;

        Ok(local_addr)
    }

    /// Handle a QUIC stream with LLM actions.
    ///
    /// Statistics are recorded against `connection_id`, not `stream_id`: only the
    /// QUIC connection is registered in the server's connection map, so an update
    /// keyed by a stream id would silently do nothing. Keeping `last_activity`
    /// fresh matters here more than for HTTP/1.1 — `cleanup_old_connections`
    /// evicts a connection idle for 10s, and a QUIC connection carrying a slow
    /// stream easily exceeds that.
    #[allow(clippy::too_many_arguments)]
    async fn handle_stream_with_actions(
        stream_id: ConnectionId,
        connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        send_stream: quinn::SendStream,
        mut recv_stream: quinn::RecvStream,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        streams: Arc<Mutex<HashMap<ConnectionId, StreamData>>>,
        protocol: Arc<QuicProtocol>,
    ) {
        let send_stream_arc = Arc::new(Mutex::new(send_stream));

        // Add stream to tracking
        streams.lock().await.insert(
            stream_id,
            StreamData {
                state: StreamState::Idle,
                queued_data: Vec::new(),
                memory: String::new(),
                send_stream: send_stream_arc.clone(),
            },
        );

        // Notify LLM of new stream
        let event = Event::new(
            &QUIC_STREAM_OPENED_EVENT,
            serde_json::json!({
                "stream_id": stream_id.to_string()
            }),
        );

        match call_llm(
            &llm_client,
            &app_state,
            server_id,
            Some(stream_id),
            &event,
            protocol.as_ref(),
        )
        .await
        {
            Ok(execution_result) => {
                for msg in execution_result.messages {
                    let _ = status_tx.send(msg);
                }

                // Handle any initial actions
                for protocol_result in execution_result.protocol_results {
                    if let ActionResult::Output(output_data) = protocol_result {
                        let write_result = {
                            let mut send = send_stream_arc.lock().await;
                            send.write_all(&output_data).await
                        };
                        if let Err(e) = write_result {
                            error!("Failed to send initial data on stream {}: {}", stream_id, e);
                        } else {
                            Log::new(Some(&status_tx)).debug(format!(
                                "QUIC sent {} bytes on stream {}",
                                output_data.len(),
                                stream_id
                            ));
                            app_state
                                .update_connection_stats(
                                    server_id,
                                    connection_id,
                                    None,
                                    Some(output_data.len() as u64),
                                    None,
                                    Some(1),
                                )
                                .await;
                        }
                    }
                }
            }
            Err(e) => {
                // Non-fatal: the read loop still runs, so WARN.
                Log::new(Some(&status_tx)).warn(format!("LLM error on stream opened: {}", e));
            }
        }

        // Read loop
        let mut buffer = vec![0u8; 65536];
        loop {
            match recv_stream.read(&mut buffer).await {
                Ok(Some(n)) => {
                    let data = Bytes::copy_from_slice(&buffer[..n]);

                    // Count every inbound read exactly once, before dispatch: the
                    // handler below may queue the data instead of processing it.
                    app_state
                        .update_connection_stats(
                            server_id,
                            connection_id,
                            Some(n as u64),
                            None,
                            Some(1),
                            None,
                        )
                        .await;

                    // Data summary + full payload are FileOnly: the quic_data_received
                    // event template renders the equivalent lines to the TUI, so streaming
                    // the payload here too would duplicate it and load the unbounded
                    // status channel.
                    let log = Log::new(Some(&status_tx));
                    if data
                        .iter()
                        .all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace())
                    {
                        let data_str = String::from_utf8_lossy(&data);
                        let preview = if data_str.len() > 100 {
                            format!("{}...", &data_str[..100])
                        } else {
                            data_str.to_string()
                        };
                        log.debug(format!(
                            "QUIC received {} bytes on stream {}: {}",
                            n, stream_id, preview
                        ));
                        log.trace(format!("QUIC data (text): {:?}", data_str));
                    } else {
                        log.debug(format!(
                            "QUIC received {} bytes on stream {} (binary data)",
                            n, stream_id
                        ));
                        log.trace(format!("QUIC data (hex): {}", hex::encode(&data)));
                    }

                    // Handle data directly (await to ensure processing completes before stream removal)
                    Self::handle_data_with_actions(
                        stream_id,
                        connection_id,
                        server_id,
                        data,
                        llm_client.clone(),
                        app_state.clone(),
                        status_tx.clone(),
                        streams.clone(),
                        protocol.clone(),
                    )
                    .await;
                }
                Ok(None) => {
                    // Stream finished
                    Log::new(Some(&status_tx)).info(format!("QUIC stream {} finished", stream_id));
                    streams.lock().await.remove(&stream_id);
                    break;
                }
                Err(e) => {
                    Log::new(Some(&status_tx))
                        .error(format!("Read error on stream {}: {}", stream_id, e));
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
        connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        data: Bytes,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        streams: Arc<Mutex<HashMap<ConnectionId, StreamData>>>,
        protocol: Arc<QuicProtocol>,
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
            streams.lock().await.entry(stream_id).and_modify(|s| {
                s.queued_data.extend_from_slice(&data);
            });
            Log::new(Some(&status_tx)).debug(format!(
                "Queued {} bytes for stream {}",
                data.len(),
                stream_id
            ));
            return;
        }

        // Merge any queued data with new data. The stream can disappear between
        // the state check above and this lock (peer reset, close_this_stream on
        // another task), so this must not unwrap.
        let mut all_data = {
            let mut streams_lock = streams.lock().await;
            let Some(stream_data) = streams_lock.get_mut(&stream_id) else {
                debug!(
                    "Stream {} closed before its data could be processed",
                    stream_id
                );
                return;
            };
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
                streams_lock
                    .get(&stream_id)
                    .map(|s| s.memory.clone())
                    .unwrap_or_default()
            };

            // Get send_stream for context
            let send_stream = {
                let streams_lock = streams.lock().await;
                streams_lock.get(&stream_id).map(|s| s.send_stream.clone())
            };

            let Some(send_stream) = send_stream else {
                return; // Stream not found
            };

            // Format data for the event parameter. Printable ASCII is passed
            // through as text, anything else is hex-encoded. `encoding` names
            // which of the two the model is looking at, and uses the same
            // vocabulary send_quic_data accepts, so the model can echo the
            // payload back byte-for-byte by passing both fields straight
            // through. (It used to say "text" where the outbound side had no
            // encoding field at all, which made binary un-echoable.)
            let (data_str, encoding) = crate::server::quic::actions::encode_quic_payload(&all_data);

            // Create data received event
            let event = Event::new(
                &QUIC_DATA_RECEIVED_EVENT,
                serde_json::json!({
                    "stream_id": stream_id.to_string(),
                    "data": data_str,
                    "encoding": encoding
                }),
            );

            // Call LLM
            match call_llm(
                &llm_client,
                &app_state,
                server_id,
                Some(stream_id),
                &event,
                protocol.as_ref(),
            )
            .await
            {
                Ok(execution_result) => {
                    debug!("LLM QUIC response received");

                    // Update memory
                    streams
                        .lock()
                        .await
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
                                let write_result = {
                                    let mut send = send_stream.lock().await;
                                    send.write_all(&output_data).await
                                };
                                if let Err(e) = write_result {
                                    error!(
                                        "Failed to send response on stream {}: {}",
                                        stream_id, e
                                    );
                                } else {
                                    app_state
                                        .update_connection_stats(
                                            server_id,
                                            connection_id,
                                            None,
                                            Some(output_data.len() as u64),
                                            None,
                                            Some(1),
                                        )
                                        .await;
                                    // Sent-data summary + payload are FileOnly: the
                                    // send_quic_data action template already reports the send
                                    // to the TUI.
                                    let log = Log::new(Some(&status_tx));
                                    if output_data
                                        .iter()
                                        .all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace())
                                    {
                                        let data_str = String::from_utf8_lossy(&output_data);
                                        let preview = if data_str.len() > 100 {
                                            format!("{}...", &data_str[..100])
                                        } else {
                                            data_str.to_string()
                                        };
                                        log.debug(format!(
                                            "QUIC sent {} bytes on stream {}: {}",
                                            output_data.len(),
                                            stream_id,
                                            preview
                                        ));
                                        log.trace(format!("QUIC sent (text): {:?}", data_str));
                                    } else {
                                        log.debug(format!(
                                            "QUIC sent {} bytes on stream {} (binary data)",
                                            output_data.len(),
                                            stream_id
                                        ));
                                        log.trace(format!(
                                            "QUIC sent (hex): {}",
                                            hex::encode(&output_data)
                                        ));
                                    }
                                    log.debug(format!(
                                        "Sent {} bytes on stream {}",
                                        output_data.len(),
                                        stream_id
                                    ));
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
                        streams
                            .lock()
                            .await
                            .entry(stream_id)
                            .and_modify(|s| s.state = StreamState::Accumulating);
                        Log::new(Some(&status_tx))
                            .debug(format!("Waiting for more data on stream {}", stream_id));
                        return;
                    }

                    // Handle close_stream
                    if should_close {
                        streams.lock().await.remove(&stream_id);
                        Log::new(Some(&status_tx)).info(format!("Closed stream {}", stream_id));
                        return;
                    }

                    // Check for queued data
                    let has_queued = {
                        let streams_lock = streams.lock().await;
                        streams_lock
                            .get(&stream_id)
                            .map(|s| !s.queued_data.is_empty())
                            .unwrap_or(false)
                    };

                    if has_queued {
                        // Take the queue and make it the next iteration's payload.
                        // Leaving it in place re-sent the SAME bytes to the model on
                        // every pass and never emptied the queue: one response per
                        // iteration, forever, for a single line of input.
                        let queued = {
                            let mut streams_lock = streams.lock().await;
                            match streams_lock.get_mut(&stream_id) {
                                Some(stream) => std::mem::take(&mut stream.queued_data),
                                None => return,
                            }
                        };
                        if queued.is_empty() {
                            streams
                                .lock()
                                .await
                                .entry(stream_id)
                                .and_modify(|s| s.state = StreamState::Idle);
                            return;
                        }
                        all_data = Bytes::from(queued);
                    } else {
                        // Go to Idle state
                        streams
                            .lock()
                            .await
                            .entry(stream_id)
                            .and_modify(|s| s.state = StreamState::Idle);
                        return;
                    }
                }
                Err(e) => {
                    Log::new(Some(&status_tx)).warn(format!("LLM error for QUIC data: {}", e));
                    streams
                        .lock()
                        .await
                        .entry(stream_id)
                        .and_modify(|s| s.state = StreamState::Idle);
                    return;
                }
            }
        }
    }
}
