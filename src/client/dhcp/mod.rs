//! DHCP client implementation
pub mod actions;

pub use actions::DhcpClientProtocol;

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};

use actions::{DHCP_CLIENT_CONNECTED_EVENT, DHCP_CLIENT_RESPONSE_RECEIVED_EVENT};

#[cfg(feature = "dhcp")]
use dhcproto::{v4, Decodable, Decoder};

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
    memory: String,
}

/// DHCP client that sends requests and processes responses via LLM
pub struct DhcpClient;

impl DhcpClient {
    /// Connect to DHCP server with LLM-controlled actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Parse remote_addr (DHCP server address)
        let server_addr: SocketAddr = remote_addr.parse()
            .context("Invalid DHCP server address")?;

        // Bind to DHCP client port (68)
        // Note: This may require elevated privileges
        let socket = Arc::new(UdpSocket::bind("0.0.0.0:68").await
            .context("Failed to bind to DHCP client port 68 (may need elevated privileges)")?);

        // Enable broadcast
        socket.set_broadcast(true)?;

        let local_addr = socket.local_addr()?;

        info!("DHCP client {} bound to {}, targeting server {}", client_id, local_addr, server_addr);
        let _ = status_tx.send(format!("[CLIENT] DHCP client {} connected, targeting server {}", client_id, server_addr));

        // Update client status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Initialize client data
        let client_data = Arc::new(Mutex::new(ClientData {
            state: ConnectionState::Idle,
            memory: String::new(),
        }));

        // Spawn event: dhcp_connected
        let event = Event::new(&DHCP_CLIENT_CONNECTED_EVENT, serde_json::json!({
            "server_addr": server_addr.to_string(),
            "local_addr": local_addr.to_string()
        }));

        debug!("DHCP client {} calling LLM for connected event", client_id);

        // Call LLM for initial connection event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(DhcpClientProtocol::new());

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

                    // Execute actions from LLM response
                    for action in actions {
                        use crate::llm::actions::client_trait::Client;

                        match protocol.as_ref().execute_action(action) {
                            Ok(action_result) => {
                                use crate::llm::actions::client_trait::ClientActionResult;

                                match action_result {
                                    ClientActionResult::Custom { name, data } => {
                                        if name == "dhcp_discover" {
                                            #[cfg(feature = "dhcp")]
                                            {
                                                if let Ok(discover_packet) = Self::build_discover_packet(&data) {
                                                    let target = if data.get("broadcast").and_then(|v| v.as_bool()).unwrap_or(true) {
                                                        "255.255.255.255:67".parse().unwrap()
                                                    } else {
                                                        server_addr
                                                    };

                                                    let _ = socket.send_to(&discover_packet, target).await;
                                                    debug!("DHCP client {} sent DISCOVER ({} bytes)", client_id, discover_packet.len());
                                                    trace!("DHCP DISCOVER (hex): {}", hex::encode(&discover_packet));
                                                }
                                            }

                                            #[cfg(not(feature = "dhcp"))]
                                            {
                                                error!("DHCP feature not enabled");
                                            }
                                        } else if name == "dhcp_request" {
                                            #[cfg(feature = "dhcp")]
                                            {
                                                if let Ok(request_packet) = Self::build_request_packet(&data) {
                                                    let target = if data.get("broadcast").and_then(|v| v.as_bool()).unwrap_or(true) {
                                                        "255.255.255.255:67".parse().unwrap()
                                                    } else {
                                                        server_addr
                                                    };

                                                    let _ = socket.send_to(&request_packet, target).await;
                                                    debug!("DHCP client {} sent REQUEST ({} bytes)", client_id, request_packet.len());
                                                    trace!("DHCP REQUEST (hex): {}", hex::encode(&request_packet));
                                                }
                                            }

                                            #[cfg(not(feature = "dhcp"))]
                                            {
                                                error!("DHCP feature not enabled");
                                            }
                                        }
                                    }
                                    ClientActionResult::Disconnect => {
                                        info!("DHCP client {} disconnecting", client_id);
                                        return Ok(local_addr);
                                    }
                                    _ => {}
                                }
                            }
                            Err(e) => {
                                error!("DHCP client {} action execution failed: {}", client_id, e);
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("DHCP client {} LLM call failed: {}", client_id, e);
                }
            }
        }

        // Spawn receive loop
        let socket_clone = socket.clone();
        let llm_clone = llm_client.clone();
        let state_clone = app_state.clone();
        let status_clone = status_tx.clone();
        let client_data_clone = client_data.clone();

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 1500];

            loop {
                match socket_clone.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();

                        debug!("DHCP client {} received {} bytes from {}", client_id, n, peer_addr);
                        trace!("DHCP response (hex): {}", hex::encode(&data));

                        // Handle data with LLM
                        let mut client_data_lock = client_data_clone.lock().await;

                        match client_data_lock.state {
                            ConnectionState::Idle => {
                                // Process immediately
                                client_data_lock.state = ConnectionState::Processing;
                                drop(client_data_lock);

                                // Parse DHCP response
                                #[cfg(feature = "dhcp")]
                                let parsed_info = Self::parse_dhcp_response(&data);

                                #[cfg(not(feature = "dhcp"))]
                                let parsed_info: Option<(String, serde_json::Value)> = None;

                                let event_data = if let Some((message_type, details)) = parsed_info {
                                    serde_json::json!({
                                        "message_type": message_type,
                                        "details": details
                                    })
                                } else {
                                    serde_json::json!({
                                        "message_type": "unknown",
                                        "data_hex": hex::encode(&data),
                                        "data_length": n
                                    })
                                };

                                let event = Event::new(&DHCP_CLIENT_RESPONSE_RECEIVED_EVENT, event_data);

                                // Call LLM with response event
                                if let Some(instruction) = state_clone.get_instruction_for_client(client_id).await {
                                    let protocol = Arc::new(DhcpClientProtocol::new());

                                    match call_llm_for_client(
                                        &llm_clone,
                                        &state_clone,
                                        client_id.to_string(),
                                        &instruction,
                                        &client_data_clone.lock().await.memory,
                                        Some(&event),
                                        protocol.as_ref(),
                                        &status_clone,
                                    ).await {
                                        Ok(ClientLlmResult { actions, memory_updates }) => {
                                            // Update memory
                                            if let Some(mem) = memory_updates {
                                                client_data_clone.lock().await.memory = mem;
                                            }

                                            // Execute actions from LLM
                                            for action in actions {
                                                use crate::llm::actions::client_trait::Client;

                                                match protocol.as_ref().execute_action(action) {
                                                    Ok(action_result) => {
                                                        use crate::llm::actions::client_trait::ClientActionResult;

                                                        match action_result {
                                                            ClientActionResult::Custom { name, data: action_data } => {
                                                                if name == "dhcp_discover" {
                                                                    #[cfg(feature = "dhcp")]
                                                                    {
                                                                        if let Ok(discover_packet) = Self::build_discover_packet(&action_data) {
                                                                            let target = if action_data.get("broadcast").and_then(|v| v.as_bool()).unwrap_or(true) {
                                                                                "255.255.255.255:67".parse().unwrap()
                                                                            } else {
                                                                                peer_addr
                                                                            };

                                                                            let _ = socket_clone.send_to(&discover_packet, target).await;
                                                                            debug!("DHCP client {} sent DISCOVER", client_id);
                                                                        }
                                                                    }
                                                                } else if name == "dhcp_request" {
                                                                    #[cfg(feature = "dhcp")]
                                                                    {
                                                                        if let Ok(request_packet) = Self::build_request_packet(&action_data) {
                                                                            let target = if action_data.get("broadcast").and_then(|v| v.as_bool()).unwrap_or(true) {
                                                                                "255.255.255.255:67".parse().unwrap()
                                                                            } else {
                                                                                peer_addr
                                                                            };

                                                                            let _ = socket_clone.send_to(&request_packet, target).await;
                                                                            debug!("DHCP client {} sent REQUEST", client_id);
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                            ClientActionResult::Disconnect => {
                                                                info!("DHCP client {} disconnecting", client_id);
                                                                state_clone.update_client_status(client_id, ClientStatus::Disconnected).await;
                                                                let _ = status_clone.send("__UPDATE_UI__".to_string());
                                                                break;
                                                            }
                                                            ClientActionResult::WaitForMore => {
                                                                debug!("DHCP client {} waiting for more data", client_id);
                                                            }
                                                            _ => {}
                                                        }
                                                    }
                                                    Err(e) => {
                                                        error!("DHCP client {} action execution failed: {}", client_id, e);
                                                    }
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            error!("DHCP client {} LLM call failed: {}", client_id, e);
                                        }
                                    }
                                }

                                // Reset to idle state
                                client_data_clone.lock().await.state = ConnectionState::Idle;
                            }
                            ConnectionState::Processing | ConnectionState::Accumulating => {
                                // Queue data for later processing
                                debug!("DHCP client {} is processing, queueing response", client_id);
                            }
                        }
                    }
                    Err(e) => {
                        error!("DHCP client {} receive error: {}", client_id, e);
                        state_clone.update_client_status(client_id, ClientStatus::Error(e.to_string())).await;
                        let _ = status_clone.send("__UPDATE_UI__".to_string());
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    #[cfg(feature = "dhcp")]
    fn build_discover_packet(params: &serde_json::Value) -> Result<Vec<u8>> {
        use dhcproto::Encodable;
        use std::net::Ipv4Addr;

        // Generate random transaction ID
        let xid = rand::random::<u32>();

        // Get MAC address from params or generate random one
        let mac_str = params.get("mac_address")
            .and_then(|v| v.as_str())
            .unwrap_or("00:00:00:00:00:00");

        let chaddr = Self::parse_mac_address(mac_str)?;

        // Build DHCP DISCOVER
        let mut msg = v4::Message::default();
        msg.set_opcode(v4::Opcode::BootRequest)
            .set_xid(xid)
            .set_flags(v4::Flags::default().set_broadcast())
            .set_chaddr(&chaddr);

        // Add DHCP options
        msg.opts_mut().insert(v4::DhcpOption::MessageType(v4::MessageType::Discover));

        // Optional: requested IP
        if let Some(requested_ip) = params.get("requested_ip").and_then(|v| v.as_str()) {
            if let Ok(ip) = requested_ip.parse::<Ipv4Addr>() {
                msg.opts_mut().insert(v4::DhcpOption::RequestedIpAddress(ip));
            }
        }

        // Encode to bytes
        let bytes = msg.to_vec()?;
        Ok(bytes)
    }

    #[cfg(feature = "dhcp")]
    fn build_request_packet(params: &serde_json::Value) -> Result<Vec<u8>> {
        use dhcproto::Encodable;
        use std::net::Ipv4Addr;

        // Generate random transaction ID
        let xid = rand::random::<u32>();

        // Get MAC address
        let mac_str = params.get("mac_address")
            .and_then(|v| v.as_str())
            .unwrap_or("00:00:00:00:00:00");

        let chaddr = Self::parse_mac_address(mac_str)?;

        // Get requested IP (required for REQUEST)
        let requested_ip = params.get("requested_ip")
            .and_then(|v| v.as_str())
            .context("Missing 'requested_ip' parameter")?
            .parse::<Ipv4Addr>()?;

        // Get server IP (optional)
        let server_ip = params.get("server_ip")
            .and_then(|v| v.as_str())
            .map(|s| s.parse::<Ipv4Addr>())
            .transpose()?;

        // Build DHCP REQUEST
        let mut msg = v4::Message::default();
        msg.set_opcode(v4::Opcode::BootRequest)
            .set_xid(xid)
            .set_flags(v4::Flags::default().set_broadcast())
            .set_chaddr(&chaddr);

        // Add DHCP options
        msg.opts_mut().insert(v4::DhcpOption::MessageType(v4::MessageType::Request));
        msg.opts_mut().insert(v4::DhcpOption::RequestedIpAddress(requested_ip));

        if let Some(server) = server_ip {
            msg.opts_mut().insert(v4::DhcpOption::ServerIdentifier(server));
        }

        // Encode to bytes
        let bytes = msg.to_vec()?;
        Ok(bytes)
    }

    #[cfg(feature = "dhcp")]
    fn parse_mac_address(mac_str: &str) -> Result<Vec<u8>> {
        let parts: Vec<&str> = mac_str.split(':').collect();
        if parts.len() != 6 {
            anyhow::bail!("Invalid MAC address format: {}", mac_str);
        }

        let mut mac = Vec::with_capacity(16); // DHCP chaddr is 16 bytes
        for part in parts {
            let byte = u8::from_str_radix(part, 16)
                .context("Invalid hex in MAC address")?;
            mac.push(byte);
        }

        // Pad to 16 bytes (DHCP chaddr field)
        while mac.len() < 16 {
            mac.push(0);
        }

        Ok(mac)
    }

    #[cfg(feature = "dhcp")]
    fn parse_dhcp_response(data: &[u8]) -> Option<(String, serde_json::Value)> {
        use std::net::Ipv4Addr;

        match v4::Message::decode(&mut Decoder::new(data)) {
            Ok(msg) => {
                // Extract message type
                let message_type = msg.opts().get(v4::OptionCode::MessageType)
                    .and_then(|opt| {
                        if let v4::DhcpOption::MessageType(mt) = opt {
                            Some(*mt)
                        } else {
                            None
                        }
                    });

                let message_type_str = message_type.as_ref()
                    .map(|mt| format!("{:?}", mt))
                    .unwrap_or_else(|| "Unknown".to_string());

                // Extract key fields
                let offered_ip = msg.yiaddr();
                let server_ip = msg.opts().get(v4::OptionCode::ServerIdentifier)
                    .and_then(|opt| {
                        if let v4::DhcpOption::ServerIdentifier(ip) = opt {
                            Some(*ip)
                        } else {
                            None
                        }
                    });

                let subnet_mask = msg.opts().get(v4::OptionCode::SubnetMask)
                    .and_then(|opt| {
                        if let v4::DhcpOption::SubnetMask(mask) = opt {
                            Some(*mask)
                        } else {
                            None
                        }
                    });

                let router = msg.opts().get(v4::OptionCode::Router)
                    .and_then(|opt| {
                        if let v4::DhcpOption::Router(routers) = opt {
                            routers.first().copied()
                        } else {
                            None
                        }
                    });

                let dns_servers = msg.opts().get(v4::OptionCode::DomainNameServer)
                    .and_then(|opt| {
                        if let v4::DhcpOption::DomainNameServer(dns) = opt {
                            Some(dns.clone())
                        } else {
                            None
                        }
                    });

                let lease_time = msg.opts().get(v4::OptionCode::AddressLeaseTime)
                    .and_then(|opt| {
                        if let v4::DhcpOption::AddressLeaseTime(time) = opt {
                            Some(*time)
                        } else {
                            None
                        }
                    });

                // Build details JSON
                let mut details = serde_json::json!({
                    "transaction_id": format!("0x{:08x}", msg.xid()),
                    "client_mac": hex::encode(msg.chaddr())
                });

                if offered_ip != Ipv4Addr::UNSPECIFIED {
                    details["offered_ip"] = serde_json::json!(offered_ip.to_string());
                }

                if let Some(server) = server_ip {
                    details["server_ip"] = serde_json::json!(server.to_string());
                }

                if let Some(mask) = subnet_mask {
                    details["subnet_mask"] = serde_json::json!(mask.to_string());
                }

                if let Some(gw) = router {
                    details["router"] = serde_json::json!(gw.to_string());
                }

                if let Some(dns) = dns_servers {
                    let dns_strs: Vec<String> = dns.iter().map(|ip| ip.to_string()).collect();
                    details["dns_servers"] = serde_json::json!(dns_strs);
                }

                if let Some(lease) = lease_time {
                    details["lease_time"] = serde_json::json!(lease);
                }

                Some((message_type_str, details))
            }
            Err(e) => {
                tracing::warn!("Failed to parse DHCP response: {}", e);
                None
            }
        }
    }
}
