//! UDP client implementation
pub mod actions;

pub use actions::UdpClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, trace, warn};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::ClientActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::udp::actions::{UDP_CLIENT_CONNECTED_EVENT, UDP_CLIENT_DATAGRAM_RECEIVED_EVENT};

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
    queued_datagrams: Vec<(Vec<u8>, SocketAddr)>, // (data, source_addr)
    memory: String,
    default_target: SocketAddr,
}

/// UDP client that sends/receives datagrams
pub struct UdpClient;

impl UdpClient {
    /// Bind a UDP socket and integrate with LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Parse remote address for default target
        let default_target: SocketAddr = remote_addr.parse()
            .context(format!("Failed to parse remote address: {}", remote_addr))?;

        // Bind to local address (0.0.0.0:0 for any available port)
        let socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .context("Failed to bind UDP socket")?;

        let local_addr = socket.local_addr()?;


        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        console_info!(status_tx, "[CLIENT] UDP client {} ready", client_id);
        console_info!(status_tx, "__UPDATE_UI__");

        // Initialize client data
        let client_data = Arc::new(Mutex::new(ClientData {
            state: ConnectionState::Idle,
            queued_datagrams: Vec::new(),
            memory: String::new(),
            default_target,
        }));

        let socket_arc = Arc::new(socket);

        // Call LLM with connected event
        tokio::spawn({
            let socket_arc = socket_arc.clone();
            let client_data = client_data.clone();
            let llm_client = llm_client.clone();
            let app_state = app_state.clone();
            let status_tx = status_tx.clone();

            async move {
                // Get instruction for this client
                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                    let protocol = Arc::new(UdpClientProtocol::new());
                    let connected_event = Event::new(&UDP_CLIENT_CONNECTED_EVENT, serde_json::json!({
                        "remote_addr": default_target.to_string(),
                        "local_addr": local_addr.to_string(),
                    }));

                    // Call LLM with connected event
                    match call_llm_for_client(
                        &llm_client,
                        &app_state,
                        client_id.to_string(),
                        &instruction,
                        "",
                        Some(&connected_event),
                        protocol.as_ref(),
                        &status_tx,
                    )
                    .await
                    {
                        Ok(llm_result) => {
                            if let Err(e) = Self::handle_llm_result(
                                llm_result,
                                &socket_arc,
                                &client_data,
                                client_id,
                                &app_state,
                                &status_tx,
                            )
                            .await
                            {
                                error!("Error handling LLM result for UDP client {}: {}", client_id, e);
                            }
                        }
                        Err(e) => {
                            error!("Failed to call LLM for UDP client {} connected event: {}", client_id, e);
                        }
                    }
                }
            }
        });

        // Spawn receive loop
        tokio::spawn({
            let socket_arc = socket_arc.clone();
            let client_data = client_data.clone();

            async move {
                let mut buffer = vec![0u8; 65536]; // Max UDP datagram size

                loop {
                    match socket_arc.recv_from(&mut buffer).await {
                        Ok((n, source_addr)) => {
                            let data = buffer[..n].to_vec();
                            trace!("UDP client {} received {} bytes from {}", client_id, n, source_addr);

                            // Handle datagram with LLM
                            let mut client_data_lock = client_data.lock().await;

                            match client_data_lock.state {
                                ConnectionState::Idle => {
                                    // Process immediately
                                    client_data_lock.state = ConnectionState::Processing;
                                    drop(client_data_lock);

                                    // Process the datagram
                                    if let Err(e) = Self::process_datagram(
                                        data,
                                        source_addr,
                                        client_id,
                                        &llm_client,
                                        &app_state,
                                        &status_tx,
                                        &socket_arc,
                                        &client_data,
                                    )
                                    .await
                                    {
                                        error!("Error processing UDP datagram for client {}: {}", client_id, e);

                                        // Reset to Idle on error
                                        let mut client_data_lock = client_data.lock().await;
                                        client_data_lock.state = ConnectionState::Idle;
                                    }
                                }
                                ConnectionState::Processing => {
                                    // Queue the datagram
                                    trace!("UDP client {} is processing, queuing datagram", client_id);
                                    client_data_lock.queued_datagrams.push((data, source_addr));
                                    drop(client_data_lock);
                                }
                                ConnectionState::Accumulating => {
                                    // Accumulate the datagram
                                    trace!("UDP client {} is accumulating, adding datagram", client_id);
                                    client_data_lock.queued_datagrams.push((data, source_addr));
                                    drop(client_data_lock);
                                }
                            }
                        }
                        Err(e) => {
                            app_state.update_client_status(client_id, ClientStatus::Error(e.to_string())).await;
                            console_error!(status_tx, "[CLIENT] UDP client {} error: {}", client_id, e);
                            console_error!(status_tx, "__UPDATE_UI__");
                            break;
                        }
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Process a received datagram with the LLM
    async fn process_datagram(
        data: Vec<u8>,
        source_addr: SocketAddr,
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        socket: &Arc<UdpSocket>,
        client_data: &Arc<Mutex<ClientData>>,
    ) -> Result<()> {
        let data_hex = hex::encode(&data);
        let data_len = data.len();

        // Get instruction for this client
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(UdpClientProtocol::new());
            let event = Event::new(&UDP_CLIENT_DATAGRAM_RECEIVED_EVENT, serde_json::json!({
                "data_hex": data_hex,
                "data_length": data_len,
                "source_addr": source_addr.to_string(),
            }));

            // Get current memory
            let memory = {
                let client_data_lock = client_data.lock().await;
                client_data_lock.memory.clone()
            };

            // Call LLM
            let llm_result = call_llm_for_client(
                llm_client,
                app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                protocol.as_ref(),
                status_tx,
            )
            .await?;

            // Handle LLM result
            Self::handle_llm_result(llm_result, socket, client_data, client_id, app_state, status_tx).await?;
        }

        Ok(())
    }

    /// Handle LLM result and execute actions
    async fn handle_llm_result(
        llm_result: ClientLlmResult,
        socket: &Arc<UdpSocket>,
        client_data: &Arc<Mutex<ClientData>>,
        client_id: ClientId,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Update memory if needed
        if let Some(new_memory) = llm_result.memory_updates {
            let mut client_data_lock = client_data.lock().await;
            client_data_lock.memory = new_memory;
        }

        // Execute actions
        let protocol = Arc::new(UdpClientProtocol::new());
        for action in llm_result.actions {
            use crate::llm::actions::client_trait::Client;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};
            let action_result = protocol.as_ref().execute_action(action)?;

            match action_result {
                ClientActionResult::SendData(_) => {
                    // Not used for UDP (we use Custom with target_addr)
                    warn!("SendData action not supported for UDP client, use send_udp_datagram");
                }
                ClientActionResult::Custom { name, data } => {
                    if name == "send_udp_datagram" {
                        let data_bytes = data["data"]
                            .as_array()
                            .context("Missing 'data' array in send_udp_datagram")?
                            .iter()
                            .map(|v| v.as_u64().unwrap_or(0) as u8)
                            .collect::<Vec<u8>>();

                        let target_addr = if let Some(target) = data["target_addr"].as_str() {
                            target.parse::<SocketAddr>()
                                .context(format!("Invalid target address: {}", target))?
                        } else {
                            // Use default target or last source
                            let client_data_lock = client_data.lock().await;
                            client_data_lock.default_target
                        };

                        // Send datagram
                        socket.send_to(&data_bytes, target_addr).await?;
                        trace!("UDP client {} sent {} bytes to {}", client_id, data_bytes.len(), target_addr);
                    } else if name == "change_target" {
                        let new_target_str = data["new_target"]
                            .as_str()
                            .context("Missing 'new_target' in change_target action")?;

                        let new_target = new_target_str.parse::<SocketAddr>()
                            .context(format!("Invalid target address: {}", new_target_str))?;

                        let mut client_data_lock = client_data.lock().await;
                        client_data_lock.default_target = new_target;
                        info!("UDP client {} changed default target to {}", client_id, new_target);
                    }
                }
                ClientActionResult::Disconnect => {
                    app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                    console_info!(status_tx, "[CLIENT] UDP client {} closed", client_id);
                    console_info!(status_tx, "__UPDATE_UI__");
                    return Ok(());
                }
                ClientActionResult::WaitForMore => {
                    // Change state to Accumulating
                    let mut client_data_lock = client_data.lock().await;
                    client_data_lock.state = ConnectionState::Accumulating;
                    trace!("UDP client {} waiting for more datagrams", client_id);
                    return Ok(());
                }
                ClientActionResult::NoAction => {
                    // No action needed
                }
                ClientActionResult::Multiple(_) => {
                    warn!("Multiple actions not yet supported in UDP client");
                }
            }
        }

        // Clear queued datagrams and return to Idle
        // (LLM has already made its decision based on the current event)
        let mut client_data_lock = client_data.lock().await;
        if !client_data_lock.queued_datagrams.is_empty() {
            trace!("UDP client {} clearing {} queued datagrams", client_id, client_data_lock.queued_datagrams.len());
            client_data_lock.queued_datagrams.clear();
        }
        client_data_lock.state = ConnectionState::Idle;

        Ok(())
    }
}
