//! IGMP client implementation for multicast group management
pub mod actions;

pub use actions::IgmpClientProtocol;

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, trace, warn};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::{Client, ClientActionResult};
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::igmp::actions::{IGMP_CLIENT_CONNECTED_EVENT, IGMP_CLIENT_DATA_RECEIVED_EVENT};
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum ClientState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-client data for LLM handling
struct IgmpClientData {
    state: ClientState,
    queued_data: Vec<(Vec<u8>, SocketAddr)>,
    memory: String,
    joined_groups: HashSet<Ipv4Addr>,
}

/// IGMP client for multicast group management
pub struct IgmpClient;

impl IgmpClient {
    /// Connect (bind) IGMP client with integrated LLM actions
    pub async fn connect_with_llm_actions(
        bind_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Parse bind address - use 0.0.0.0:0 if not specified
        let socket_addr: SocketAddr = if bind_addr.is_empty() || bind_addr == "igmp" {
            "0.0.0.0:0".parse()?
        } else {
            bind_addr.parse().context("Invalid bind address")?
        };

        // Create UDP socket for multicast reception
        let socket = UdpSocket::bind(socket_addr)
            .await
            .context("Failed to bind UDP socket for IGMP client")?;

        let local_addr = socket.local_addr()?;


        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        console_info!(status_tx, "[CLIENT] IGMP client {} ready", client_id);
        console_info!(status_tx, "__UPDATE_UI__");

        // Trigger initial connected event
        let client_instance = app_state.get_client(client_id).await;
        let instruction = client_instance
            .as_ref()
            .map(|c| c.instruction.clone())
            .unwrap_or_default();

        let connected_event = Event::new(&IGMP_CLIENT_CONNECTED_EVENT, serde_json::json!({
            "local_addr": local_addr.to_string(),
        }));

        // Initial LLM call
        let protocol = Arc::new(IgmpClientProtocol::new());
        let _ = call_llm_for_client(
            &llm_client,
            &app_state,
            client_id.to_string(),
            &instruction,
            "",
            Some(&connected_event),
            protocol.as_ref(),
            &status_tx,
        )
        .await;

        // Wrap socket in Arc for shared access
        let socket_arc = Arc::new(socket);

        // Initialize client data
        let client_data = Arc::new(Mutex::new(IgmpClientData {
            state: ClientState::Idle,
            queued_data: Vec::new(),
            memory: String::new(),
            joined_groups: HashSet::new(),
        }));

        // Clone references for read loop
        let socket_clone = socket_arc.clone();
        let app_state_clone = app_state.clone();
        let status_tx_clone = status_tx.clone();
        let llm_client_clone = llm_client.clone();
        let client_data_clone = client_data.clone();

        // Spawn read loop for receiving multicast data
        tokio::spawn(async move {
            let mut buffer = vec![0u8; 65536];
            let protocol = Arc::new(IgmpClientProtocol::new());

            loop {
                match socket_clone.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        trace!("IGMP client {} received {} bytes from {}", client_id, n, peer_addr);

                        // Handle data with LLM
                        let mut client_data_lock = client_data_clone.lock().await;

                        match client_data_lock.state {
                            ClientState::Idle => {
                                // Process immediately
                                client_data_lock.state = ClientState::Processing;
                                drop(client_data_lock);

                                // Get current instruction and memory
                                let instruction = app_state_clone.get_instruction_for_client(client_id).await;

                                if let Some(instruction) = instruction {
                                    let memory = {
                                        let data_lock = client_data_clone.lock().await;
                                        data_lock.memory.clone()
                                    };

                                    // Create event
                                    let event = Event::new(&IGMP_CLIENT_DATA_RECEIVED_EVENT, serde_json::json!({
                                        "data_hex": hex::encode(&data),
                                        "data_length": n,
                                        "source_addr": peer_addr.to_string(),
                                    }));

                                    // Call LLM
                                    match call_llm_for_client(
                                        &llm_client_clone,
                                        &app_state_clone,
                                        client_id.to_string(),
                                        &instruction,
                                        &memory,
                                        Some(&event),
                                        protocol.as_ref(),
                                        &status_tx_clone,
                                    )
                                    .await {
                                        Ok(ClientLlmResult { actions, memory_updates }) => {
                                            // Update memory
                                            if let Some(mem) = memory_updates {
                                                client_data_clone.lock().await.memory = mem;
                                            }

                                            // Execute actions
                                            for action in actions {
                                                match protocol.as_ref().execute_action(action) {
                                                    Ok(ClientActionResult::Custom { name, data }) => {
                                                        if name == "join_multicast_group" {
                                                            if let (Some(mcast), Some(iface)) = (
                                                                data["multicast_addr"].as_str(),
                                                                data["interface_addr"].as_str(),
                                                            ) {
                                                                if let Ok(mcast_ip) = mcast.parse::<Ipv4Addr>() {
                                                                    if let Ok(iface_ip) = iface.parse::<Ipv4Addr>() {
                                                                        // Use socket2 to join multicast group
                                                                        let socket2 = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
                                                                            .context("Failed to create socket2");

                                                                        if let Ok(sock) = socket2 {
                                                                            if let Err(e) = sock.join_multicast_v4(&mcast_ip, &iface_ip) {
                                                                                error!("Failed to join multicast group {}: {}", mcast_ip, e);
                                                                                let _ = status_tx_clone.send(format!("[CLIENT] Failed to join multicast group {}: {}", mcast_ip, e));
                                                                            } else {
                                                                                info!("IGMP client {} joined multicast group {}", client_id, mcast_ip);
                                                                                client_data_clone.lock().await.joined_groups.insert(mcast_ip);
                                                                                let _ = status_tx_clone.send(format!("[CLIENT] Joined multicast group {}", mcast_ip));
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        } else if name == "leave_multicast_group" {
                                                            if let (Some(mcast), Some(iface)) = (
                                                                data["multicast_addr"].as_str(),
                                                                data["interface_addr"].as_str(),
                                                            ) {
                                                                if let Ok(mcast_ip) = mcast.parse::<Ipv4Addr>() {
                                                                    if let Ok(iface_ip) = iface.parse::<Ipv4Addr>() {
                                                                        // Use socket2 to leave multicast group
                                                                        let socket2 = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
                                                                            .context("Failed to create socket2");

                                                                        if let Ok(sock) = socket2 {
                                                                            if let Err(e) = sock.leave_multicast_v4(&mcast_ip, &iface_ip) {
                                                                                error!("Failed to leave multicast group {}: {}", mcast_ip, e);
                                                                                let _ = status_tx_clone.send(format!("[CLIENT] Failed to leave multicast group {}: {}", mcast_ip, e));
                                                                            } else {
                                                                                info!("IGMP client {} left multicast group {}", client_id, mcast_ip);
                                                                                client_data_clone.lock().await.joined_groups.remove(&mcast_ip);
                                                                                let _ = status_tx_clone.send(format!("[CLIENT] Left multicast group {}", mcast_ip));
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        } else if name == "send_multicast" {
                                                            if let (Some(mcast), Some(port), Some(data_vec)) = (
                                                                data["multicast_addr"].as_str(),
                                                                data["port"].as_u64(),
                                                                data["data"].as_array(),
                                                            ) {
                                                                let bytes: Vec<u8> = data_vec.iter()
                                                                    .filter_map(|v| v.as_u64().map(|n| n as u8))
                                                                    .collect();

                                                                let dest = format!("{}:{}", mcast, port);
                                                                if let Ok(dest_addr) = dest.parse::<SocketAddr>() {
                                                                    match socket_clone.send_to(&bytes, dest_addr).await {
                                                                        Ok(n) => {
                                                                            trace!("IGMP client {} sent {} bytes to {}", client_id, n, dest_addr);
                                                                            let _ = status_tx_clone.send(format!("[CLIENT] Sent {} bytes to multicast {}", n, dest_addr));
                                                                        }
                                                                        Err(e) => {
                                                                            error!("Failed to send multicast: {}", e);
                                                                            let _ = status_tx_clone.send(format!("[CLIENT] Failed to send multicast: {}", e));
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                    Ok(ClientActionResult::WaitForMore) => {
                                                        trace!("IGMP client {} waiting for more data", client_id);
                                                    }
                                                    Err(e) => {
                                                        warn!("Action execution error for IGMP client {}: {}", client_id, e);
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            error!("LLM error for IGMP client {}: {}", client_id, e);
                                        }
                                    }
                                }

                                // Process queued data if any
                                let mut client_data_lock = client_data_clone.lock().await;
                                if !client_data_lock.queued_data.is_empty() {
                                    client_data_lock.queued_data.clear();
                                }
                                client_data_lock.state = ClientState::Idle;
                            }
                            ClientState::Processing => {
                                // Queue data for later processing
                                client_data_lock.queued_data.push((data, peer_addr));
                                trace!("IGMP client {} queued data (processing state)", client_id);
                            }
                            ClientState::Accumulating => {
                                // Already accumulating, just add to queue
                                client_data_lock.queued_data.push((data, peer_addr));
                                trace!("IGMP client {} queued data (accumulating state)", client_id);
                            }
                        }
                    }
                    Err(e) => {
                        error!("IGMP client {} read error: {}", client_id, e);
                        app_state_clone.update_client_status(client_id, ClientStatus::Error(e.to_string())).await;
                        let _ = status_tx_clone.send(format!("[CLIENT] IGMP client {} error: {}", client_id, e));
                        let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}
