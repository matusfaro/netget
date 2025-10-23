//! TCP server implementation

use anyhow::{Context, Result};
use bytes::Bytes;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};

use super::connection::ConnectionId;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::prompt::PromptBuilder;
use crate::llm::response_handler;
use crate::llm::{ActionResponse, execute_actions, NetworkContext, ActionResult, ProtocolActions};
use crate::network::TcpProtocol;
use crate::state::app_state::AppState;

/// Get LLM context and output format instructions for TCP stack
pub fn get_llm_protocol_prompt() -> (&'static str, &'static str) {
    let context = r#"You are handling raw TCP data. Common protocols over TCP:
- FTP (port 21): Commands like USER, PASS, LIST, RETR, QUIT with numeric response codes
- HTTP (port 80/8080): GET/POST requests with status codes and headers
- SMTP (port 25): Mail commands with numeric codes
- Custom text protocols"#;

    let output_format = r#"IMPORTANT: Respond with a JSON object:
{
  "output": "data to send back (null if no response needed)",
  "close_connection": false,  // Close this connection after sending
  "wait_for_more": false,  // Wait for more data before responding
  "shutdown_server": false,  // Shutdown the entire server
  "message": null,  // Optional message to show user
  "set_memory": null,  // Replace memory
  "append_memory": null  // Append to memory
}

Examples:
- FTP greeting: {"output": "220 Welcome to FTP Server\r\n"}
- HTTP response: {"output": "HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nHello"}
- Wait for more: {"wait_for_more": true, "append_memory": "Partial: GET /pa"}
- Close after response: {"output": "221 Goodbye\r\n", "close_connection": true}"#;

    (context, output_format)
}

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
    /// Handle new connection with LLM
    async fn handle_connection(
        connection_id: ConnectionId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        send_first: bool,
        connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
        write_half: Arc<Mutex<tokio::io::WriteHalf<TcpStream>>>,
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
            let model = app_state.get_ollama_model().await;
            let prompt_config = get_llm_protocol_prompt();
            let event_description = format!("New connection opened: {}", connection_id);

            let prompt = PromptBuilder::build_network_event_prompt(
                &app_state,
                connection_id,
                &event_description,
                prompt_config,
            ).await;

            match llm_client.generate_llm_response(&model, &prompt).await {
                Ok(response) => {
                    let processed = response_handler::handle_llm_response(response, &app_state).await;

                    if let Some(output) = processed.output {
                        let output_bytes = output.as_bytes();
                        let mut write = write_half.lock().await;
                        if let Err(e) = write.write_all(output_bytes).await {
                            error!("Failed to send banner: {}", e);
                        } else {
                            // DEBUG: Log summary with data preview
                            if output_bytes.iter().all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace()) {
                                let preview = if output.len() > 100 {
                                    format!("{}...", &output[..100])
                                } else {
                                    output.clone()
                                };
                                debug!("TCP sent {} bytes to {}: {}", output_bytes.len(), connection_id, preview);
                                let _ = status_tx.send(format!("[DEBUG] TCP sent {} bytes to {}: {}", output_bytes.len(), connection_id, preview));

                                // TRACE: Log full text payload
                                trace!("TCP sent (text): {:?}", output);
                                let _ = status_tx.send(format!("[TRACE] TCP sent (text): {:?}", output));
                            } else {
                                debug!("TCP sent {} bytes to {} (binary data)", output_bytes.len(), connection_id);
                                let _ = status_tx.send(format!("[DEBUG] TCP sent {} bytes to {} (binary data)", output_bytes.len(), connection_id));

                                // TRACE: Log full hex payload
                                let hex_str = hex::encode(output_bytes);
                                trace!("TCP sent (hex): {}", hex_str);
                                let _ = status_tx.send(format!("[TRACE] TCP sent (hex): {}", hex_str));
                            }
                            let _ = status_tx.send(format!("→ Sent banner to {}", connection_id));
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error generating banner: {}", e);
                }
            }
        }
    }

    /// Handle data received on a connection with LLM
    async fn handle_data(
        connection_id: ConnectionId,
        data: Bytes,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
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
            // Get model and memory
            let model = app_state.get_ollama_model().await;
            let memory = {
                let conns = connections.lock().await;
                conns.get(&connection_id).map(|c| c.memory.clone()).unwrap_or_default()
            };

            // Build event description
            let prompt_config = get_llm_protocol_prompt();
            let event_description = {
                let data_preview = if all_data.len() > 200 {
                    format!("{} bytes (preview: {:?}...)", all_data.len(), &all_data[..200])
                } else {
                    format!("{:?}", all_data)
                };
                format!("Data received on connection {}: {}", connection_id, data_preview)
            };

            // Build prompt and call LLM
            let prompt = PromptBuilder::build_network_event_prompt(
                &app_state,
                connection_id,
                &event_description,
                prompt_config,
            ).await;

            match llm_client.generate_llm_response(&model, &prompt).await {
                Ok(response) => {
                    // Handle common actions
                    let processed = response_handler::handle_llm_response(response, &app_state).await;

                    // Update memory in the map
                    connections.lock().await
                        .entry(connection_id)
                        .and_modify(|conn| conn.memory = memory.clone());

                    // Handle wait_for_more
                    if processed.wait_for_more {
                        connections.lock().await
                            .entry(connection_id)
                            .and_modify(|conn| conn.state = ConnectionState::Accumulating);
                        let _ = status_tx.send(format!("⏳ Waiting for more data from {}", connection_id));
                        return;
                    }

                    // Send output if present
                    if let Some(output) = processed.output {
                        let write_half = {
                            let conns = connections.lock().await;
                            conns.get(&connection_id).map(|c| c.write_half.clone())
                        };

                        if let Some(write_half) = write_half {
                            let output_bytes = output.as_bytes();
                            let mut write = write_half.lock().await;
                            if let Err(e) = write.write_all(output_bytes).await {
                                error!("Failed to send response: {}", e);
                            } else {
                                // DEBUG: Log summary to both file and TUI
                                debug!("TCP sent {} bytes to {}", output_bytes.len(), connection_id);
                                let _ = status_tx.send(format!("[DEBUG] TCP sent {} bytes to {}", output_bytes.len(), connection_id));

                                // TRACE: Log full payload
                                if output_bytes.iter().all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace()) {
                                    trace!("TCP sent (text): {:?}", output);
                                    let _ = status_tx.send(format!("[TRACE] TCP sent (text): {:?}", output));
                                } else {
                                    let hex_str = hex::encode(output_bytes);
                                    trace!("TCP sent (hex): {}", hex_str);
                                    let _ = status_tx.send(format!("[TRACE] TCP sent (hex): {}", hex_str));
                                }
                                let _ = status_tx.send(format!("→ Sent {} bytes to {}", output.len(), connection_id));
                            }
                        }
                    }

                    // Handle close_connection
                    if processed.close_connection {
                        connections.lock().await.remove(&connection_id);
                        let _ = status_tx.send(format!("✗ Closed connection {}", connection_id));
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
                        let _ = status_tx.send(format!("▶ Processing queued data for {}", connection_id));
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
                    error!("LLM error: {}", e);
                    connections.lock().await
                        .entry(connection_id)
                        .and_modify(|conn| conn.state = ConnectionState::Idle);
                    return;
                }
            }
        }
    }

    /// Spawn the TCP server with integrated LLM handling
    pub async fn spawn_with_llm(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        send_first: bool,
    ) -> Result<SocketAddr> {
        // Create and bind TCP server
        let listener = TcpListener::bind(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("TCP server listening on {}", local_addr);

        let connections = Arc::new(Mutex::new(HashMap::new()));

        // Spawn accept loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new();
                        info!("Accepted connection {} from {}", connection_id, remote_addr);

                        // Split stream
                        let (read_half, write_half) = tokio::io::split(stream);
                        let write_half_arc = Arc::new(Mutex::new(write_half));

                        // Handle connection (send data first if needed)
                        let llm_client_clone = llm_client.clone();
                        let app_state_clone = app_state.clone();
                        let status_tx_clone = status_tx.clone();
                        let connections_clone = connections.clone();
                        let write_half_for_conn = write_half_arc.clone();
                        tokio::spawn(async move {
                            Self::handle_connection(
                                connection_id,
                                llm_client_clone,
                                app_state_clone,
                                status_tx_clone,
                                send_first,
                                connections_clone,
                                write_half_for_conn,
                            ).await;
                        });

                        // Spawn reader task
                        let llm_client_clone = llm_client.clone();
                        let app_state_clone = app_state.clone();
                        let status_tx_clone = status_tx.clone();
                        let connections_clone = connections.clone();
                        tokio::spawn(async move {
                            let mut buffer = vec![0u8; 8192];
                            let mut read_half = read_half;

                            loop {
                                match read_half.read(&mut buffer).await {
                                    Ok(0) => {
                                        // Connection closed
                                        connections_clone.lock().await.remove(&connection_id);
                                        let _ = status_tx_clone.send(format!("✗ Connection {} closed", connection_id));
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
                                        tokio::spawn(async move {
                                            Self::handle_data(
                                                connection_id,
                                                data,
                                                llm_clone,
                                                state_clone,
                                                status_clone,
                                                conns_clone,
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

    /// Spawn the TCP server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        send_first: bool,
    ) -> Result<SocketAddr> {
        // Create and bind TCP server
        let listener = TcpListener::bind(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("TCP server (action-based) listening on {}", local_addr);

        let connections = Arc::new(Mutex::new(HashMap::new()));
        let protocol = Arc::new(TcpProtocol::new());

        // Spawn accept loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new();
                        info!("Accepted connection {} from {}", connection_id, remote_addr);

                        // Split stream
                        let (read_half, write_half) = tokio::io::split(stream);
                        let write_half_arc = Arc::new(Mutex::new(write_half));

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
                                        let _ = status_tx_clone.send(format!("✗ Connection {} closed", connection_id));
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
    async fn handle_connection_with_actions(
        connection_id: ConnectionId,
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
            let model = app_state.get_ollama_model().await;
            let event_description = format!("New connection opened: {}", connection_id);

            // Create network context
            let context = NetworkContext::TcpConnection {
                connection_id,
                write_half: write_half.clone(),
                status_tx: status_tx.clone(),
            };

            // Get protocol sync actions
            let protocol_actions = protocol.get_sync_actions(&context);

            // Build prompt
            let prompt = PromptBuilder::build_network_event_action_prompt(
                &app_state,
                &event_description,
                protocol_actions,
            ).await;

            // Call LLM
            match llm_client.generate(&model, &prompt).await {
                Ok(llm_output) => {
                    debug!("LLM TCP banner response: {}", llm_output);

                    // Parse action response
                    match ActionResponse::from_str(&llm_output) {
                        Ok(action_response) => {
                            // Execute actions
                            match execute_actions(
                                action_response.actions,
                                &app_state,
                                Some(protocol.as_ref()),
                                Some(&context),
                            ).await {
                                Ok(result) => {
                                    // Display messages
                                    for msg in result.messages {
                                        let _ = status_tx.send(msg);
                                    }

                                    // Handle protocol results (send banner)
                                    for protocol_result in result.protocol_results {
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
                                                        debug!("TCP sent {} bytes to {}: {}", output_data.len(), connection_id, preview);
                                                        let _ = status_tx.send(format!("[DEBUG] TCP sent {} bytes to {}: {}", output_data.len(), connection_id, preview));

                                                        // TRACE: Log full text payload
                                                        trace!("TCP sent (text): {:?}", data_str);
                                                        let _ = status_tx.send(format!("[TRACE] TCP sent (text): {:?}", data_str));
                                                    } else {
                                                        debug!("TCP sent {} bytes to {} (binary data)", output_data.len(), connection_id);
                                                        let _ = status_tx.send(format!("[DEBUG] TCP sent {} bytes to {} (binary data)", output_data.len(), connection_id));

                                                        // TRACE: Log full hex payload
                                                        let hex_str = hex::encode(&output_data);
                                                        trace!("TCP sent (hex): {}", hex_str);
                                                        let _ = status_tx.send(format!("[TRACE] TCP sent (hex): {}", hex_str));
                                                    }
                                                    let _ = status_tx.send(format!("→ Sent banner to {}", connection_id));
                                                }
                                            }
                                            ActionResult::CloseConnection => {
                                                connections.lock().await.remove(&connection_id);
                                                let _ = status_tx.send(format!("✗ Closed connection {} after banner", connection_id));
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to execute actions: {}", e);
                                    let _ = status_tx.send(format!("✗ Action execution error: {}", e));
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse action response: {}", e);
                            let _ = status_tx.send(format!("✗ Parse error: {}", e));
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error generating banner: {}", e);
                    let _ = status_tx.send(format!("✗ LLM error: {}", e));
                }
            }
        }
    }

    /// Handle data received on a connection with LLM actions
    async fn handle_data_with_actions(
        connection_id: ConnectionId,
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
            // Get model and memory
            let model = app_state.get_ollama_model().await;
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

            // Build event description
            let event_description = {
                let data_preview = if all_data.len() > 200 {
                    format!("{} bytes (preview: {:?}...)", all_data.len(), &all_data[..200])
                } else {
                    format!("{:?}", all_data)
                };
                format!("Data received on connection {}: {}", connection_id, data_preview)
            };

            // Create network context
            let context = NetworkContext::TcpConnection {
                connection_id,
                write_half: write_half.clone(),
                status_tx: status_tx.clone(),
            };

            // Get protocol sync actions
            let protocol_actions = protocol.get_sync_actions(&context);

            // Build prompt and call LLM
            let prompt = PromptBuilder::build_network_event_action_prompt(
                &app_state,
                &event_description,
                protocol_actions,
            ).await;

            match llm_client.generate(&model, &prompt).await {
                Ok(llm_output) => {
                    debug!("LLM TCP response: {}", llm_output);

                    // Parse action response
                    match ActionResponse::from_str(&llm_output) {
                        Ok(action_response) => {
                            // Execute actions
                            match execute_actions(
                                action_response.actions,
                                &app_state,
                                Some(protocol.as_ref()),
                                Some(&context),
                            ).await {
                                Ok(result) => {
                                    // Update memory
                                    connections.lock().await
                                        .entry(connection_id)
                                        .and_modify(|conn| conn.memory = memory.clone());

                                    // Display messages
                                    for msg in result.messages {
                                        let _ = status_tx.send(msg);
                                    }

                                    // Handle protocol results
                                    let mut should_close = false;
                                    let mut should_wait = false;

                                    for protocol_result in result.protocol_results {
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
                                                        debug!("TCP sent {} bytes to {}: {}", output_data.len(), connection_id, preview);
                                                        let _ = status_tx.send(format!("[DEBUG] TCP sent {} bytes to {}: {}", output_data.len(), connection_id, preview));

                                                        // TRACE: Log full text payload
                                                        trace!("TCP sent (text): {:?}", data_str);
                                                        let _ = status_tx.send(format!("[TRACE] TCP sent (text): {:?}", data_str));
                                                    } else {
                                                        debug!("TCP sent {} bytes to {} (binary data)", output_data.len(), connection_id);
                                                        let _ = status_tx.send(format!("[DEBUG] TCP sent {} bytes to {} (binary data)", output_data.len(), connection_id));

                                                        // TRACE: Log full hex payload
                                                        let hex_str = hex::encode(&output_data);
                                                        trace!("TCP sent (hex): {}", hex_str);
                                                        let _ = status_tx.send(format!("[TRACE] TCP sent (hex): {}", hex_str));
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
                                        let _ = status_tx.send(format!("⏳ Waiting for more data from {}", connection_id));
                                        return;
                                    }

                                    // Handle close_connection
                                    if should_close {
                                        connections.lock().await.remove(&connection_id);
                                        let _ = status_tx.send(format!("✗ Closed connection {}", connection_id));
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
                                        let _ = status_tx.send(format!("▶ Processing queued data for {}", connection_id));
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
                                    error!("Failed to execute actions: {}", e);
                                    let _ = status_tx.send(format!("✗ Action execution error: {}", e));
                                    connections.lock().await
                                        .entry(connection_id)
                                        .and_modify(|conn| conn.state = ConnectionState::Idle);
                                    return;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse action response: {}", e);
                            let _ = status_tx.send(format!("✗ Parse error: {}", e));
                            connections.lock().await
                                .entry(connection_id)
                                .and_modify(|conn| conn.state = ConnectionState::Idle);
                            return;
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error: {}", e);
                    let _ = status_tx.send(format!("✗ LLM error: {}", e));
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
