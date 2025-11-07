//! HTTP proxy client implementation
pub mod actions;

pub use actions::HttpProxyClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::{Client, ClientActionResult};
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::http_proxy::actions::{
    HTTP_PROXY_CLIENT_CONNECTED_EVENT,
    HTTP_PROXY_TUNNEL_ESTABLISHED_EVENT,
    HTTP_PROXY_RESPONSE_RECEIVED_EVENT,
};

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-client data for LLM handling
struct ClientData {
    state: ConnectionState,
    queued_data: Vec<u8>,
    memory: String,
    tunnel_established: bool,
}

/// HTTP proxy client that connects to a proxy server
pub struct HttpProxyClient;

impl HttpProxyClient {
    /// Connect to an HTTP proxy server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Connect to the proxy server
        let stream = TcpStream::connect(&remote_addr)
            .await
            .context(format!("Failed to connect to HTTP proxy at {}", remote_addr))?;

        let local_addr = stream.local_addr()?;
        let remote_sock_addr = stream.peer_addr()?;

        info!("HTTP proxy client {} connected to proxy {} (local: {})", client_id, remote_sock_addr, local_addr);

        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] HTTP proxy client {} connected to {}", client_id, remote_sock_addr));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::http_proxy::actions::HttpProxyClientProtocol::new());
            let event = Event::new(
                &HTTP_PROXY_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "proxy_addr": remote_sock_addr.to_string(),
                }),
            );

            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                "",
                Some(&event),
                protocol.as_ref(),
                &status_tx,
            ).await {
                Ok(ClientLlmResult { actions, memory_updates }) => {
                    // Store memory
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }

                    // Execute initial actions
                    for action in actions {
                        if let Ok(result) = protocol.as_ref().execute_action(action) {
                            match result {
                                ClientActionResult::Custom { name, data } if name == "establish_tunnel" => {
                                    // Handle tunnel establishment
                                    if let (Some(target_host), Some(target_port)) = (
                                        data.get("target_host").and_then(|v| v.as_str()),
                                        data.get("target_port").and_then(|v| v.as_u64())
                                    ) {
                                        info!("HTTP proxy client {} establishing tunnel to {}:{}", client_id, target_host, target_port);
                                        // We'll establish the tunnel in the spawn task below
                                        app_state.with_client_mut(client_id, |client| {
                                            client.set_protocol_field(
                                                "tunnel_target".to_string(),
                                                serde_json::json!(format!("{}:{}", target_host, target_port)),
                                            );
                                        }).await;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error for HTTP proxy client {}: {}", client_id, e);
                }
            }
        }

        // Split stream
        let (read_half, write_half) = tokio::io::split(stream);
        let write_half_arc = Arc::new(Mutex::new(write_half));

        // Initialize client data
        let client_data = Arc::new(Mutex::new(ClientData {
            state: ConnectionState::Idle,
            queued_data: Vec::new(),
            memory: String::new(),
            tunnel_established: false,
        }));

        // Clone for spawn
        let app_state_clone = app_state.clone();
        let write_half_clone = write_half_arc.clone();
        let client_data_clone = client_data.clone();

        // Spawn task to handle tunnel establishment if needed
        tokio::spawn(async move {
            // Check if we have a tunnel target to establish
            if let Some(tunnel_target) = app_state_clone.with_client_mut(client_id, |client| {
                client.get_protocol_field("tunnel_target")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            }).await.flatten() {
                let parts: Vec<&str> = tunnel_target.split(':').collect();
                if parts.len() == 2 {
                    let target_host = parts[0];
                    let target_port = parts[1];

                    // Send CONNECT request
                    let connect_request = format!(
                        "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\n\r\n",
                        target_host, target_port, target_host, target_port
                    );

                    debug!("HTTP proxy client {} sending CONNECT request: {}", client_id, connect_request.trim());

                    if let Err(e) = write_half_clone.lock().await.write_all(connect_request.as_bytes()).await {
                        error!("HTTP proxy client {} failed to send CONNECT: {}", client_id, e);
                    }
                }
            }
        });

        // Spawn read loop
        let app_state_clone = app_state.clone();
        let status_tx_clone = status_tx.clone();
        let write_half_clone = write_half_arc.clone();

        tokio::spawn(async move {
            let mut reader = BufReader::new(read_half);

            // First, check if we need to read CONNECT response
            if let Some(target) = app_state_clone.with_client_mut(client_id, |client| {
                client.get_protocol_field("tunnel_target")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            }).await.flatten() {

                // Read CONNECT response
                let mut status_line = String::new();
                match reader.read_line(&mut status_line).await {
                    Ok(0) => {
                        error!("HTTP proxy client {} disconnected during CONNECT", client_id);
                        app_state_clone.update_client_status(client_id, ClientStatus::Error("Proxy disconnected".to_string())).await;
                        return;
                    }
                    Ok(_) => {
                        debug!("HTTP proxy client {} received status: {}", client_id, status_line.trim());

                        // Parse status code
                        let parts: Vec<&str> = status_line.split_whitespace().collect();
                        let status_code = if parts.len() >= 2 {
                            parts[1].parse::<u16>().unwrap_or(0)
                        } else {
                            0
                        };

                        // Read headers until empty line
                        loop {
                            let mut header_line = String::new();
                            match reader.read_line(&mut header_line).await {
                                Ok(0) => break,
                                Ok(_) => {
                                    if header_line.trim().is_empty() {
                                        break;
                                    }
                                }
                                Err(e) => {
                                    error!("HTTP proxy client {} error reading headers: {}", client_id, e);
                                    break;
                                }
                            }
                        }

                        if status_code == 200 {
                            info!("HTTP proxy client {} tunnel established successfully", client_id);
                            client_data_clone.lock().await.tunnel_established = true;

                            // Call LLM with tunnel established event
                            if let Some(instruction) = app_state_clone.get_instruction_for_client(client_id).await {
                                let protocol = Arc::new(crate::client::http_proxy::actions::HttpProxyClientProtocol::new());

                                let parts: Vec<&str> = target.split(':').collect();
                                let (target_host, target_port) = if parts.len() == 2 {
                                    (parts[0].to_string(), parts[1].parse::<u16>().unwrap_or(0))
                                } else {
                                    (target.clone(), 0)
                                };

                                let event = Event::new(
                                    &HTTP_PROXY_TUNNEL_ESTABLISHED_EVENT,
                                    serde_json::json!({
                                        "target_host": target_host,
                                        "target_port": target_port,
                                        "status_code": status_code,
                                    }),
                                );

                                match call_llm_for_client(
                                    &llm_client,
                                    &app_state_clone,
                                    client_id.to_string(),
                                    &instruction,
                                    &client_data_clone.lock().await.memory,
                                    Some(&event),
                                    protocol.as_ref(),
                                    &status_tx_clone,
                                ).await {
                                    Ok(ClientLlmResult { actions, memory_updates }) => {
                                        // Update memory
                                        if let Some(mem) = memory_updates {
                                            client_data_clone.lock().await.memory = mem;
                                        }

                                        // Execute actions
                                        for action in actions {
                                            if let Ok(result) = protocol.as_ref().execute_action(action) {
                                                match result {
                                                    ClientActionResult::SendData(bytes) => {
                                                        if let Ok(_) = write_half_clone.lock().await.write_all(&bytes).await {
                                                            trace!("HTTP proxy client {} sent {} bytes via tunnel", client_id, bytes.len());
                                                        }
                                                    }
                                                    ClientActionResult::Disconnect => {
                                                        info!("HTTP proxy client {} disconnecting", client_id);
                                                        return;
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("LLM error for HTTP proxy client {}: {}", client_id, e);
                                    }
                                }
                            }
                        } else {
                            error!("HTTP proxy client {} tunnel failed with status {}", client_id, status_code);
                            app_state_clone.update_client_status(client_id, ClientStatus::Error(format!("Tunnel failed: {}", status_code))).await;
                            return;
                        }
                    }
                    Err(e) => {
                        error!("HTTP proxy client {} error reading CONNECT response: {}", client_id, e);
                        app_state_clone.update_client_status(client_id, ClientStatus::Error(e.to_string())).await;
                        return;
                    }
                }
            }

            // Now read data through the tunnel
            let mut buffer = vec![0u8; 8192];

            loop {
                match reader.read(&mut buffer).await {
                    Ok(0) => {
                        info!("HTTP proxy client {} disconnected", client_id);
                        app_state_clone.update_client_status(client_id, ClientStatus::Disconnected).await;
                        let _ = status_tx_clone.send(format!("[CLIENT] HTTP proxy client {} disconnected", client_id));
                        let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        break;
                    }
                    Ok(n) => {
                        let data = buffer[..n].to_vec();
                        trace!("HTTP proxy client {} received {} bytes via tunnel", client_id, n);

                        // Handle data with LLM
                        let mut client_data_lock = client_data_clone.lock().await;

                        match client_data_lock.state {
                            ConnectionState::Idle => {
                                // Process immediately
                                client_data_lock.state = ConnectionState::Processing;
                                drop(client_data_lock);

                                // Call LLM
                                if let Some(instruction) = app_state_clone.get_instruction_for_client(client_id).await {
                                    let protocol = Arc::new(crate::client::http_proxy::actions::HttpProxyClientProtocol::new());
                                    let event = Event::new(
                                        &HTTP_PROXY_RESPONSE_RECEIVED_EVENT,
                                        serde_json::json!({
                                            "data_hex": hex::encode(&data),
                                            "data_length": data.len(),
                                        }),
                                    );

                                    match call_llm_for_client(
                                        &llm_client,
                                        &app_state_clone,
                                        client_id.to_string(),
                                        &instruction,
                                        &client_data_clone.lock().await.memory,
                                        Some(&event),
                                        protocol.as_ref(),
                                        &status_tx_clone,
                                    ).await {
                                        Ok(ClientLlmResult { actions, memory_updates }) => {
                                            // Update memory
                                            if let Some(mem) = memory_updates {
                                                client_data_clone.lock().await.memory = mem;
                                            }

                                            // Execute actions
                                            for action in actions {
                                                if let Ok(result) = protocol.as_ref().execute_action(action) {
                                                    match result {
                                                        ClientActionResult::SendData(bytes) => {
                                                            if let Ok(_) = write_half_clone.lock().await.write_all(&bytes).await {
                                                                trace!("HTTP proxy client {} sent {} bytes", client_id, bytes.len());
                                                            }
                                                        }
                                                        ClientActionResult::Disconnect => {
                                                            info!("HTTP proxy client {} disconnecting", client_id);
                                                            break;
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            error!("LLM error for HTTP proxy client {}: {}", client_id, e);
                                        }
                                    }
                                }

                                // Process queued data if any
                                let mut client_data_lock = client_data_clone.lock().await;
                                if !client_data_lock.queued_data.is_empty() {
                                    client_data_lock.queued_data.clear();
                                }
                                client_data_lock.state = ConnectionState::Idle;
                            }
                            ConnectionState::Processing => {
                                // Queue data
                                client_data_lock.queued_data.extend_from_slice(&data);
                                client_data_lock.state = ConnectionState::Accumulating;
                            }
                            ConnectionState::Accumulating => {
                                // Continue queuing
                                client_data_lock.queued_data.extend_from_slice(&data);
                            }
                        }
                    }
                    Err(e) => {
                        error!("HTTP proxy client {} read error: {}", client_id, e);
                        app_state_clone.update_client_status(client_id, ClientStatus::Error(e.to_string())).await;
                        let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}
