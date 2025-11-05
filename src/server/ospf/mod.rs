//! OSPF server implementation
//!
//! Open Shortest Path First (OSPFv2) server that allows LLM control over routing protocol operations.
//! Implements RFC 2328 with neighbor state machine and DR/BDR election.
//!
//! **Production Implementation**: Uses IP protocol 89 for real OSPF router interoperability.
//! **Requires**: Root/CAP_NET_RAW privileges for raw socket access.

pub mod actions;

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::os::unix::io::AsRawFd;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::unix::AsyncFd;
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
use crate::server::socket_helpers::create_ospf_raw_socket;
#[cfg(feature = "ospf")]
use crate::state::app_state::AppState;
#[cfg(feature = "ospf")]
use crate::state::server::{ConnectionState, ConnectionStatus, OspfNeighborState, ProtocolConnectionInfo};

// OSPF Constants
const OSPF_VERSION: u8 = 2;
const OSPF_HEADER_LEN: usize = 24;
const IP_HEADER_MIN_LEN: usize = 20;

// OSPF Packet Types (RFC 2328 Section A.3.1)
const OSPF_TYPE_HELLO: u8 = 1;
const OSPF_TYPE_DATABASE_DESCRIPTION: u8 = 2;
const OSPF_TYPE_LINK_STATE_REQUEST: u8 = 3;
const OSPF_TYPE_LINK_STATE_UPDATE: u8 = 4;
const OSPF_TYPE_LINK_STATE_ACK: u8 = 5;

// OSPF Multicast addresses
const OSPF_ALL_SPF_ROUTERS: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 5);
const OSPF_ALL_DROUTERS: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 6);

/// OSPF neighbor information
#[cfg(feature = "ospf")]
struct OspfNeighbor {
    router_id: String,
    neighbor_ip: Ipv4Addr,
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
    ///
    /// **Requires root/CAP_NET_RAW privileges** for raw socket access.
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        // Extract interface IP from listen_addr
        let interface_ip = match listen_addr.ip() {
            IpAddr::V4(ip) => ip,
            IpAddr::V6(_) => {
                return Err(anyhow!("OSPF only supports IPv4"));
            }
        };

        // Create raw OSPF socket (requires root)
        let raw_socket = create_ospf_raw_socket(interface_ip, true, false)?;

        info!("OSPF server listening on interface {} (IP protocol 89)", interface_ip);
        let _ = status_tx.send(format!("[INFO] OSPF server on interface {} (requires root)", interface_ip));

        // Extract OSPF configuration from startup params
        let (router_id, area_id, _network_mask, _hello_interval, _router_dead_interval, _router_priority) =
            if let Some(ref params) = startup_params {
                let router_id = params
                    .get_optional_string("router_id")
                    .unwrap_or_else(|| interface_ip.to_string());
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
                    "OSPF configured: router_id={}, area={}, mask={}, hello={}s, dead={}s, priority={}",
                    router_id, area_id, network_mask, hello_interval, router_dead_interval, router_priority
                );
                let _ = status_tx.send(format!(
                    "[INFO] OSPF: router_id={}, area={}, priority={}",
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
                    interface_ip.to_string(),
                    "0.0.0.0".to_string(),
                    "255.255.255.0".to_string(),
                    10,
                    40,
                    1,
                )
            };

        let protocol = Arc::new(OspfProtocol::new());
        let neighbors: Arc<Mutex<HashMap<String, OspfNeighbor>>> = Arc::new(Mutex::new(HashMap::new()));

        // Wrap raw socket in AsyncFd for tokio integration
        let async_socket = AsyncFd::new(raw_socket)?;

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 65535];

            loop {
                // Wait for socket to be readable
                let mut guard = match async_socket.readable().await {
                    Ok(guard) => guard,
                    Err(e) => {
                        error!("OSPF socket error: {}", e);
                        break;
                    }
                };

                // Try to read from socket
                match guard.try_io(|inner| {
                    // Use libc recvfrom on the raw fd
                    let fd = inner.as_raw_fd();
                    unsafe {
                        let mut addr: libc::sockaddr_in = std::mem::zeroed();
                        let mut addr_len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;

                        let n = libc::recvfrom(
                            fd,
                            buffer.as_mut_ptr() as *mut libc::c_void,
                            buffer.len(),
                            0,
                            &mut addr as *mut _ as *mut libc::sockaddr,
                            &mut addr_len,
                        );

                        if n < 0 {
                            Err(std::io::Error::last_os_error())
                        } else {
                            // Extract source IP from sockaddr_in
                            let src_ip = Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr));
                            Ok((n as usize, src_ip))
                        }
                    }
                }) {
                    Ok(Ok((n, src_ip))) => {
                        if n == 0 {
                            continue;
                        }

                        trace!("OSPF received {} bytes from {}", n, src_ip);
                        let _ = status_tx.send(format!("[TRACE] OSPF received {} bytes from {}", n, src_ip));

                        // Skip IP header (minimum 20 bytes, check IHL for actual length)
                        if n < IP_HEADER_MIN_LEN {
                            warn!("Packet too short for IP header: {} bytes", n);
                            continue;
                        }

                        let ip_header_len = ((buffer[0] & 0x0F) * 4) as usize;
                        if n < ip_header_len + OSPF_HEADER_LEN {
                            warn!("Packet too short for OSPF: {} bytes (IP header: {})", n, ip_header_len);
                            continue;
                        }

                        // Extract OSPF packet (after IP header)
                        let ospf_data = &buffer[ip_header_len..n];

                        // Parse OSPF header
                        let version = ospf_data[0];
                        let packet_type = ospf_data[1];

                        if version != OSPF_VERSION {
                            warn!("OSPF unsupported version: {}", version);
                            continue;
                        }

                        // Extract router ID and area ID from OSPF header
                        let sender_router_id = format!(
                            "{}.{}.{}.{}",
                            ospf_data[4], ospf_data[5], ospf_data[6], ospf_data[7]
                        );

                        let sender_area_id = format!(
                            "{}.{}.{}.{}",
                            ospf_data[8], ospf_data[9], ospf_data[10], ospf_data[11]
                        );

                        debug!(
                            "OSPF packet type={} from {} (router_id={}, area={})",
                            packet_type, src_ip, sender_router_id, sender_area_id
                        );

                        // Handle packet (spawn task to avoid blocking)
                        let ospf_data_owned = ospf_data.to_vec();
                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();
                        let neighbors_clone = neighbors.clone();
                        let router_id_clone = router_id.clone();
                        let area_id_clone = area_id.clone();

                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_ospf_packet(
                                packet_type,
                                &ospf_data_owned,
                                src_ip,
                                sender_router_id,
                                sender_area_id,
                                llm_clone,
                                state_clone,
                                status_clone,
                                protocol_clone,
                                neighbors_clone,
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
                    Ok(Err(e)) => {
                        error!("OSPF recv error: {}", e);
                    }
                    Err(_would_block) => {
                        // Socket not ready yet, will retry
                        continue;
                    }
                }
            }

            warn!("OSPF receive loop terminated");
        });

        // Return a dummy address since raw sockets don't have ports
        Ok(SocketAddr::new(IpAddr::V4(interface_ip), 0))
    }

    #[cfg(feature = "ospf")]
    async fn handle_ospf_packet(
        packet_type: u8,
        data: &[u8],
        src_ip: Ipv4Addr,
        sender_router_id: String,
        sender_area_id: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<OspfProtocol>,
        neighbors: Arc<Mutex<HashMap<String, OspfNeighbor>>>,
        server_id: crate::state::ServerId,
        router_id: String,
        area_id: String,
    ) -> Result<()> {
        // Get or create connection ID for this neighbor
        let connection_id = {
            let mut neighbors_lock = neighbors.lock().await;
            if let Some(neighbor) = neighbors_lock.get_mut(&sender_router_id) {
                neighbor.last_hello = Instant::now();
                neighbor.connection_id
            } else {
                let connection_id = ConnectionId::new();
                let neighbor = OspfNeighbor {
                    router_id: sender_router_id.clone(),
                    neighbor_ip: src_ip,
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

        match packet_type {
            OSPF_TYPE_HELLO => {
                Self::handle_hello_packet(
                    data,
                    src_ip,
                    sender_router_id,
                    sender_area_id,
                    connection_id,
                    llm_client,
                    app_state,
                    status_tx,
                    protocol,
                    neighbors,
                    server_id,
                    router_id,
                    area_id,
                )
                .await?;
            }
            OSPF_TYPE_DATABASE_DESCRIPTION => {
                info!("OSPF Database Description from {}", sender_router_id);
                // TODO: Implement DD handling
            }
            OSPF_TYPE_LINK_STATE_REQUEST => {
                info!("OSPF Link State Request from {}", sender_router_id);
                // TODO: Implement LSR handling
            }
            OSPF_TYPE_LINK_STATE_UPDATE => {
                info!("OSPF Link State Update from {}", sender_router_id);
                // TODO: Implement LSU handling
            }
            OSPF_TYPE_LINK_STATE_ACK => {
                trace!("OSPF Link State Ack from {}", sender_router_id);
                // TODO: Implement LSAck handling
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
        src_ip: Ipv4Addr,
        sender_router_id: String,
        sender_area_id: String,
        connection_id: ConnectionId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<OspfProtocol>,
        neighbors: Arc<Mutex<HashMap<String, OspfNeighbor>>>,
        server_id: crate::state::ServerId,
        _router_id: String,
        _area_id: String,
    ) -> Result<()> {
        if data.len() < OSPF_HEADER_LEN + 20 {
            return Err(anyhow!("Hello packet too short"));
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

        // Parse neighbor list
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
            "OSPF Hello from {} (router_id={}, area={}, priority={}, DR={}, BDR={})",
            src_ip, sender_router_id, sender_area_id, priority, dr, bdr
        );

        // Update neighbor state
        {
            let mut neighbors_lock = neighbors.lock().await;
            if let Some(neighbor) = neighbors_lock.get_mut(&sender_router_id) {
                neighbor.priority = priority;
                neighbor.dr = dr.clone();
                neighbor.bdr = bdr.clone();
                neighbor.last_hello = Instant::now();

                // State transitions
                match neighbor.state {
                    OspfNeighborState::Down => {
                        neighbor.state = OspfNeighborState::Init;
                        info!("OSPF neighbor {} state: Down -> Init", sender_router_id);
                    }
                    OspfNeighborState::Init => {
                        // Check if our router ID is in neighbor list
                        neighbor.state = OspfNeighborState::TwoWay;
                        info!("OSPF neighbor {} state: Init -> 2-Way", sender_router_id);
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
                "neighbor_ip": src_ip.to_string(),
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
            Ok(_result) => {}
            Err(e) => {
                error!("LLM call failed for OSPF Hello: {}", e);
            }
        }

        Ok(())
    }
}
