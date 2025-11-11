//! TCP server implementation
pub mod actions;

use anyhow::{Context, Result};
use bytes::Bytes;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};

use super::connection::ConnectionId;
use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ActionResult;
use actions::{TCP_CONNECTION_OPENED_EVENT, TCP_DATA_RECEIVED_EVENT};
use crate::server::TcpProtocol;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-connection data for LLM handling
struct ConnectionData {
    state: ConnectionState,
    queued_data: Vec<u8>,
    memory: String,
    write_half: Arc<Mutex<tokio::io::WriteHalf<TcpStream>>>,
}

/// TCP server that listens for incoming connections
pub struct TcpServer;

impl TcpServer {
    /// Spawn the TCP server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        send_first: bool,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        // Create and bind TCP server
        let listener = crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("TCP server (action-based) listening on {}", local_addr);

        let connections = Arc::new(Mutex::new(HashMap::new()));
        let protocol = Arc::new(TcpProtocol::new());

        // Spawn accept loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        info!("Accepted connection {} from {}", connection_id, remote_addr);

                        // Split stream
                        let (read_half, write_half) = tokio::io::split(stream);
                        let write_half_arc = Arc::new(Mutex::new(write_half));

                        // Add connection to ServerInstance
                        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
                        let now = std::time::Instant::now();
                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr,
                            local_addr: local_addr_conn,
                            bytes_sent: 0,
                            bytes_received: 0,
                            packets_sent: 0,
                            packets_received: 0,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::new(serde_json::json!({
                                "state": "Idle"
                            })),
                        };
                        app_state.add_connection_to_server(server_id, conn_state).await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        // Handle connection (send data first if needed)
                        let llm_client_clone = llm_client.clone();
                        let app_state_clone = app_state.clone();
                        let status_tx_clone = status_tx.clone();
                        let connections_clone = connections.clone();
                        let write_half_for_conn = write_half_arc.clone();
                        let protocol_clone = protocol.clone();
                        tokio::spawn(async move {
                            Self::handle_connection_with_actions(
                                connection_id,
                                server_id,
                                llm_client_clone,
                                app_state_clone,
                                status_tx_clone,
                                send_first,
                                connections_clone,
                                write_half_for_conn,
                                protocol_clone,
                            ).await;
                        });

                        // Spawn reader task
                        let llm_client_clone = llm_client.clone();
                        let app_state_clone = app_state.clone();
                        let status_tx_clone = status_tx.clone();
                        let connections_clone = connections.clone();
                        let protocol_clone = protocol.clone();
                        tokio::spawn(async move {
                            let mut buffer = vec![0u8; 8192];
                            let mut read_half = read_half;

                            loop {
                                match read_half.read(&mut buffer).await {
                                    Ok(0) => {
                                        // Connection closed
                                        connections_clone.lock().await.remove(&connection_id);
                                        app_state_clone.close_connection_on_server(server_id, connection_id).await;
                                        let _ = status_tx_clone.send(format!("✗ Connection {connection_id} closed"));
                                        let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                                        break;
                                    }
                                    Ok(n) => {
                                        let data = Bytes::copy_from_slice(&buffer[..n]);

                                        // DEBUG: Log summary with data preview
                                        if data.iter().all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace()) {
                                            let data_str = String::from_utf8_lossy(&data);
                                            let preview = if data_str.len() > 100 {
                                                format!("{}...", &data_str[..100])
                                            } else {
                                                data_str.to_string()
                                            };
                                            debug!("TCP received {} bytes on {}: {}", n, connection_id, preview);
                                            let _ = status_tx_clone.send(format!("[DEBUG] TCP received {} bytes on {}: {}", n, connection_id, preview));

                                            // TRACE: Log full text payload
                                            trace!("TCP data (text): {:?}", data_str);
                                            let _ = status_tx_clone.send(format!("[TRACE] TCP data (text): {:?}", data_str));
                                        } else {
                                            debug!("TCP received {} bytes on {} (binary data)", n, connection_id);
                                            let _ = status_tx_clone.send(format!("[DEBUG] TCP received {} bytes on {} (binary data)", n, connection_id));

                                            // TRACE: Log full hex payload
                                            let hex_str = hex::encode(&data);
                                            trace!("TCP data (hex): {}", hex_str);
                                            let _ = status_tx_clone.send(format!("[TRACE] TCP data (hex): {}", hex_str));
                                        }

                                        // Handle data in separate task
                                        let llm_clone = llm_client_clone.clone();
                                        let state_clone = app_state_clone.clone();
                                        let status_clone = status_tx_clone.clone();
                                        let conns_clone = connections_clone.clone();
                                        let protocol_clone = protocol_clone.clone();
                                        tokio::spawn(async move {
                                            Self::handle_data_with_actions(
                                                connection_id,
                                                server_id,
                                                data,
                                                llm_clone,
                                                state_clone,
                                                status_clone,
                                                conns_clone,
                                                protocol_clone,
                                            ).await;
                                        });
                                    }
                                    Err(e) => {
                                        error!("Read error on {}: {}", connection_id, e);
                                        connections_clone.lock().await.remove(&connection_id);
                                        break;
                                    }
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("Accept error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Handle new connection with LLM actions
    #[allow(clippy::too_many_arguments)]
    async fn handle_connection_with_actions(
        connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        send_first: bool,
        connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
        write_half: Arc<Mutex<tokio::io::WriteHalf<TcpStream>>>,
        protocol: Arc<TcpProtocol>,
    ) {
        // Add connection to tracking
        connections.lock().await.insert(connection_id, ConnectionData {
            state: ConnectionState::Idle,
            queued_data: Vec::new(),
            memory: String::new(),
            write_half: write_half.clone(),
        });

        // Send data first if requested
        if send_first {
            // Create connection opened event
            let event = Event::new(&TCP_CONNECTION_OPENED_EVENT, serde_json::json!({}));

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
                    debug!("LLM TCP banner response received");

                    // Display messages
                    for msg in execution_result.messages {
                        let _ = status_tx.send(msg);
                    }

                    // Handle protocol results (send banner)
                    for protocol_result in execution_result.protocol_results {
                        match protocol_result {
                            ActionResult::Output(output_data) => {
                                let mut write = write_half.lock().await;
                                if let Err(e) = write.write_all(&output_data).await {
                                    error!("Failed to send banner: {}", e);
                                } else {
                                    // DEBUG: Log summary with data preview
                                    if output_data.iter().all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace()) {
                                        let data_str = String::from_utf8_lossy(&output_data);
                                        let preview = if data_str.len() > 100 {
                                            format!("{}...", &data_str[..100])
                                        } else {
                                            data_str.to_string()
                                        };
                                        console_debug!(status_tx, "TCP sent {} bytes to {}: {}", output_data.len(), connection_id, preview);

                                        // TRACE: Log full text payload
                                        console_trace!(status_tx, "TCP sent (text): {:?}", data_str);
                                    } else {
                                        console_debug!(status_tx, "TCP sent {} bytes to {} (binary data)", output_data.len(), connection_id);

                                        // TRACE: Log full hex payload
                                        let hex_str = hex::encode(&output_data);
                                        console_trace!(status_tx, "TCP sent (hex): {}", hex_str);
                                    }
                                    let _ = status_tx.send(format!("→ Sent banner to {connection_id}"));
                                }
                            }
                            ActionResult::CloseConnection => {
                                connections.lock().await.remove(&connection_id);
                                let _ = status_tx.send(format!("✗ Closed connection {connection_id} after banner"));
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error generating banner: {}", e);
                    let _ = status_tx.send(format!("✗ LLM error: {e}"));
                }
            }
        }
    }

    /// Handle data received on a connection with LLM actions
    #[allow(clippy::too_many_arguments)]
    async fn handle_data_with_actions(
        connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        data: Bytes,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
        protocol: Arc<TcpProtocol>,
    ) {
        // Check connection state
        let current_state = {
            let conns = connections.lock().await;
            if let Some(conn_data) = conns.get(&connection_id) {
                conn_data.state.clone()
            } else {
                return; // Connection not found
            }
        };

        // If processing, queue the data
        if current_state == ConnectionState::Processing {
            connections.lock().await
                .entry(connection_id)
                .and_modify(|conn| {
                    conn.queued_data.extend_from_slice(&data);
                });
            let _ = status_tx.send(format!("⏸ Queued {} bytes for {}", data.len(), connection_id));
            return;
        }

        // Merge any queued data with new data
        let all_data = {
            let mut conns = connections.lock().await;
            let conn_data = conns.get_mut(&connection_id).unwrap();
            conn_data.state = ConnectionState::Processing;
            let mut merged = conn_data.queued_data.clone();
            merged.extend_from_slice(&data);
            conn_data.queued_data.clear();
            Bytes::from(merged)
        };

        loop {
            // Get memory
            let memory = {
                let conns = connections.lock().await;
                conns.get(&connection_id).map(|c| c.memory.clone()).unwrap_or_default()
            };

            // Get write_half for context
            let write_half = {
                let conns = connections.lock().await;
                conns.get(&connection_id).map(|c| c.write_half.clone())
            };

            let Some(write_half) = write_half else {
                return; // Connection not found
            };

            // Format data for event parameter
            let data_str = if all_data.iter().all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace()) {
                String::from_utf8_lossy(&all_data).to_string()
            } else {
                hex::encode(&all_data)
            };

            // Create data received event
            let event = Event::new(&TCP_DATA_RECEIVED_EVENT, serde_json::json!({
                "data": data_str
            }));

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
                    debug!("LLM TCP response received");

                    // Update memory
                    connections.lock().await
                        .entry(connection_id)
                        .and_modify(|conn| conn.memory = memory.clone());

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
                                let mut write = write_half.lock().await;
                                if let Err(e) = write.write_all(&output_data).await {
                                    error!("Failed to send response: {}", e);
                                } else {
                                    // DEBUG: Log summary with data preview
                                    if output_data.iter().all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace()) {
                                        let data_str = String::from_utf8_lossy(&output_data);
                                        let preview = if data_str.len() > 100 {
                                            format!("{}...", &data_str[..100])
                                        } else {
                                            data_str.to_string()
                                        };
                                        console_debug!(status_tx, "TCP sent {} bytes to {}: {}", output_data.len(), connection_id, preview);

                                        // TRACE: Log full text payload
                                        console_trace!(status_tx, "TCP sent (text): {:?}", data_str);
                                    } else {
                                        console_debug!(status_tx, "TCP sent {} bytes to {} (binary data)", output_data.len(), connection_id);

                                        // TRACE: Log full hex payload
                                        let hex_str = hex::encode(&output_data);
                                        console_trace!(status_tx, "TCP sent (hex): {}", hex_str);
                                    }
                                    let _ = status_tx.send(format!("→ Sent {} bytes to {}", output_data.len(), connection_id));
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
                        connections.lock().await
                            .entry(connection_id)
                            .and_modify(|conn| conn.state = ConnectionState::Accumulating);
                        let _ = status_tx.send(format!("⏳ Waiting for more data from {connection_id}"));
                        return;
                    }

                    // Handle close_connection
                    if should_close {
                        connections.lock().await.remove(&connection_id);
                        let _ = status_tx.send(format!("✗ Closed connection {connection_id}"));
                        return;
                    }

                    // Check for queued data
                    let has_queued = {
                        let conns = connections.lock().await;
                        conns.get(&connection_id)
                            .map(|c| !c.queued_data.is_empty())
                            .unwrap_or(false)
                    };

                    if has_queued {
                        let _ = status_tx.send(format!("▶ Processing queued data for {connection_id}"));
                        // Loop continues to process queued data
                    } else {
                        // Go to Idle state
                        connections.lock().await
                            .entry(connection_id)
                            .and_modify(|conn| conn.state = ConnectionState::Idle);
                        return;
                    }
                }
                Err(e) => {
                    error!("LLM error for TCP data: {}", e);
                    let _ = status_tx.send(format!("✗ LLM error: {e}"));
                    connections.lock().await
                        .entry(connection_id)
                        .and_modify(|conn| conn.state = ConnectionState::Idle);
                    return;
                }
            }
        }
    }
}

/// Send data on a TCP connection
pub async fn send_data(stream: &mut TcpStream, data: &[u8]) -> Result<()> {
    stream
        .write_all(data)
        .await
        .context("Failed to write data")?;
    stream.flush().await.context("Failed to flush stream")?;
    Ok(())
}
