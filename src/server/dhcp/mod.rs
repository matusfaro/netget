//! DHCP server implementation
pub mod actions;

use crate::server::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use actions::DHCP_REQUEST_EVENT;
use crate::server::DhcpProtocol;
use crate::protocol::Event;
use crate::state::app_state::AppState;

#[cfg(feature = "dhcp")]
use dhcproto::{v4, Decodable, Decoder};
#[cfg(feature = "dhcp")]
use actions::DhcpRequestContext;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// DHCP server that forwards requests to LLM
pub struct DhcpServer;

impl DhcpServer {
    /// Spawn DHCP server with integrated LLM handling
    pub async fn spawn_with_llm(
        listen_addr: SocketAddr,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<SocketAddr> {
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        info!("DHCP server listening on {}", local_addr);

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 1500]; // Standard MTU size

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();

                        // DEBUG: Log summary
                        console_debug!(status_tx, "DHCP received {} bytes from {}", n, peer_addr);

                        // TRACE: Log full payload (always hex for DHCP)
                        let hex_str = hex::encode(&data);
                        console_trace!(status_tx, "DHCP data (hex): {}", hex_str);

                        // Legacy method - no longer supported
                        error!("DHCP legacy spawn_with_llm is deprecated, use spawn_with_llm_actions");
                        let _ = status_tx.send(
                            "✗ DHCP legacy mode no longer supported, please restart with action-based mode".to_string()
                        );
                    }
                    Err(e) => {
                        error!("DHCP receive error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Spawn DHCP server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        info!("DHCP server (action-based) listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] DHCP server listening on {}", local_addr));

        let protocol = Arc::new(DhcpProtocol::new());

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 1500];

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);

                        // DEBUG: Log summary
                        console_debug!(status_tx, "DHCP received {} bytes from {}", n, peer_addr);

                        // TRACE: Log full payload (always hex for DHCP)
                        let hex_str = hex::encode(&data);
                        console_trace!(status_tx, "DHCP data (hex): {}", hex_str);

                        #[cfg(feature = "dhcp")]
                        let parsed_info = Self::parse_dhcp_message(&data);

                        #[cfg(not(feature = "dhcp"))]
                        let parsed_info: Option<(String, Option<DhcpRequestContext>)> = None;

                        // Add connection to ServerInstance
                        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
                        let now = std::time::Instant::now();

                        #[cfg(feature = "dhcp")]
                        let _request_type = parsed_info.as_ref()
                            .map(|(desc, _)| desc.clone())
                            .unwrap_or_else(|| "unknown".to_string());

                        #[cfg(not(feature = "dhcp"))]
                        let request_type = "request".to_string();

                        let conn_state = ServerConnectionState {
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
                            protocol_info: ProtocolConnectionInfo::empty(),
                        };
                        app_state.add_connection_to_server(server_id, conn_state).await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            #[cfg(feature = "dhcp")]
                            if let Some((_, context)) = parsed_info.as_ref() {
                                if let Some(ctx) = context {
                                    protocol_clone.set_request_context(ctx.clone());
                                }
                            }

                            // Extract event data
                            #[cfg(feature = "dhcp")]
                            let (message_type, client_mac, requested_ip) = if let Some((_, Some(ctx))) = &parsed_info {
                                (
                                    format!("{:?}", ctx.message_type),
                                    hex::encode(&ctx.chaddr),
                                    ctx.requested_ip.map(|ip| ip.to_string())
                                )
                            } else {
                                ("unknown".to_string(), "unknown".to_string(), None)
                            };

                            #[cfg(not(feature = "dhcp"))]
                            let (message_type, client_mac, requested_ip) = ("unknown".to_string(), "unknown".to_string(), None::<String>);

                            let mut event_data = serde_json::json!({
                                "message_type": message_type,
                                "client_mac": client_mac
                            });
                            if let Some(ip) = requested_ip {
                                event_data["requested_ip"] = serde_json::json!(ip);
                            }

                            let event = Event::new(&DHCP_REQUEST_EVENT, event_data);

                            debug!("DHCP calling LLM for request from {}", peer_addr);
                            let _ = status_clone.send(format!("[DEBUG] DHCP calling LLM for request from {}", peer_addr));

                            match call_llm(
                                &llm_clone,
                                &state_clone,
                                server_id,
                                None,
                                &event,
                                protocol_clone.as_ref(),
                            ).await {
                                Ok(execution_result) => {
                                    for message in &execution_result.messages {
                                        info!("{}", message);
                                        let _ = status_clone.send(format!("[INFO] {}", message));
                                    }

                                    debug!("DHCP got {} protocol results", execution_result.protocol_results.len());
                                    let _ = status_clone.send(format!("[DEBUG] DHCP got {} protocol results", execution_result.protocol_results.len()));

                                    for protocol_result in execution_result.protocol_results {
                                                        if let Some(output_data) = protocol_result.get_all_output().first() {
                                                            let _ = socket_clone.send_to(output_data, peer_addr).await;

                                                            // DEBUG: Log summary
                                                            debug!("DHCP sent {} bytes to {}", output_data.len(), peer_addr);
                                                            let _ = status_clone.send(format!("[DEBUG] DHCP sent {} bytes to {}", output_data.len(), peer_addr));

                                                            // TRACE: Log full payload
                                                            let hex_str = hex::encode(output_data);
                                                            trace!("DHCP sent (hex): {}", hex_str);
                                                            let _ = status_clone.send(format!("[TRACE] DHCP sent (hex): {}", hex_str));

                                                            let _ = status_clone.send(format!("→ DHCP response to {} ({} bytes)", peer_addr, output_data.len()));
                                        } else {
                                            debug!("DHCP protocol result has no output data");
                                            let _ = status_clone.send("[DEBUG] DHCP protocol result has no output data".to_string());
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("DHCP LLM call failed: {}", e);
                                    let _ = status_clone.send(format!("✗ DHCP LLM error: {}", e));
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("DHCP receive error: {}", e);
                        let _ = status_tx.send(format!("✗ DHCP receive error: {}", e));
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    #[cfg(feature = "dhcp")]
    fn parse_dhcp_message(data: &[u8]) -> Option<(String, Option<DhcpRequestContext>)> {
        use std::net::Ipv4Addr;

        match v4::Message::decode(&mut Decoder::new(data)) {
            Ok(msg) => {
                // Extract message type from options
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

                // Extract requested IP from options if present
                let requested_ip = msg.opts().get(v4::OptionCode::RequestedIpAddress)
                    .and_then(|opt| {
                        if let v4::DhcpOption::RequestedIpAddress(ip) = opt {
                            Some(*ip)
                        } else {
                            None
                        }
                    });

                // Build human-readable description
                let mac_str = hex::encode(msg.chaddr());
                let mut description = format!(
                    "DHCP {} from client MAC {} (transaction ID: 0x{:08x})",
                    message_type_str, mac_str, msg.xid()
                );

                if msg.ciaddr() != Ipv4Addr::UNSPECIFIED {
                    description.push_str(&format!(", client IP: {}", msg.ciaddr()));
                }

                if let Some(req_ip) = requested_ip {
                    description.push_str(&format!(", requested IP: {}", req_ip));
                }

                // Create context for action execution
                let context = message_type.map(|mt| DhcpRequestContext {
                    xid: msg.xid(),
                    chaddr: msg.chaddr().to_vec(),
                    message_type: mt,
                    ciaddr: msg.ciaddr(),
                    requested_ip,
                });

                Some((description, context))
            }
            Err(e) => {
                tracing::warn!("Failed to parse DHCP message: {}", e);
                None
            }
        }
    }
}
