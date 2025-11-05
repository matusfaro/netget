//! OSPF server implementation
//!
//! Open Shortest Path First (OSPFv2) server that allows LLM control over routing protocol operations.
//! Implements RFC 2328 with neighbor state machine.
//!
//! **Note**: Uses UDP transport (port 2600) instead of IP protocol 89 for portability.

pub mod actions;

use anyhow::Result;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace, warn};

#[cfg(feature = "ospf")]
use crate::llm::action_helper::call_llm;
#[cfg(feature = "ospf")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "ospf")]
use actions::{
    OspfProtocol, OSPF_DATABASE_DESCRIPTION_EVENT, OSPF_HELLO_EVENT,
    OSPF_LINK_STATE_ACK_EVENT, OSPF_LINK_STATE_REQUEST_EVENT, OSPF_LINK_STATE_UPDATE_EVENT,
};
#[cfg(feature = "ospf")]
use crate::protocol::Event;
#[cfg(feature = "ospf")]
use crate::server::connection::ConnectionId;
#[cfg(feature = "ospf")]
use crate::state::app_state::AppState;
#[cfg(feature = "ospf")]
use crate::state::server::{ConnectionState, ConnectionStatus, OspfNeighborState, ProtocolConnectionInfo};

// OSPF Constants
const OSPF_VERSION: u8 = 2;
const OSPF_HEADER_LEN: usize = 24;

// OSPF Packet Types (RFC 2328 Section A.3.1)
const OSPF_TYPE_HELLO: u8 = 1;
const OSPF_TYPE_DATABASE_DESCRIPTION: u8 = 2;
const OSPF_TYPE_LINK_STATE_REQUEST: u8 = 3;
const OSPF_TYPE_LINK_STATE_UPDATE: u8 = 4;
const OSPF_TYPE_LINK_STATE_ACK: u8 = 5;

/// OSPF neighbor information
#[cfg(feature = "ospf")]
struct OspfNeighbor {
    router_id: String,
    neighbor_addr: SocketAddr,
    connection_id: ConnectionId,
    state: OspfNeighborState,
    priority: u8,
    dr: String,
    bdr: String,
    last_hello: Instant,
    dd_sequence: u32,
    master: bool,
}

/// OSPF server that handles routing protocol operations with LLM
pub struct OspfServer;

#[cfg(feature = "ospf")]
impl OspfServer {
    /// Spawn OSPF server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        info!("OSPF server listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] OSPF server listening on {}", local_addr));

        // Extract OSPF configuration from startup params
        let (router_id, area_id, network_mask, hello_interval, router_dead_interval, router_priority) =
            if let Some(ref params) = startup_params {
                let router_id = params
                    .get_optional_string("router_id")
                    .unwrap_or_else(|| "1.1.1.1".to_string());
                let area_id = params
                    .get_optional_string("area_id")
                    .unwrap_or_else(|| "0.0.0.0".to_string());
                let network_mask = params
                    .get_optional_string("network_mask")
                    .unwrap_or_else(|| "255.255.255.0".to_string());
                let hello_interval = params.get_optional_u32("hello_interval").unwrap_or(10);
                let router_dead_interval = params.get_optional_u32("router_dead_interval").unwrap_or(40);
                let router_priority = params.get_optional_u32("router_priority").unwrap_or(1) as u8;

                info!(
                    "OSPF configured: router_id={}, area={}, mask={}, hello_interval={}s, dead_interval={}s, priority={}",
                    router_id, area_id, network_mask, hello_interval, router_dead_interval, router_priority
                );
                let _ = status_tx.send(format!(
                    "[INFO] OSPF configured: router_id={}, area={}, priority={}",
                    router_id, area_id, router_priority
                ));

                (
                    router_id,
                    area_id,
                    network_mask,
                    hello_interval,
                    router_dead_interval,
                    router_priority,
                )
            } else {
                (
                    "1.1.1.1".to_string(),
                    "0.0.0.0".to_string(),
                    "255.255.255.0".to_string(),
                    10,
                    40,
                    1,
                )
            };

        let protocol = Arc::new(OspfProtocol::new());
        let neighbors: Arc<Mutex<HashMap<String, OspfNeighbor>>> = Arc::new(Mutex::new(HashMap::new()));

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 65535];

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();

                        trace!("OSPF received {} bytes from {}", n, peer_addr);
                        let _ = status_tx.send(format!("[TRACE] OSPF received {} bytes from {}", n, peer_addr));

                        // Parse OSPF header
                        if n < OSPF_HEADER_LEN {
                            warn!("OSPF packet too short: {} bytes", n);
                            continue;
                        }

                        let version = data[0];
                        let packet_type = data[1];

                        if version != OSPF_VERSION {
                            warn!("OSPF unsupported version: {}", version);
                            continue;
                        }

                        // Extract router ID from header
                        let sender_router_id = format!(
                            "{}.{}.{}.{}",
                            data[4], data[5], data[6], data[7]
                        );

                        // Extract area ID from header
                        let sender_area_id = format!(
                            "{}.{}.{}.{}",
                            data[8], data[9], data[10], data[11]
                        );

                        debug!(
                            "OSPF packet type={} from router {} (area {})",
                            packet_type, sender_router_id, sender_area_id
                        );
                        let _ = status_tx.send(format!(
                            "[DEBUG] OSPF packet type={} from {}",
                            packet_type, sender_router_id
                        ));

                        // Get or create neighbor entry
                        let connection_id = {
                            let mut neighbors_lock = neighbors.lock().await;
                            if let Some(neighbor) = neighbors_lock.get_mut(&sender_router_id) {
                                neighbor.last_hello = Instant::now();
                                neighbor.connection_id
                            } else {
                                let connection_id = ConnectionId::new();
                                let neighbor = OspfNeighbor {
                                    router_id: sender_router_id.clone(),
                                    neighbor_addr: peer_addr,
                                    connection_id,
                                    state: OspfNeighborState::Down,
                                    priority: 0,
                                    dr: "0.0.0.0".to_string(),
                                    bdr: "0.0.0.0".to_string(),
                                    last_hello: Instant::now(),
                                    dd_sequence: 0,
                                    master: false,
                                };
                                neighbors_lock.insert(sender_router_id.clone(), neighbor);
                                connection_id
                            }
                        };

                        // Add/update connection in app state
                        let now = Instant::now();
                        let neighbors_lock = neighbors.lock().await;
                        if let Some(neighbor) = neighbors_lock.get(&sender_router_id) {
                            let conn_state = ConnectionState {
                                id: connection_id,
                                remote_addr: peer_addr,
                                local_addr,
                                bytes_sent: 0,
                                bytes_received: n as u64,
                                packets_sent: 0,
                                packets_received: 1,
                                last_activity: now,
                                status: ConnectionStatus::Active,
                                status_changed_at: now,
                                protocol_info: ProtocolConnectionInfo::Ospf {
                                    neighbor_state: neighbor.state.clone(),
                                    router_id: neighbor.router_id.clone(),
                                    area_id: sender_area_id.clone(),
                                    dr: neighbor.dr.clone(),
                                    bdr: neighbor.bdr.clone(),
                                },
                            };
                            app_state.add_connection_to_server(server_id, conn_state).await;
                        }
                        drop(neighbors_lock);
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();
                        let neighbors_clone = neighbors.clone();
                        let socket_clone = socket.clone();
                        let router_id_clone = router_id.clone();
                        let area_id_clone = area_id.clone();

                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_ospf_packet(
                                packet_type,
                                &data,
                                peer_addr,
                                sender_router_id,
                                sender_area_id,
                                connection_id,
                                llm_clone,
                                state_clone,
                                status_clone,
                                protocol_clone,
                                neighbors_clone,
                                socket_clone,
                                server_id,
                                router_id_clone,
                                area_id_clone,
                            )
                            .await
                            {
                                error!("OSPF packet handling error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("OSPF recv error: {}", e);
                        let _ = status_tx.send(format!("[ERROR] OSPF recv error: {}", e));
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    #[cfg(feature = "ospf")]
    async fn handle_ospf_packet(
        packet_type: u8,
        data: &[u8],
        peer_addr: SocketAddr,
        sender_router_id: String,
        sender_area_id: String,
        connection_id: ConnectionId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<OspfProtocol>,
        neighbors: Arc<Mutex<HashMap<String, OspfNeighbor>>>,
        socket: Arc<UdpSocket>,
        server_id: crate::state::ServerId,
        router_id: String,
        area_id: String,
    ) -> Result<()> {
        match packet_type {
            OSPF_TYPE_HELLO => {
                Self::handle_hello_packet(
                    data,
                    peer_addr,
                    sender_router_id,
                    sender_area_id,
                    connection_id,
                    llm_client,
                    app_state,
                    status_tx,
                    protocol,
                    neighbors,
                    socket,
                    server_id,
                    router_id,
                    area_id,
                )
                .await?;
            }
            OSPF_TYPE_DATABASE_DESCRIPTION => {
                Self::handle_database_description_packet(
                    data,
                    peer_addr,
                    sender_router_id,
                    connection_id,
                    llm_client,
                    app_state,
                    status_tx,
                    protocol,
                    server_id,
                )
                .await?;
            }
            OSPF_TYPE_LINK_STATE_REQUEST => {
                Self::handle_link_state_request_packet(
                    data,
                    peer_addr,
                    sender_router_id,
                    connection_id,
                    llm_client,
                    app_state,
                    status_tx,
                    protocol,
                    server_id,
                )
                .await?;
            }
            OSPF_TYPE_LINK_STATE_UPDATE => {
                Self::handle_link_state_update_packet(
                    data,
                    peer_addr,
                    sender_router_id,
                    connection_id,
                    llm_client,
                    app_state,
                    status_tx,
                    protocol,
                    server_id,
                )
                .await?;
            }
            OSPF_TYPE_LINK_STATE_ACK => {
                Self::handle_link_state_ack_packet(
                    data,
                    peer_addr,
                    sender_router_id,
                    connection_id,
                    llm_client,
                    app_state,
                    status_tx,
                    protocol,
                    server_id,
                )
                .await?;
            }
            _ => {
                warn!("OSPF unknown packet type: {}", packet_type);
            }
        }

        Ok(())
    }

    #[cfg(feature = "ospf")]
    async fn handle_hello_packet(
        data: &[u8],
        peer_addr: SocketAddr,
        sender_router_id: String,
        sender_area_id: String,
        connection_id: ConnectionId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<OspfProtocol>,
        neighbors: Arc<Mutex<HashMap<String, OspfNeighbor>>>,
        _socket: Arc<UdpSocket>,
        server_id: crate::state::ServerId,
        _router_id: String,
        _area_id: String,
    ) -> Result<()> {
        if data.len() < OSPF_HEADER_LEN + 20 {
            return Err(anyhow::anyhow!("Hello packet too short"));
        }

        // Parse Hello packet fields
        let network_mask = format!(
            "{}.{}.{}.{}",
            data[24], data[25], data[26], data[27]
        );
        let hello_interval = u16::from_be_bytes([data[28], data[29]]);
        let priority = data[31];
        let router_dead_interval = u32::from_be_bytes([data[32], data[33], data[34], data[35]]);
        let dr = format!(
            "{}.{}.{}.{}",
            data[36], data[37], data[38], data[39]
        );
        let bdr = format!(
            "{}.{}.{}.{}",
            data[40], data[41], data[42], data[43]
        );

        // Parse neighbor list (remaining bytes, 4 bytes per neighbor)
        let mut neighbor_list = Vec::new();
        let mut offset = 44;
        while offset + 4 <= data.len() {
            let neighbor_id = format!(
                "{}.{}.{}.{}",
                data[offset], data[offset + 1], data[offset + 2], data[offset + 3]
            );
            neighbor_list.push(neighbor_id);
            offset += 4;
        }

        info!(
            "OSPF Hello from {} (area {}, priority {}, DR {}, BDR {})",
            sender_router_id, sender_area_id, priority, dr, bdr
        );
        let _ = status_tx.send(format!(
            "[INFO] OSPF Hello from {} (priority {})",
            sender_router_id, priority
        ));

        // Update neighbor state
        {
            let mut neighbors_lock = neighbors.lock().await;
            if let Some(neighbor) = neighbors_lock.get_mut(&sender_router_id) {
                neighbor.priority = priority;
                neighbor.dr = dr.clone();
                neighbor.bdr = bdr.clone();
                neighbor.last_hello = Instant::now();

                // Update state: Down -> Init or Init -> 2-Way
                match neighbor.state {
                    OspfNeighborState::Down => {
                        neighbor.state = OspfNeighborState::Init;
                        debug!("OSPF neighbor {} state: Down -> Init", sender_router_id);
                        let _ = status_tx.send(format!(
                            "[DEBUG] OSPF neighbor {} state: Down -> Init",
                            sender_router_id
                        ));
                    }
                    OspfNeighborState::Init => {
                        // Check if our router ID is in neighbor list
                        // (simplified - should check against our actual router ID)
                        neighbor.state = OspfNeighborState::TwoWay;
                        debug!("OSPF neighbor {} state: Init -> 2-Way", sender_router_id);
                        let _ = status_tx.send(format!(
                            "[DEBUG] OSPF neighbor {} state: Init -> 2-Way",
                            sender_router_id
                        ));
                    }
                    _ => {}
                }
            }
        }

        // Ask LLM how to respond
        let event = Event {
            event_type: &OSPF_HELLO_EVENT,
            data: serde_json::json!({
                "connection_id": connection_id.to_string(),
                "neighbor_id": sender_router_id,
                "neighbor_addr": peer_addr.to_string(),
                "area_id": sender_area_id,
                "network_mask": network_mask,
                "hello_interval": hello_interval,
                "router_dead_interval": router_dead_interval,
                "router_priority": priority,
                "dr": dr,
                "bdr": bdr,
                "neighbors": neighbor_list,
            }),
        };

        match call_llm(
            &llm_client,
            &app_state,
            server_id,
            Some(connection_id),
            &event,
            &*protocol,
        )
        .await
        {
            Ok(_result) => {
                // Actions are executed automatically by the action system
            }
            Err(e) => {
                error!("LLM call failed for OSPF Hello: {}", e);
                let _ = status_tx.send(format!("[ERROR] LLM call failed: {}", e));
            }
        }

        Ok(())
    }

    #[cfg(feature = "ospf")]
    async fn handle_database_description_packet(
        data: &[u8],
        _peer_addr: SocketAddr,
        sender_router_id: String,
        connection_id: ConnectionId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<OspfProtocol>,
        server_id: crate::state::ServerId,
    ) -> Result<()> {
        if data.len() < OSPF_HEADER_LEN + 8 {
            return Err(anyhow::anyhow!("DD packet too short"));
        }

        // Parse DD packet
        let flags = data[27];
        let dd_sequence = u32::from_be_bytes([data[28], data[29], data[30], data[31]]);

        let init = (flags & 0x04) != 0;
        let more = (flags & 0x02) != 0;
        let master = (flags & 0x01) != 0;

        debug!(
            "OSPF Database Description from {}: seq={}, init={}, more={}, master={}",
            sender_router_id, dd_sequence, init, more, master
        );
        let _ = status_tx.send(format!(
            "[DEBUG] OSPF DD from {}: seq={}",
            sender_router_id, dd_sequence
        ));

        // Ask LLM how to respond
        let event = Event {
            event_type: &OSPF_DATABASE_DESCRIPTION_EVENT,
            data: serde_json::json!({
                "connection_id": connection_id.to_string(),
                "neighbor_id": sender_router_id,
                "sequence": dd_sequence,
                "init": init,
                "more": more,
                "master": master,
            }),
        };

        match call_llm(
            &llm_client,
            &app_state,
            server_id,
            Some(connection_id),
            &event,
            &*protocol,
        )
        .await
        {
            Ok(_result) => {}
            Err(e) => {
                error!("LLM call failed for OSPF DD: {}", e);
                let _ = status_tx.send(format!("[ERROR] LLM call failed: {}", e));
            }
        }

        Ok(())
    }

    #[cfg(feature = "ospf")]
    async fn handle_link_state_request_packet(
        _data: &[u8],
        _peer_addr: SocketAddr,
        sender_router_id: String,
        connection_id: ConnectionId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<OspfProtocol>,
        server_id: crate::state::ServerId,
    ) -> Result<()> {
        debug!("OSPF Link State Request from {}", sender_router_id);
        let _ = status_tx.send(format!("[DEBUG] OSPF LSR from {}", sender_router_id));

        let event = Event {
            event_type: &OSPF_LINK_STATE_REQUEST_EVENT,
            data: serde_json::json!({
                "connection_id": connection_id.to_string(),
                "neighbor_id": sender_router_id,
            }),
        };

        match call_llm(
            &llm_client,
            &app_state,
            server_id,
            Some(connection_id),
            &event,
            &*protocol,
        )
        .await
        {
            Ok(_result) => {}
            Err(e) => {
                error!("LLM call failed for OSPF LSR: {}", e);
            }
        }

        Ok(())
    }

    #[cfg(feature = "ospf")]
    async fn handle_link_state_update_packet(
        _data: &[u8],
        _peer_addr: SocketAddr,
        sender_router_id: String,
        connection_id: ConnectionId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<OspfProtocol>,
        server_id: crate::state::ServerId,
    ) -> Result<()> {
        debug!("OSPF Link State Update from {}", sender_router_id);
        let _ = status_tx.send(format!("[DEBUG] OSPF LSU from {}", sender_router_id));

        let event = Event {
            event_type: &OSPF_LINK_STATE_UPDATE_EVENT,
            data: serde_json::json!({
                "connection_id": connection_id.to_string(),
                "neighbor_id": sender_router_id,
            }),
        };

        match call_llm(
            &llm_client,
            &app_state,
            server_id,
            Some(connection_id),
            &event,
            &*protocol,
        )
        .await
        {
            Ok(_result) => {}
            Err(e) => {
                error!("LLM call failed for OSPF LSU: {}", e);
            }
        }

        Ok(())
    }

    #[cfg(feature = "ospf")]
    async fn handle_link_state_ack_packet(
        _data: &[u8],
        _peer_addr: SocketAddr,
        sender_router_id: String,
        connection_id: ConnectionId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<OspfProtocol>,
        server_id: crate::state::ServerId,
    ) -> Result<()> {
        trace!("OSPF Link State Ack from {}", sender_router_id);
        let _ = status_tx.send(format!("[TRACE] OSPF LSAck from {}", sender_router_id));

        let event = Event {
            event_type: &OSPF_LINK_STATE_ACK_EVENT,
            data: serde_json::json!({
                "connection_id": connection_id.to_string(),
                "neighbor_id": sender_router_id,
            }),
        };

        match call_llm(
            &llm_client,
            &app_state,
            server_id,
            Some(connection_id),
            &event,
            &*protocol,
        )
        .await
        {
            Ok(_result) => {}
            Err(e) => {
                error!("LLM call failed for OSPF LSAck: {}", e);
            }
        }

        Ok(())
    }
}
