//! RIP (Routing Information Protocol) client implementation
pub mod actions;

pub use actions::RipClientProtocol;

use anyhow::{Context, Result};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::rip::actions::{RIP_CLIENT_CONNECTED_EVENT, RIP_CLIENT_RESPONSE_RECEIVED_EVENT};

/// RIP protocol version
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum RipVersion {
    V1 = 1,
    V2 = 2,
}

/// RIP command types
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum RipCommand {
    Request = 1,
    Response = 2,
}

/// RIP route entry (24 bytes for RIPv2)
#[derive(Debug, Clone)]
pub struct RipRouteEntry {
    pub address_family: u16,
    pub route_tag: u16,        // RIPv2 only (0 for RIPv1)
    pub ip_address: Ipv4Addr,
    pub subnet_mask: Ipv4Addr, // RIPv2 only (0.0.0.0 for RIPv1)
    pub next_hop: Ipv4Addr,    // RIPv2 only (0.0.0.0 for RIPv1)
    pub metric: u32,
}

impl RipRouteEntry {
    /// Create a new RIP route entry
    pub fn new(ip: Ipv4Addr, metric: u32) -> Self {
        Self {
            address_family: 2, // AF_INET
            route_tag: 0,
            ip_address: ip,
            subnet_mask: Ipv4Addr::new(0, 0, 0, 0),
            next_hop: Ipv4Addr::new(0, 0, 0, 0),
            metric,
        }
    }

    /// Create a RIPv2 route entry with subnet mask
    pub fn new_v2(ip: Ipv4Addr, subnet_mask: Ipv4Addr, next_hop: Ipv4Addr, metric: u32) -> Self {
        Self {
            address_family: 2,
            route_tag: 0,
            ip_address: ip,
            subnet_mask,
            next_hop,
            metric,
        }
    }

    /// Encode route entry to bytes
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(20);
        bytes.extend_from_slice(&self.address_family.to_be_bytes());
        bytes.extend_from_slice(&self.route_tag.to_be_bytes());
        bytes.extend_from_slice(&self.ip_address.octets());
        bytes.extend_from_slice(&self.subnet_mask.octets());
        bytes.extend_from_slice(&self.next_hop.octets());
        bytes.extend_from_slice(&self.metric.to_be_bytes());
        bytes
    }

    /// Decode route entry from bytes
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < 20 {
            return Err(anyhow::anyhow!("RIP route entry too short"));
        }

        Ok(Self {
            address_family: u16::from_be_bytes([data[0], data[1]]),
            route_tag: u16::from_be_bytes([data[2], data[3]]),
            ip_address: Ipv4Addr::new(data[4], data[5], data[6], data[7]),
            subnet_mask: Ipv4Addr::new(data[8], data[9], data[10], data[11]),
            next_hop: Ipv4Addr::new(data[12], data[13], data[14], data[15]),
            metric: u32::from_be_bytes([data[16], data[17], data[18], data[19]]),
        })
    }
}

/// RIP message structure
#[derive(Debug, Clone)]
pub struct RipMessage {
    pub command: RipCommand,
    pub version: RipVersion,
    pub routes: Vec<RipRouteEntry>,
}

impl RipMessage {
    /// Create a RIP request message
    pub fn request(version: RipVersion) -> Self {
        // Request for entire routing table: address family 0, metric 16
        Self {
            command: RipCommand::Request,
            version,
            routes: vec![RipRouteEntry {
                address_family: 0,
                route_tag: 0,
                ip_address: Ipv4Addr::new(0, 0, 0, 0),
                subnet_mask: Ipv4Addr::new(0, 0, 0, 0),
                next_hop: Ipv4Addr::new(0, 0, 0, 0),
                metric: 16,
            }],
        }
    }

    /// Encode RIP message to bytes
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::new();

        // Command (1 byte)
        bytes.push(self.command as u8);

        // Version (1 byte)
        bytes.push(self.version as u8);

        // Must be zero (2 bytes)
        bytes.extend_from_slice(&[0, 0]);

        // Route entries (20 bytes each)
        for route in &self.routes {
            bytes.extend_from_slice(&route.encode());
        }

        bytes
    }

    /// Decode RIP message from bytes
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < 4 {
            return Err(anyhow::anyhow!("RIP message too short"));
        }

        let command = match data[0] {
            1 => RipCommand::Request,
            2 => RipCommand::Response,
            _ => return Err(anyhow::anyhow!("Invalid RIP command: {}", data[0])),
        };

        let version = match data[1] {
            1 => RipVersion::V1,
            2 => RipVersion::V2,
            _ => return Err(anyhow::anyhow!("Invalid RIP version: {}", data[1])),
        };

        // Parse route entries
        let mut routes = Vec::new();
        let mut offset = 4;

        while offset + 20 <= data.len() {
            routes.push(RipRouteEntry::decode(&data[offset..offset + 20])?);
            offset += 20;
        }

        Ok(Self {
            command,
            version,
            routes,
        })
    }
}

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
    queued_responses: Vec<RipMessage>,
    memory: String,
}

/// RIP client that queries RIP routers
pub struct RipClient;

impl RipClient {
    /// Connect to a RIP router with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Parse remote address
        let remote_sock_addr: SocketAddr = remote_addr
            .parse()
            .context(format!("Invalid RIP router address: {}", remote_addr))?;

        // Bind to any available local port (not 520, as that requires root)
        let socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .context("Failed to bind UDP socket")?;

        let local_addr = socket.local_addr()?;


        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        console_info!(status_tx, "[CLIENT] RIP client {} connected", client_id);
        console_info!(status_tx, "__UPDATE_UI__");

        let socket_arc = Arc::new(socket);

        // Initialize client data
        let client_data = Arc::new(Mutex::new(ClientData {
            state: ConnectionState::Idle,
            queued_responses: Vec::new(),
            memory: String::new(),
        }));

        // Call LLM with connected event
        let protocol = Arc::new(crate::client::rip::actions::RipClientProtocol::new());
        let event = Event::new(
            &RIP_CLIENT_CONNECTED_EVENT,
            serde_json::json!({
                "remote_addr": remote_sock_addr.to_string(),
            }),
        );

        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &client_data.lock().await.memory,
                Some(&event),
                protocol.as_ref(),
                &status_tx,
            ).await {
                Ok(ClientLlmResult { actions, memory_updates }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        client_data.lock().await.memory = mem;
                    }

                    // Execute initial actions
                    for action in actions {
                        use crate::llm::actions::client_trait::Client;
                        match protocol.as_ref().execute_action(action) {
                            Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data }) => {
                                if name == "send_rip_request" {
                                    // Send RIP request
                                    if let Some(version) = data["version"].as_u64() {
                                        let rip_version = if version == 1 { RipVersion::V1 } else { RipVersion::V2 };
                                        let request = RipMessage::request(rip_version);
                                        let bytes = request.encode();

                                        if let Err(e) = socket_arc.send_to(&bytes, remote_sock_addr).await {
                                            error!("Failed to send RIP request: {}", e);
                                        } else {
                                            debug!("RIP client {} sent request (version {})", client_id, version);
                                        }
                                    }
                                }
                            }
                            Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                                info!("RIP client {} disconnecting", client_id);
                                app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                                return Ok(local_addr);
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error for RIP client {}: {}", client_id, e);
                }
            }
        }

        // Spawn receive loop
        let socket_clone = socket_arc.clone();
        let client_data_clone = client_data.clone();
        tokio::spawn(async move {
            let mut buffer = vec![0u8; 1500]; // Max UDP packet size

            loop {
                match socket_clone.recv_from(&mut buffer).await {
                    Ok((n, peer)) => {
                        trace!("RIP client {} received {} bytes from {}", client_id, n, peer);

                        // Parse RIP message
                        match RipMessage::decode(&buffer[..n]) {
                            Ok(msg) => {
                                let mut client_data_lock = client_data_clone.lock().await;

                                match client_data_lock.state {
                                    ConnectionState::Idle => {
                                        // Process immediately
                                        client_data_lock.state = ConnectionState::Processing;
                                        drop(client_data_lock);

                                        // Call LLM with response event
                                        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                                            let routes: Vec<_> = msg.routes.iter().map(|r| {
                                                serde_json::json!({
                                                    "ip_address": r.ip_address.to_string(),
                                                    "subnet_mask": r.subnet_mask.to_string(),
                                                    "next_hop": r.next_hop.to_string(),
                                                    "metric": r.metric,
                                                })
                                            }).collect();

                                            let event = Event::new(
                                                &RIP_CLIENT_RESPONSE_RECEIVED_EVENT,
                                                serde_json::json!({
                                                    "version": msg.version as u8,
                                                    "command": match msg.command {
                                                        RipCommand::Request => "request",
                                                        RipCommand::Response => "response",
                                                    },
                                                    "route_count": routes.len(),
                                                    "routes": routes,
                                                }),
                                            );

                                            match call_llm_for_client(
                                                &llm_client,
                                                &app_state,
                                                client_id.to_string(),
                                                &instruction,
                                                &client_data_clone.lock().await.memory,
                                                Some(&event),
                                                protocol.as_ref(),
                                                &status_tx,
                                            ).await {
                                                Ok(ClientLlmResult { actions, memory_updates }) => {
                                                    // Update memory
                                                    if let Some(mem) = memory_updates {
                                                        client_data_clone.lock().await.memory = mem;
                                                    }

                                                    // Execute actions
                                                    for action in actions {
                                                        use crate::llm::actions::client_trait::Client;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};
                                                        match protocol.as_ref().execute_action(action) {
                                                            Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data }) => {
                                                                if name == "send_rip_request" {
                                                                    if let Some(version) = data["version"].as_u64() {
                                                                        let rip_version = if version == 1 { RipVersion::V1 } else { RipVersion::V2 };
                                                                        let request = RipMessage::request(rip_version);
                                                                        let bytes = request.encode();

                                                                        if let Err(e) = socket_clone.send_to(&bytes, remote_sock_addr).await {
                                                                            error!("Failed to send RIP request: {}", e);
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                            Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                                                                info!("RIP client {} disconnecting", client_id);
                                                                app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                                                                return;
                                                            }
                                                            _ => {}
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    error!("LLM error for RIP client {}: {}", client_id, e);
                                                }
                                            }
                                        }

                                        // Reset state
                                        let mut client_data_lock = client_data_clone.lock().await;
                                        client_data_lock.queued_responses.clear();
                                        client_data_lock.state = ConnectionState::Idle;
                                    }
                                    ConnectionState::Processing => {
                                        // Queue response
                                        client_data_lock.queued_responses.push(msg);
                                        client_data_lock.state = ConnectionState::Accumulating;
                                    }
                                    ConnectionState::Accumulating => {
                                        // Continue queuing
                                        client_data_lock.queued_responses.push(msg);
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Failed to parse RIP message: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        app_state.update_client_status(client_id, ClientStatus::Error(e.to_string())).await;
                        console_error!(status_tx, "__UPDATE_UI__");
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}
