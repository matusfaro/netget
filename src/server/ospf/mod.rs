//! OSPF protocol simulator - LLM-controlled OSPF responses
//!
//! This is an OSPF protocol simulator that speaks real OSPF (IP protocol 89)
//! but has LLM-generated responses instead of real routing logic.
//!
//! **Use cases**: Testing, honeypots, route injection, OSPF reconnaissance
//! **Requires**: Root/CAP_NET_RAW privileges for raw socket access

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
use hex;

#[cfg(feature = "ospf")]
use crate::llm::action_helper::call_llm;
#[cfg(feature = "ospf")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "ospf")]
use actions::{
    OspfProtocol, OSPF_HELLO_EVENT,
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
use crate::state::server::OspfNeighborState;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

// OSPF Constants
const OSPF_VERSION: u8 = 2;
const OSPF_HEADER_LEN: usize = 24;
const IP_HEADER_MIN_LEN: usize = 20;

// OSPF Packet Types
const OSPF_TYPE_HELLO: u8 = 1;
const OSPF_TYPE_DATABASE_DESCRIPTION: u8 = 2;
const OSPF_TYPE_LINK_STATE_REQUEST: u8 = 3;
const OSPF_TYPE_LINK_STATE_UPDATE: u8 = 4;
const OSPF_TYPE_LINK_STATE_ACK: u8 = 5;

// OSPF Multicast addresses
const OSPF_ALL_SPF_ROUTERS: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 5);
#[allow(dead_code)]
const OSPF_ALL_DROUTERS: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 6);

/// OSPF neighbor information
#[cfg(feature = "ospf")]
struct OspfNeighbor {
    #[allow(dead_code)]
    router_id: String,
    #[allow(dead_code)]
    neighbor_ip: Ipv4Addr,
    connection_id: ConnectionId,
    state: OspfNeighborState,
    priority: u8,
    dr: String,
    bdr: String,
    last_hello: Instant,
}

/// Shared OSPF server state
#[cfg(feature = "ospf")]
struct OspfState {
    socket_fd: i32,
    #[allow(dead_code)]
    interface_ip: Ipv4Addr,
    #[allow(dead_code)]
    router_id: String,
    #[allow(dead_code)]
    area_id: String,
    neighbors: Arc<Mutex<HashMap<String, OspfNeighbor>>>,
}

/// OSPF server
pub struct OspfServer;

#[cfg(feature = "ospf")]
impl OspfServer {
    /// Spawn OSPF server with LLM control
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        // Extract interface IP
        let interface_ip = match listen_addr.ip() {
            IpAddr::V4(ip) => ip,
            IpAddr::V6(_) => return Err(anyhow!("OSPF only supports IPv4")),
        };

        // Create raw OSPF socket
        let raw_socket = create_ospf_raw_socket(interface_ip, true, false)?;
        let socket_fd = raw_socket.as_raw_fd();

        console_info!(status_tx, "[INFO] OSPF server on {} (requires root)", interface_ip);

        // Extract configuration
        let (router_id, area_id) = if let Some(ref params) = startup_params {
            let router_id = params
                .get_optional_string("router_id")
                .unwrap_or_else(|| interface_ip.to_string());
            let area_id = params
                .get_optional_string("area_id")
                .unwrap_or_else(|| "0.0.0.0".to_string());

            console_info!(status_tx, "[INFO] OSPF: router_id={}, area={}", router_id, area_id);

            (router_id, area_id)
        } else {
            (interface_ip.to_string(), "0.0.0.0".to_string())
        };

        let protocol = Arc::new(OspfProtocol::new());
        let neighbors: Arc<Mutex<HashMap<String, OspfNeighbor>>> = Arc::new(Mutex::new(HashMap::new()));

        let ospf_state = Arc::new(OspfState {
            socket_fd,
            interface_ip,
            router_id: router_id.clone(),
            area_id: area_id.clone(),
            neighbors: neighbors.clone(),
        });

        // Wrap socket for async I/O
        let async_socket = AsyncFd::new(raw_socket)?;

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 65535];

            loop {
                let mut guard = match async_socket.readable().await {
                    Ok(guard) => guard,
                    Err(e) => {
                        error!("OSPF socket error: {}", e);
                        break;
                    }
                };

                match guard.try_io(|inner| {
                    let fd = inner.as_raw_fd();
                    unsafe {
                        let n = libc::recv(
                            fd,
                            buffer.as_mut_ptr() as *mut libc::c_void,
                            buffer.len(),
                            0,
                        );

                        if n < 0 {
                            Err(std::io::Error::last_os_error())
                        } else {
                            Ok(n as usize)
                        }
                    }
                }) {
                    Ok(Ok(n)) => {
                        if n == 0 {
                            continue;
                        }

                        // Skip IP header
                        if n < IP_HEADER_MIN_LEN {
                            continue;
                        }

                        let ip_header_len = ((buffer[0] & 0x0F) * 4) as usize;
                        if n < ip_header_len + OSPF_HEADER_LEN {
                            continue;
                        }

                        // Extract source IP from IP header
                        let src_ip = Ipv4Addr::new(
                            buffer[12], buffer[13], buffer[14], buffer[15]
                        );

                        // Extract OSPF packet
                        let ospf_data = &buffer[ip_header_len..n];
                        let version = ospf_data[0];
                        let packet_type = ospf_data[1];

                        if version != OSPF_VERSION {
                            continue;
                        }

                        // Extract router ID and area ID
                        let sender_router_id = format!(
                            "{}.{}.{}.{}",
                            ospf_data[4], ospf_data[5], ospf_data[6], ospf_data[7]
                        );

                        let sender_area_id = format!(
                            "{}.{}.{}.{}",
                            ospf_data[8], ospf_data[9], ospf_data[10], ospf_data[11]
                        );

                        debug!("OSPF type={} from {} ({})", packet_type, src_ip, sender_router_id);

                        // Handle packet
                        let ospf_data_owned = ospf_data.to_vec();
                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();
                        let ospf_state_clone = ospf_state.clone();

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
                                ospf_state_clone,
                                server_id,
                            )
                            .await
                            {
                                error!("OSPF packet error: {}", e);
                            }
                        });
                    }
                    Ok(Err(e)) => {
                        error!("OSPF recv error: {}", e);
                    }
                    Err(_would_block) => continue,
                }
            }

            warn!("OSPF receive loop terminated");
        });

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
        ospf_state: Arc<OspfState>,
        server_id: crate::state::ServerId,
    ) -> Result<()> {
        // Get or create connection ID
        let connection_id = {
            let mut neighbors = ospf_state.neighbors.lock().await;
            if let Some(neighbor) = neighbors.get_mut(&sender_router_id) {
                neighbor.last_hello = Instant::now();
                neighbor.connection_id
            } else {
                let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);
                let neighbor = OspfNeighbor {
                    router_id: sender_router_id.clone(),
                    neighbor_ip: src_ip,
                    connection_id,
                    state: OspfNeighborState::Down,
                    priority: 0,
                    dr: "0.0.0.0".to_string(),
                    bdr: "0.0.0.0".to_string(),
                    last_hello: Instant::now(),
                };
                neighbors.insert(sender_router_id.clone(), neighbor);
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
                    ospf_state,
                    server_id,
                )
                .await?;
            }
            OSPF_TYPE_DATABASE_DESCRIPTION => {
                info!("OSPF DD from {}", sender_router_id);
                // TODO: LLM handles DD
            }
            OSPF_TYPE_LINK_STATE_REQUEST => {
                info!("OSPF LSR from {}", sender_router_id);
                // TODO: LLM generates fake LSAs
            }
            OSPF_TYPE_LINK_STATE_UPDATE => {
                info!("OSPF LSU from {}", sender_router_id);
                // TODO: LLM logs LSAs
            }
            OSPF_TYPE_LINK_STATE_ACK => {
                trace!("OSPF LSAck from {}", sender_router_id);
            }
            _ => {
                warn!("OSPF unknown type: {}", packet_type);
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
        _status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<OspfProtocol>,
        ospf_state: Arc<OspfState>,
        server_id: crate::state::ServerId,
    ) -> Result<()> {
        if data.len() < OSPF_HEADER_LEN + 20 {
            return Err(anyhow!("Hello too short"));
        }

        // Parse Hello fields
        let network_mask = format!("{}.{}.{}.{}", data[24], data[25], data[26], data[27]);
        let hello_interval = u16::from_be_bytes([data[28], data[29]]);
        let priority = data[31];
        let router_dead_interval = u32::from_be_bytes([data[32], data[33], data[34], data[35]]);
        let dr = format!("{}.{}.{}.{}", data[36], data[37], data[38], data[39]);
        let bdr = format!("{}.{}.{}.{}", data[40], data[41], data[42], data[43]);

        // Parse neighbors
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
            "OSPF Hello from {} (priority={}, DR={}, BDR={})",
            sender_router_id, priority, dr, bdr
        );

        // Update neighbor state
        {
            let mut neighbors = ospf_state.neighbors.lock().await;
            if let Some(neighbor) = neighbors.get_mut(&sender_router_id) {
                neighbor.priority = priority;
                neighbor.dr = dr.clone();
                neighbor.bdr = bdr.clone();
                neighbor.last_hello = Instant::now();

                // State transitions
                match neighbor.state {
                    OspfNeighborState::Down => {
                        neighbor.state = OspfNeighborState::Init;
                        info!("OSPF neighbor {} -> Init", sender_router_id);
                    }
                    OspfNeighborState::Init => {
                        neighbor.state = OspfNeighborState::TwoWay;
                        info!("OSPF neighbor {} -> 2-Way", sender_router_id);
                    }
                    _ => {}
                }
            }
        }

        // Send structured event to LLM
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
            Ok(execution_result) => {
                // Log LLM messages
                for message in &execution_result.messages {
                    info!("{}", message);
                    let _ = _status_tx.send(format!("[INFO] {}", message));
                }

                debug!("OSPF got {} protocol results", execution_result.protocol_results.len());
                let _ = _status_tx.send(format!("[DEBUG] OSPF got {} protocol results", execution_result.protocol_results.len()));

                // Process each protocol result (OSPF packets to send)
                for protocol_result in execution_result.protocol_results {
                    // Check if this is a Custom result with OSPF action
                    if let crate::llm::actions::protocol_trait::ActionResult::Custom { name, data } = &protocol_result {
                        if name == "ospf_action" {
                            // Extract action type and destination
                            let action_type = data.get("type").and_then(|v| v.as_str()).unwrap_or("");
                            let destination_str = data.get("destination").and_then(|d| d.as_str()).unwrap_or("multicast");

                            // Build packet from structured action data (no bytes in JSON!)
                            let packet_result = match action_type {
                                "send_hello" => actions::OspfProtocol::build_hello_packet(data),
                                "send_database_description" => actions::OspfProtocol::build_database_description_packet(data),
                                "send_link_state_request" => actions::OspfProtocol::build_link_state_request_packet(data),
                                "send_link_state_update" => actions::OspfProtocol::build_link_state_update_packet(data),
                                "send_link_state_ack" => actions::OspfProtocol::build_link_state_ack_packet(data),
                                _ => {
                                    warn!("Unknown OSPF action type: {}", action_type);
                                    continue;
                                }
                            };

                            match packet_result {
                                Ok(packet) => {
                                    // Parse destination: "multicast", "dr_multicast", or IP address
                                    let dest_ip = match destination_str {
                                        "multicast" => OSPF_ALL_SPF_ROUTERS,
                                        "dr_multicast" => OSPF_ALL_DROUTERS,
                                        ip_str => {
                                            match ip_str.parse::<Ipv4Addr>() {
                                                Ok(ip) => ip,
                                                Err(_) => {
                                                    warn!("Invalid destination '{}', using multicast", ip_str);
                                                    OSPF_ALL_SPF_ROUTERS
                                                }
                                            }
                                        }
                                    };

                                    // Send packet to destination
                                    match Self::send_ospf_packet(ospf_state.socket_fd, dest_ip, &packet) {
                                        Ok(()) => {
                                            debug!("OSPF sent {} bytes to {}", packet.len(), dest_ip);
                                            let _ = _status_tx.send(format!("[DEBUG] OSPF sent {} bytes to {}", packet.len(), dest_ip));

                                            // TRACE: Log packet hex
                                            trace!("OSPF sent (hex): {}", hex::encode(&packet));
                                            let _ = _status_tx.send(format!("[TRACE] OSPF sent (hex): {}", hex::encode(&packet)));
                                        }
                                        Err(e) => {
                                            error!("Failed to send OSPF packet: {}", e);
                                            let _ = _status_tx.send(format!("✗ OSPF send error: {}", e));
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to build OSPF packet: {}", e);
                                    let _ = _status_tx.send(format!("✗ OSPF packet build error: {}", e));
                                }
                            }
                            continue;
                        }
                    }

                    // Fallback: Check for legacy Output results
                    if let Some(output_data) = protocol_result.get_all_output().first() {
                        // Send to default multicast
                        match Self::send_ospf_packet(ospf_state.socket_fd, OSPF_ALL_SPF_ROUTERS, output_data) {
                            Ok(()) => {
                                debug!("OSPF sent {} bytes to multicast (legacy)", output_data.len());
                                let _ = _status_tx.send(format!("[DEBUG] OSPF sent {} bytes to multicast (224.0.0.5)", output_data.len()));
                            }
                            Err(e) => {
                                error!("Failed to send OSPF packet: {}", e);
                                let _ = _status_tx.send(format!("✗ OSPF send error: {}", e));
                            }
                        }
                    }
                }
            }
            Err(e) => {
                error!("LLM call failed: {}", e);
                let _ = _status_tx.send(format!("✗ OSPF LLM error: {}", e));
            }
        }

        Ok(())
    }

    /// Send OSPF packet to destination
    #[cfg(feature = "ospf")]
    pub fn send_ospf_packet(
        socket_fd: i32,
        dest_ip: Ipv4Addr,
        ospf_data: &[u8],
    ) -> Result<()> {
        unsafe {
            let mut dest_addr = std::mem::zeroed::<libc::sockaddr_in>();
            #[cfg(target_os = "macos")]
            {
                dest_addr.sin_family = libc::AF_INET as libc::sa_family_t;
                dest_addr.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
            }
            #[cfg(not(target_os = "macos"))]
            {
                dest_addr.sin_family = libc::AF_INET as u16;
            }
            dest_addr.sin_port = 0; // Raw IP, no port
            dest_addr.sin_addr.s_addr = u32::from(dest_ip).to_be();

            let n = libc::sendto(
                socket_fd,
                ospf_data.as_ptr() as *const libc::c_void,
                ospf_data.len(),
                0,
                &dest_addr as *const _ as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in>() as u32,
            );

            if n < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        }

        Ok(())
    }
}
