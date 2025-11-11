//! Unix domain socket server implementation
//!
//! Platform: Unix/Linux only (uses Unix domain sockets)
#![cfg(unix)]

pub mod actions;

use anyhow::{Context, Result};
use bytes::Bytes;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};

use super::connection::ConnectionId;
use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ActionResult;
use actions::{SOCKET_FILE_CONNECTION_OPENED_EVENT, SOCKET_FILE_DATA_RECEIVED_EVENT};
use crate::server::SocketFileProtocol;
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
    write_half: Arc<Mutex<tokio::io::WriteHalf<UnixStream>>>,
}

/// Unix domain socket server that listens for incoming connections
pub struct SocketFileServer;

impl SocketFileServer {
    /// Spawn the socket file server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        socket_path: PathBuf,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        send_first: bool,
        server_id: crate::state::ServerId,
    ) -> Result<PathBuf> {
        // Remove existing socket file if present
        if socket_path.exists() {
            std::fs::remove_file(&socket_path)
                .with_context(|| format!("Failed to remove existing socket file: {:?}", socket_path))?;
        }

        // Create and bind Unix domain socket server
        let listener = tokio::net::UnixListener::bind(&socket_path)
            .with_context(|| format!("Failed to bind to socket path: {:?}", socket_path))?;

        info!("Socket file server listening on {:?}", socket_path);

        let connections = Arc::new(Mutex::new(HashMap::new()));
        let protocol = Arc::new(SocketFileProtocol::new());

        let socket_path_clone = socket_path.clone();

        // Spawn accept loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
                        info!("Accepted socket file connection {}", connection_id);

                        // Split stream
                        let (read_half, write_half) = tokio::io::split(stream);
                        let write_half_arc = Arc::new(Mutex::new(write_half));

                        // Add connection to ServerInstance
                        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
                        let now = std::time::Instant::now();
                        // Use a dummy SocketAddr since Unix sockets don't have IP addresses
                        let dummy_addr = "127.0.0.1:0".parse().unwrap();
                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr: dummy_addr,
                            local_addr: dummy_addr,
                            bytes_sent: 0,
                            bytes_received: 0,
                            packets_sent: 0,
                            packets_received: 0,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::new(serde_json::json!({
                                "state": "Idle",
                                "socket_path": socket_path_clone.to_string_lossy()
                            })),
                        };
                        app_state.add_connection_to_server(server_id, conn_state).await;
                        console_info!(status_tx, "__UPDATE_UI__");

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
                                        let _ = status_tx_clone.send(format!("✗ Socket file connection {connection_id} closed"));
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
                                            debug!("Socket file received {} bytes on {}: {}", n, connection_id, preview);
                                            let _ = status_tx_clone.send(format!("[DEBUG] Socket file received {} bytes on {}: {}", n, connection_id, preview));

                                            // TRACE: Log full text payload
                                            trace!("Socket file data (text): {:?}", data_str);
                                            let _ = status_tx_clone.send(format!("[TRACE] Socket file data (text): {:?}", data_str));
                                        } else {
                                            debug!("Socket file received {} bytes on {} (binary data)", n, connection_id);
                                            let _ = status_tx_clone.send(format!("[DEBUG] Socket file received {} bytes on {} (binary data)", n, connection_id));

                                            // TRACE: Log full hex payload
                                            let hex_str = hex::encode(&data);
                                            trace!("Socket file data (hex): {}", hex_str);
                                            let _ = status_tx_clone.send(format!("[TRACE] Socket file data (hex): {}", hex_str));
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
                                        error!("Read error on socket file connection {}: {}", connection_id, e);
                                        connections_clone.lock().await.remove(&connection_id);
                                        break;
                                    }
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("Accept error on socket file: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(socket_path)
    }

    /// Handle new connection with LLM actions
    async fn handle_connection_with_actions(
        connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        send_first: bool,
        connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
        write_half: Arc<Mutex<tokio::io::WriteHalf<UnixStream>>>,
        protocol: Arc<SocketFileProtocol>,
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
            let event = Event::new(&SOCKET_FILE_CONNECTION_OPENED_EVENT, serde_json::json!({}));

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
                    debug!("LLM socket file banner response received");

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
                                    error!("Failed to send socket file banner: {}", e);
                                } else {
                                    // DEBUG: Log summary with data preview
                                    if output_data.iter().all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace()) {
                                        let data_str = String::from_utf8_lossy(&output_data);
                                        let preview = if data_str.len() > 100 {
                                            format!("{}...", &data_str[..100])
                                        } else {
                                            data_str.to_string()
                                        };
                                        console_debug!(status_tx, "[DEBUG] Socket file sent {} bytes to {}: {}", output_data.len(), connection_id, preview);

                                        // TRACE: Log full text payload
                                        console_trace!(status_tx, "[TRACE] Socket file sent (text): {:?}", data_str);
                                    } else {
                                        console_debug!(status_tx, "[DEBUG] Socket file sent {} bytes to {} (binary data)", output_data.len(), connection_id);

                                        // TRACE: Log full hex payload
                                        let hex_str = hex::encode(&output_data);
                                        console_trace!(status_tx, "[TRACE] Socket file sent (hex): {}", hex_str);
                                    }
                                    console_trace!(status_tx, "→ Sent banner to socket file connection {connection_id}");
                                }
                            }
                            ActionResult::CloseConnection => {
                                connections.lock().await.remove(&connection_id);
                                console_error!(status_tx, "✗ Closed socket file connection {connection_id} after banner");
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    console_error!(status_tx, "✗ LLM error: {e}");
                }
            }
        }
    }

    /// Handle data received on a connection with LLM actions
    async fn handle_data_with_actions(
        connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        data: Bytes,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
        protocol: Arc<SocketFileProtocol>,
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
            console_info!(status_tx, "⏸ Queued {} bytes for socket file connection {}", data.len(), connection_id);
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
            let event = Event::new(&SOCKET_FILE_DATA_RECEIVED_EVENT, serde_json::json!({
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
                    debug!("LLM socket file response received");

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
                                    error!("Failed to send socket file response: {}", e);
                                } else {
                                    // DEBUG: Log summary with data preview
                                    if output_data.iter().all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace()) {
                                        let data_str = String::from_utf8_lossy(&output_data);
                                        let preview = if data_str.len() > 100 {
                                            format!("{}...", &data_str[..100])
                                        } else {
                                            data_str.to_string()
                                        };
                                        console_debug!(status_tx, "[DEBUG] Socket file sent {} bytes to {}: {}", output_data.len(), connection_id, preview);

                                        // TRACE: Log full text payload
                                        console_trace!(status_tx, "[TRACE] Socket file sent (text): {:?}", data_str);
                                    } else {
                                        console_debug!(status_tx, "[DEBUG] Socket file sent {} bytes to {} (binary data)", output_data.len(), connection_id);

                                        // TRACE: Log full hex payload
                                        let hex_str = hex::encode(&output_data);
                                        console_trace!(status_tx, "[TRACE] Socket file sent (hex): {}", hex_str);
                                    }
                                    console_trace!(status_tx, "→ Sent {} bytes to socket file connection {}", output_data.len(), connection_id);
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
                        console_info!(status_tx, "⏳ Waiting for more data from socket file connection {connection_id}");
                        return;
                    }

                    // Handle close_connection
                    if should_close {
                        connections.lock().await.remove(&connection_id);
                        console_error!(status_tx, "✗ Closed socket file connection {connection_id}");
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
                        console_info!(status_tx, "▶ Processing queued data for socket file connection {connection_id}");
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
                    console_error!(status_tx, "✗ LLM error: {e}");
                    connections.lock().await
                        .entry(connection_id)
                        .and_modify(|conn| conn.state = ConnectionState::Idle);
                    return;
                }
            }
        }
    }
}
