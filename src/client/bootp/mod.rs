//! BOOTP client implementation
pub mod actions;

pub use actions::BootpClientProtocol;

use anyhow::{Context, Result};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, trace, warn};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::bootp::actions::{BOOTP_CLIENT_CONNECTED_EVENT, BOOTP_REPLY_RECEIVED_EVENT};

#[cfg(feature = "bootp")]
use dhcproto::v4;

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
    queued_replies: Vec<BootpReply>,
    memory: String,
}

/// Parsed BOOTP reply data
#[derive(Debug, Clone)]
struct BootpReply {
    assigned_ip: Ipv4Addr,
    server_ip: Ipv4Addr,
    gateway_ip: Ipv4Addr,
    boot_filename: String,
}

/// BOOTP client that sends requests to a BOOTP/DHCP server
pub struct BootpClient;

impl BootpClient {
    /// Connect to a BOOTP server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Parse server address
        let server_addr: SocketAddr = remote_addr
            .parse()
            .context("Invalid BOOTP server address")?;

        // Bind UDP socket to BOOTP client port (68)
        // Note: Binding to port 68 may require elevated privileges
        let socket = match UdpSocket::bind("0.0.0.0:68").await {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to bind to port 68 (requires privileges): {}. Binding to random port.", e);
                UdpSocket::bind("0.0.0.0:0").await
                    .context("Failed to bind UDP socket")?
            }
        };

        // Enable broadcast if needed
        socket.set_broadcast(true)
            .context("Failed to enable broadcast")?;

        let local_addr = socket.local_addr()?;

        info!("BOOTP client {} bound to {} (server: {})", client_id, local_addr, server_addr);

        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] BOOTP client {} connected", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        let socket_arc = Arc::new(socket);

        // Initialize client data
        let client_data = Arc::new(Mutex::new(ClientData {
            state: ConnectionState::Idle,
            queued_replies: Vec::new(),
            memory: String::new(),
        }));

        // Clone for spawned task
        let socket_clone = Arc::clone(&socket_arc);
        let app_state_clone = Arc::clone(&app_state);
        let status_tx_clone = status_tx.clone();

        // Send initial connected event to LLM
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::bootp::actions::BootpClientProtocol::new());
            let event = Event::new(
                &BOOTP_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "server_addr": server_addr.to_string(),
                }),
            );

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
                        if let Err(e) = Self::execute_bootp_action(
                            action,
                            &socket_arc,
                            server_addr,
                            &protocol,
                            &status_tx,
                            client_id,
                        ).await {
                            error!("Failed to execute initial BOOTP action: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("Initial LLM call failed for BOOTP client {}: {}", client_id, e);
                }
            }
        }

        // Spawn receive loop
        tokio::spawn(async move {
            let mut buffer = vec![0u8; 1500]; // BOOTP packets are typically ~300 bytes

            loop {
                match socket_clone.recv_from(&mut buffer).await {
                    Ok((n, peer)) => {
                        trace!("BOOTP client {} received {} bytes from {}", client_id, n, peer);

                        // Parse BOOTP reply
                        match Self::parse_bootp_reply(&buffer[..n]) {
                            Ok(reply) => {
                                info!(
                                    "BOOTP client {} received reply: IP={}, Server={}, Boot={}",
                                    client_id, reply.assigned_ip, reply.server_ip, reply.boot_filename
                                );

                                // Handle reply with LLM
                                let mut client_data_lock = client_data.lock().await;

                                match client_data_lock.state {
                                    ConnectionState::Idle => {
                                        // Process immediately
                                        client_data_lock.state = ConnectionState::Processing;
                                        drop(client_data_lock);

                                        // Call LLM
                                        if let Some(instruction) = app_state_clone.get_instruction_for_client(client_id).await {
                                            let protocol = Arc::new(crate::client::bootp::actions::BootpClientProtocol::new());
                                            let event = Event::new(
                                                &BOOTP_REPLY_RECEIVED_EVENT,
                                                serde_json::json!({
                                                    "assigned_ip": reply.assigned_ip.to_string(),
                                                    "server_ip": reply.server_ip.to_string(),
                                                    "boot_filename": reply.boot_filename,
                                                    "gateway_ip": reply.gateway_ip.to_string(),
                                                }),
                                            );

                                            match call_llm_for_client(
                                                &llm_client,
                                                &app_state_clone,
                                                client_id.to_string(),
                                                &instruction,
                                                &client_data.lock().await.memory,
                                                Some(&event),
                                                protocol.as_ref(),
                                                &status_tx_clone,
                                            ).await {
                                                Ok(ClientLlmResult { actions, memory_updates }) => {
                                                    // Update memory
                                                    if let Some(mem) = memory_updates {
                                                        client_data.lock().await.memory = mem;
                                                    }

                                                    // Execute actions
                                                    for action in actions {
                                                        if let Err(e) = Self::execute_bootp_action(
                                                            action,
                                                            &socket_clone,
                                                            server_addr,
                                                            &protocol,
                                                            &status_tx_clone,
                                                            client_id,
                                                        ).await {
                                                            error!("Failed to execute BOOTP action: {}", e);
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    error!("LLM error for BOOTP client {}: {}", client_id, e);
                                                }
                                            }
                                        }

                                        // Process queued replies if any
                                        let mut client_data_lock = client_data.lock().await;
                                        if !client_data_lock.queued_replies.is_empty() {
                                            client_data_lock.queued_replies.clear();
                                        }
                                        client_data_lock.state = ConnectionState::Idle;
                                    }
                                    ConnectionState::Processing => {
                                        // Queue reply
                                        client_data_lock.queued_replies.push(reply);
                                        client_data_lock.state = ConnectionState::Accumulating;
                                    }
                                    ConnectionState::Accumulating => {
                                        // Continue queuing
                                        client_data_lock.queued_replies.push(reply);
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("Failed to parse BOOTP reply: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("BOOTP client {} recv error: {}", client_id, e);
                        app_state_clone.update_client_status(client_id, ClientStatus::Error(e.to_string())).await;
                        let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Execute a BOOTP client action
    async fn execute_bootp_action(
        action: serde_json::Value,
        socket: &Arc<UdpSocket>,
        server_addr: SocketAddr,
        protocol: &Arc<BootpClientProtocol>,
        status_tx: &mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<()> {
        use crate::llm::actions::client_trait::{Client, ClientActionResult};

        match protocol.as_ref().execute_action(action)? {
            ClientActionResult::Custom { name, data } if name == "send_bootp_request" => {
                let client_mac = data["client_mac"]
                    .as_str()
                    .context("Missing client_mac")?;
                let broadcast = data["broadcast"]
                    .as_bool()
                    .unwrap_or(true);

                // Parse MAC address
                let mac_bytes = Self::parse_mac(client_mac)?;

                // Build BOOTP request
                let bootp_request = Self::build_bootp_request(&mac_bytes)?;

                // Send to server or broadcast
                let target = if broadcast {
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), server_addr.port())
                } else {
                    server_addr
                };

                socket.send_to(&bootp_request, target).await?;
                trace!("BOOTP client {} sent request to {}", client_id, target);
                let _ = status_tx.send(format!("[CLIENT] BOOTP request sent to {}", target));
            }
            ClientActionResult::Disconnect => {
                info!("BOOTP client {} disconnecting", client_id);
                // UDP is connectionless, nothing to do
            }
            ClientActionResult::WaitForMore => {
                // Do nothing, just wait
            }
            _ => {}
        }

        Ok(())
    }

    /// Parse MAC address from string format (00:11:22:33:44:55)
    fn parse_mac(mac_str: &str) -> Result<[u8; 6]> {
        let parts: Vec<&str> = mac_str.split(':').collect();
        if parts.len() != 6 {
            return Err(anyhow::anyhow!("Invalid MAC address format"));
        }

        let mut mac = [0u8; 6];
        for (i, part) in parts.iter().enumerate() {
            mac[i] = u8::from_str_radix(part, 16)
                .context("Invalid hex digit in MAC address")?;
        }

        Ok(mac)
    }

    /// Build a BOOTP request message
    fn build_bootp_request(client_mac: &[u8; 6]) -> Result<Vec<u8>> {
        #[cfg(feature = "bootp")]
        {
            let mut msg = v4::Message::new(
                Ipv4Addr::UNSPECIFIED, // ciaddr (client IP)
                Ipv4Addr::UNSPECIFIED, // yiaddr (your IP)
                Ipv4Addr::UNSPECIFIED, // siaddr (server IP)
                Ipv4Addr::UNSPECIFIED, // giaddr (gateway IP)
                client_mac,
            );
            msg.set_opcode(v4::Opcode::BootRequest);
            msg.set_flags(v4::Flags::default().set_broadcast()); // Request broadcast reply

            // Generate random transaction ID
            msg.set_xid(rand::random::<u32>());

            use dhcproto::Encodable;
            msg.to_vec().context("Failed to encode BOOTP request")
        }

        #[cfg(not(feature = "bootp"))]
        {
            Err(anyhow::anyhow!("BOOTP feature not enabled"))
        }
    }

    /// Parse a BOOTP reply message
    fn parse_bootp_reply(data: &[u8]) -> Result<BootpReply> {
        #[cfg(feature = "bootp")]
        {
            use dhcproto::Decodable;
            use dhcproto::decoder::Decoder;

            let mut decoder = Decoder::new(data);
            let msg = v4::Message::decode(&mut decoder)
                .context("Failed to decode BOOTP reply")?;

            // Verify it's a reply
            if msg.opcode() != v4::Opcode::BootReply {
                return Err(anyhow::anyhow!("Not a BOOTP reply"));
            }

            let boot_filename = msg.fname()
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
                .unwrap_or("")
                .trim_end_matches('\0')
                .to_string();

            Ok(BootpReply {
                assigned_ip: msg.yiaddr(),
                server_ip: msg.siaddr(),
                gateway_ip: msg.giaddr(),
                boot_filename,
            })
        }

        #[cfg(not(feature = "bootp"))]
        {
            Err(anyhow::anyhow!("BOOTP feature not enabled"))
        }
    }
}
