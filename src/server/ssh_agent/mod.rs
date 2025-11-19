//! SSH Agent server implementation
//!
//! Platform: Unix/Linux/macOS (uses Unix domain sockets)
#![cfg(unix)]

pub mod actions;

use anyhow::{Context, Result};
use bytes::{BufMut, Bytes, BytesMut};
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
use crate::protocol::Event;
use crate::server::SshAgentProtocol;
use crate::state::app_state::AppState;
use actions::*;

/// SSH Agent message types (from SSH Agent Protocol specification)
const SSH_AGENTC_REQUEST_IDENTITIES: u8 = 11;
const SSH_AGENTC_SIGN_REQUEST: u8 = 13;
const SSH_AGENTC_ADD_IDENTITY: u8 = 17;
const SSH_AGENTC_REMOVE_IDENTITY: u8 = 18;
const SSH_AGENTC_REMOVE_ALL_IDENTITIES: u8 = 19;
const SSH_AGENTC_ADD_ID_CONSTRAINED: u8 = 25;
const SSH_AGENTC_LOCK: u8 = 22;
const SSH_AGENTC_UNLOCK: u8 = 23;

const SSH_AGENT_FAILURE: u8 = 5;
const SSH_AGENT_SUCCESS: u8 = 6;
const SSH_AGENT_IDENTITIES_ANSWER: u8 = 12;
const SSH_AGENT_SIGN_RESPONSE: u8 = 14;

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

/// SSH Agent server that listens for incoming connections
pub struct SshAgentServer;

impl SshAgentServer {
    /// Spawn the SSH Agent server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        socket_path: PathBuf,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<PathBuf> {
        // Remove existing socket file if present
        if socket_path.exists() {
            std::fs::remove_file(&socket_path).with_context(|| {
                format!("Failed to remove existing socket file: {:?}", socket_path)
            })?;
        }

        // Create and bind Unix domain socket server
        let listener = tokio::net::UnixListener::bind(&socket_path)
            .with_context(|| format!("Failed to bind to socket path: {:?}", socket_path))?;

        info!("SSH Agent server listening on {:?}", socket_path);
        let _ = status_tx.send(format!("SSH Agent server listening on {:?}", socket_path));

        let connections = Arc::new(Mutex::new(HashMap::new()));
        let protocol = Arc::new(SshAgentProtocol::new());

        let socket_path_clone = socket_path.clone();

        // Spawn accept loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);
                        info!("Accepted SSH Agent connection {}", connection_id);
                        let _ = status_tx
                            .send(format!("✓ SSH Agent connection {} opened", connection_id));

                        // Split stream
                        let (read_half, write_half) = tokio::io::split(stream);
                        let write_half_arc = Arc::new(Mutex::new(write_half));

                        // Add connection to ServerInstance
                        use crate::state::server::{
                            ConnectionState as ServerConnectionState, ConnectionStatus,
                            ProtocolConnectionInfo,
                        };
                        let now = std::time::Instant::now();
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
                        app_state
                            .add_connection_to_server(server_id, conn_state)
                            .await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        // Handle connection with LLM integration
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
                                connections_clone,
                                write_half_for_conn,
                                protocol_clone,
                            )
                            .await;
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
                                        app_state_clone
                                            .close_connection_on_server(server_id, connection_id)
                                            .await;
                                        let _ = status_tx_clone.send(format!(
                                            "✗ SSH Agent connection {} closed",
                                            connection_id
                                        ));
                                        let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                                        break;
                                    }
                                    Ok(n) => {
                                        let data = Bytes::copy_from_slice(&buffer[..n]);
                                        trace!(
                                            "SSH Agent received {} bytes on connection {}",
                                            n,
                                            connection_id
                                        );

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
                                            )
                                            .await;
                                        });
                                    }
                                    Err(e) => {
                                        error!(
                                            "Read error on SSH Agent connection {}: {}",
                                            connection_id, e
                                        );
                                        connections_clone.lock().await.remove(&connection_id);
                                        break;
                                    }
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept SSH Agent connection: {}", e);
                    }
                }
            }
        });

        Ok(socket_path)
    }

    /// Handle connection lifecycle
    async fn handle_connection_with_actions(
        connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
        write_half: Arc<Mutex<tokio::io::WriteHalf<UnixStream>>>,
        protocol: Arc<SshAgentProtocol>,
    ) {
        // Initialize connection
        connections.lock().await.insert(
            connection_id,
            ConnectionData {
                state: ConnectionState::Idle,
                queued_data: Vec::new(),
                memory: String::new(),
                write_half: write_half.clone(),
            },
        );

        // Call LLM with connection opened event
        let event = Event::new(
            &SSH_AGENT_CONNECTION_OPENED_EVENT,
            serde_json::json!({
                "connection_id": connection_id.to_string(),
            }),
        );

        match call_llm(
            &llm_client,
            &app_state,
            server_id,
            Some(connection_id),
            &event,
            protocol.as_ref(),
        )
        .await
        {
            Ok(execution_result) => {
                // Display messages
                for msg in execution_result.messages {
                    let _ = status_tx.send(msg);
                }

                // Execute protocol actions
                for action in execution_result.protocol_results {
                    if let Err(e) = Self::execute_action_result(
                        action,
                        connection_id,
                        server_id,
                        &connections,
                        &app_state,
                        &status_tx,
                    )
                    .await
                    {
                        error!("Failed to execute action: {}", e);
                    }
                }
            }
            Err(e) => {
                error!("LLM error on connection opened: {}", e);
            }
        }
    }

    /// Handle incoming data with LLM actions
    async fn handle_data_with_actions(
        connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        data: Bytes,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
        protocol: Arc<SshAgentProtocol>,
    ) {
        // Check connection state
        let mut conns = connections.lock().await;
        let conn_data = match conns.get_mut(&connection_id) {
            Some(cd) => cd,
            None => return,
        };

        match conn_data.state {
            ConnectionState::Idle => {
                // Process immediately
                conn_data.state = ConnectionState::Processing;
                let current_memory = conn_data.memory.clone();
                drop(conns);

                // Parse and handle message
                Box::pin(Self::process_message_with_llm(
                    connection_id,
                    server_id,
                    data.to_vec(),
                    llm_client,
                    app_state,
                    status_tx,
                    connections.clone(),
                    protocol,
                    current_memory,
                ))
                .await;
            }
            ConnectionState::Processing => {
                // Queue data
                conn_data.queued_data.extend_from_slice(&data);
                conn_data.state = ConnectionState::Accumulating;
                debug!(
                    "SSH Agent: Queuing {} bytes (total queued: {})",
                    data.len(),
                    conn_data.queued_data.len()
                );
            }
            ConnectionState::Accumulating => {
                // Continue queuing
                conn_data.queued_data.extend_from_slice(&data);
                debug!(
                    "SSH Agent: Queuing {} bytes (total queued: {})",
                    data.len(),
                    conn_data.queued_data.len()
                );
            }
        }
    }

    /// Process SSH Agent message and call LLM
    async fn process_message_with_llm(
        connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        data: Vec<u8>,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
        protocol: Arc<SshAgentProtocol>,
        _current_memory: String,
    ) {
        // Parse SSH Agent message
        let event = match Self::parse_message(&data) {
            Ok(Some(event)) => event,
            Ok(None) => {
                // Unknown or unsupported message type - send failure
                Self::send_failure(connection_id, &connections).await;
                Self::check_queued_data(
                    connection_id,
                    server_id,
                    llm_client,
                    app_state,
                    status_tx,
                    connections,
                    protocol,
                )
                .await;
                return;
            }
            Err(e) => {
                error!("Failed to parse SSH Agent message: {}", e);
                Self::send_failure(connection_id, &connections).await;
                Self::check_queued_data(
                    connection_id,
                    server_id,
                    llm_client,
                    app_state,
                    status_tx,
                    connections,
                    protocol,
                )
                .await;
                return;
            }
        };

        // Call LLM
        match call_llm(
            &llm_client,
            &app_state,
            server_id,
            Some(connection_id),
            &event,
            protocol.as_ref(),
        )
        .await
        {
            Ok(execution_result) => {
                // Send messages to TUI
                for msg in execution_result.messages {
                    let _ = status_tx.send(msg);
                }

                // Execute actions
                for action in execution_result.protocol_results {
                    if let Err(e) = Self::execute_action_result(
                        action,
                        connection_id,
                        server_id,
                        &connections,
                        &app_state,
                        &status_tx,
                    )
                    .await
                    {
                        error!("Failed to execute action: {}", e);
                    }
                }
            }
            Err(e) => {
                error!("LLM error: {}", e);
                Self::send_failure(connection_id, &connections).await;
            }
        }

        // Check for queued data
        Self::check_queued_data(
            connection_id,
            server_id,
            llm_client,
            app_state,
            status_tx,
            connections,
            protocol,
        )
        .await;
    }

    /// Parse SSH Agent protocol message
    fn parse_message(data: &[u8]) -> Result<Option<Event>> {
        if data.len() < 5 {
            return Ok(None);
        }

        // SSH Agent wire format: [uint32: length][byte: type][data...]
        // Skip the 4-byte length prefix
        let _length = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        let msg_type = data[4];
        let mut cursor = &data[5..];

        debug!("SSH Agent: parsing message type {} from {} bytes", msg_type, data.len());

        match msg_type {
            SSH_AGENTC_REQUEST_IDENTITIES => Ok(Some(Event::new(
                &SSH_AGENT_REQUEST_IDENTITIES_EVENT,
                serde_json::json!({}),
            ))),
            SSH_AGENTC_SIGN_REQUEST => {
                // Parse: string public_key_blob, string data, uint32 flags
                let public_key_blob = Self::read_string(&mut cursor)?;
                let data_to_sign = Self::read_string(&mut cursor)?;
                let flags = Self::read_uint32(&mut cursor)?;

                Ok(Some(Event::new(
                    &SSH_AGENT_SIGN_REQUEST_EVENT,
                    serde_json::json!({
                        "public_key_blob_hex": hex::encode(public_key_blob),
                        "data_hex": hex::encode(data_to_sign),
                        "flags": flags,
                    }),
                )))
            }
            SSH_AGENTC_ADD_IDENTITY | SSH_AGENTC_ADD_ID_CONSTRAINED => {
                // Parse: string key_type, (key-specific data), string comment
                let key_type = Self::read_string(&mut cursor)?;
                let key_type_str = String::from_utf8_lossy(key_type);

                // For simplicity, we'll just extract key type and skip to comment
                // In a full implementation, we'd parse the key-specific data
                let public_key_blob = Self::read_string(&mut cursor)?;
                let _private_key_blob = Self::read_string(&mut cursor)?; // Skip private key
                let comment = Self::read_string(&mut cursor)?;

                Ok(Some(Event::new(
                    &SSH_AGENT_ADD_IDENTITY_EVENT,
                    serde_json::json!({
                        "key_type": key_type_str,
                        "public_key_blob_hex": hex::encode(public_key_blob),
                        "comment": String::from_utf8_lossy(comment),
                        "constrained": msg_type == SSH_AGENTC_ADD_ID_CONSTRAINED,
                    }),
                )))
            }
            SSH_AGENTC_REMOVE_IDENTITY => {
                let public_key_blob = Self::read_string(&mut cursor)?;

                Ok(Some(Event::new(
                    &SSH_AGENT_REMOVE_IDENTITY_EVENT,
                    serde_json::json!({
                        "public_key_blob_hex": hex::encode(public_key_blob),
                    }),
                )))
            }
            SSH_AGENTC_REMOVE_ALL_IDENTITIES => Ok(Some(Event::new(
                &SSH_AGENT_REMOVE_ALL_IDENTITIES_EVENT,
                serde_json::json!({}),
            ))),
            SSH_AGENTC_LOCK => Ok(Some(Event::new(
                &SSH_AGENT_LOCK_EVENT,
                serde_json::json!({}),
            ))),
            SSH_AGENTC_UNLOCK => Ok(Some(Event::new(
                &SSH_AGENT_UNLOCK_EVENT,
                serde_json::json!({}),
            ))),
            _ => {
                debug!("Unknown SSH Agent message type: {}", msg_type);
                Ok(None)
            }
        }
    }

    /// Read SSH wire format string (uint32 length + bytes)
    fn read_string<'a>(cursor: &mut &'a [u8]) -> Result<&'a [u8]> {
        if cursor.len() < 4 {
            anyhow::bail!("Not enough data to read string length");
        }
        let len = u32::from_be_bytes([cursor[0], cursor[1], cursor[2], cursor[3]]) as usize;
        *cursor = &cursor[4..];

        if cursor.len() < len {
            anyhow::bail!("Not enough data to read string of length {}", len);
        }
        let result = &cursor[..len];
        *cursor = &cursor[len..];
        Ok(result)
    }

    /// Read SSH wire format uint32
    fn read_uint32(cursor: &mut &[u8]) -> Result<u32> {
        if cursor.len() < 4 {
            anyhow::bail!("Not enough data to read uint32");
        }
        let result = u32::from_be_bytes([cursor[0], cursor[1], cursor[2], cursor[3]]);
        *cursor = &cursor[4..];
        Ok(result)
    }

    /// Execute action result
    async fn execute_action_result(
        action: ActionResult,
        connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        connections: &Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        match action {
            ActionResult::Custom { name, data } => match name.as_str() {
                "send_identities_list" => {
                    let identities = data["identities"]
                        .as_array()
                        .context("Missing 'identities' field")?;
                    Self::send_identities_list(connection_id, identities, connections).await;
                }
                "send_sign_response" => {
                    let signature_hex = data["signature_hex"]
                        .as_str()
                        .context("Missing 'signature_hex' field")?;
                    Self::send_sign_response(connection_id, signature_hex, connections).await;
                }
                "send_success" => {
                    Self::send_success(connection_id, connections).await;
                }
                "send_failure" => {
                    Self::send_failure(connection_id, connections).await;
                }
                _ => {
                    debug!("Unknown custom action: {}", name);
                }
            },
            ActionResult::CloseConnection => {
                connections.lock().await.remove(&connection_id);
                app_state
                    .close_connection_on_server(server_id, connection_id)
                    .await;
                let _ = status_tx.send(format!("✗ SSH Agent connection {} closed", connection_id));
                let _ = status_tx.send("__UPDATE_UI__".to_string());
            }
            _ => {}
        }
        Ok(())
    }

    /// Send SSH_AGENT_IDENTITIES_ANSWER
    async fn send_identities_list(
        connection_id: ConnectionId,
        identities: &Vec<serde_json::Value>,
        connections: &Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
    ) {
        let mut response = BytesMut::new();
        response.put_u8(SSH_AGENT_IDENTITIES_ANSWER);
        response.put_u32(identities.len() as u32);

        for identity in identities {
            let key_blob_hex = identity["public_key_blob_hex"].as_str().unwrap_or("");
            let comment = identity["comment"].as_str().unwrap_or("");

            let key_blob = hex::decode(key_blob_hex).unwrap_or_default();

            // Write key blob (string)
            response.put_u32(key_blob.len() as u32);
            response.put_slice(&key_blob);

            // Write comment (string)
            response.put_u32(comment.len() as u32);
            response.put_slice(comment.as_bytes());
        }

        Self::send_response(connection_id, response.freeze(), connections).await;
    }

    /// Send SSH_AGENT_SIGN_RESPONSE
    async fn send_sign_response(
        connection_id: ConnectionId,
        signature_hex: &str,
        connections: &Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
    ) {
        let signature_blob = hex::decode(signature_hex).unwrap_or_default();

        let mut response = BytesMut::new();
        response.put_u8(SSH_AGENT_SIGN_RESPONSE);
        response.put_u32(signature_blob.len() as u32);
        response.put_slice(&signature_blob);

        Self::send_response(connection_id, response.freeze(), connections).await;
    }

    /// Send SSH_AGENT_SUCCESS
    async fn send_success(
        connection_id: ConnectionId,
        connections: &Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
    ) {
        let mut response = BytesMut::new();
        response.put_u8(SSH_AGENT_SUCCESS);
        Self::send_response(connection_id, response.freeze(), connections).await;
    }

    /// Send SSH_AGENT_FAILURE
    async fn send_failure(
        connection_id: ConnectionId,
        connections: &Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
    ) {
        let mut response = BytesMut::new();
        response.put_u8(SSH_AGENT_FAILURE);
        Self::send_response(connection_id, response.freeze(), connections).await;
    }

    /// Send response with SSH wire format (length prefix + data)
    async fn send_response(
        connection_id: ConnectionId,
        data: Bytes,
        connections: &Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
    ) {
        let conns = connections.lock().await;
        if let Some(conn_data) = conns.get(&connection_id) {
            // Prepend length
            let mut message = BytesMut::new();
            message.put_u32(data.len() as u32);
            message.put_slice(&data);

            if let Ok(mut write_half) = conn_data.write_half.try_lock() {
                if let Err(e) = write_half.write_all(&message).await {
                    error!("Failed to write SSH Agent response: {}", e);
                }
            }
        }
    }

    /// Check for queued data and process if present
    async fn check_queued_data(
        connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
        protocol: Arc<SshAgentProtocol>,
    ) {
        let mut conns = connections.lock().await;
        if let Some(conn_data) = conns.get_mut(&connection_id) {
            if conn_data.state == ConnectionState::Accumulating {
                let queued = std::mem::take(&mut conn_data.queued_data);
                conn_data.state = ConnectionState::Processing;
                let current_memory = conn_data.memory.clone();
                drop(conns);

                Box::pin(Self::process_message_with_llm(
                    connection_id,
                    server_id,
                    queued,
                    llm_client,
                    app_state,
                    status_tx,
                    connections,
                    protocol,
                    current_memory,
                ))
                .await;
            } else {
                conn_data.state = ConnectionState::Idle;
            }
        }
    }
}
