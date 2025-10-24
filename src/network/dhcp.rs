//! DHCP server implementation

use crate::network::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::ollama_client::OllamaClient;
use crate::llm::prompt::PromptBuilder;
use crate::llm::{ActionResponse, execute_actions, ProtocolActions};
use crate::network::DhcpProtocol;
use crate::state::app_state::AppState;

#[cfg(feature = "dhcp")]
use dhcproto::{v4, Decodable, Decoder};
#[cfg(feature = "dhcp")]
use crate::network::dhcp_actions::DhcpRequestContext;

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
                        debug!("DHCP received {} bytes from {}", n, peer_addr);
                        let _ = status_tx.send(format!("[DEBUG] DHCP received {} bytes from {}", n, peer_addr));

                        // TRACE: Log full payload (always hex for DHCP)
                        let hex_str = hex::encode(&data);
                        trace!("DHCP data (hex): {}", hex_str);
                        let _ = status_tx.send(format!("[TRACE] DHCP data (hex): {}", hex_str));

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
                        let connection_id = ConnectionId::new();

                        // DEBUG: Log summary
                        debug!("DHCP received {} bytes from {}", n, peer_addr);
                        let _ = status_tx.send(format!("[DEBUG] DHCP received {} bytes from {}", n, peer_addr));

                        // TRACE: Log full payload (always hex for DHCP)
                        let hex_str = hex::encode(&data);
                        trace!("DHCP data (hex): {}", hex_str);
                        let _ = status_tx.send(format!("[TRACE] DHCP data (hex): {}", hex_str));

                        #[cfg(feature = "dhcp")]
                        let parsed_info = Self::parse_dhcp_message(&data);

                        #[cfg(not(feature = "dhcp"))]
                        let parsed_info: Option<(String, Option<DhcpRequestContext>)> = None;

                        // Add connection to ServerInstance
                        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
                        let now = std::time::Instant::now();

                        #[cfg(feature = "dhcp")]
                        let request_type = parsed_info.as_ref()
                            .map(|(desc, _)| desc.clone())
                            .unwrap_or_else(|| "unknown".to_string());

                        #[cfg(not(feature = "dhcp"))]
                        let request_type = "request".to_string();

                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr: peer_addr,
                            local_addr: local_addr,
                            bytes_sent: 0,
                            bytes_received: n as u64,
                            packets_sent: 0,
                            packets_received: 1,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::Dhcp {
                                recent_requests: vec![(request_type, now)],
                            },
                        };
                        app_state.add_connection_to_server(server_id, conn_state).await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            let model = state_clone.get_ollama_model().await;

                            #[cfg(feature = "dhcp")]
                            let event_description = if let Some((desc, context)) = parsed_info {
                                // Set the request context so actions can access it
                                if let Some(ctx) = context {
                                    protocol_clone.set_request_context(ctx);
                                }
                                desc
                            } else {
                                format!("DHCP request from {} ({} bytes). Warning: Could not parse DHCP message", peer_addr, data.len())
                            };

                            #[cfg(not(feature = "dhcp"))]
                            let event_description = format!("DHCP request from {} ({} bytes)", peer_addr, data.len());

                            let protocol_actions = protocol_clone.get_sync_actions();
                            let prompt = PromptBuilder::build_network_event_action_prompt(
                                &state_clone, &event_description, protocol_actions).await;

                            // DEBUG: Log LLM prompt
                            debug!("DHCP LLM request ({} chars)", prompt.len());
                            let _ = status_clone.send(format!("[DEBUG] DHCP LLM request ({} chars)", prompt.len()));

                            // TRACE: Log full prompt
                            trace!("DHCP LLM prompt:\n{}", prompt);
                            let _ = status_clone.send(format!("[TRACE] DHCP LLM prompt:\n{}", prompt));

                            match llm_clone.generate(&model, &prompt).await {
                                Ok(llm_output) => {
                                    // DEBUG: Log LLM response summary
                                    debug!("DHCP LLM response ({} chars)", llm_output.len());
                                    let _ = status_clone.send(format!("[DEBUG] DHCP LLM response ({} chars)", llm_output.len()));

                                    // TRACE: Log full LLM response
                                    trace!("DHCP LLM raw output:\n{}", llm_output);
                                    let _ = status_clone.send(format!("[TRACE] DHCP LLM raw output:\n{}", llm_output));

                                    match ActionResponse::from_str(&llm_output) {
                                        Ok(action_response) => {
                                            debug!("DHCP parsed {} actions", action_response.actions.len());
                                            let _ = status_clone.send(format!("[DEBUG] DHCP parsed {} actions", action_response.actions.len()));

                                            match execute_actions(action_response.actions, &state_clone,
                                                Some(protocol_clone.as_ref())).await {
                                                Ok(result) => {
                                                    for msg in result.messages {
                                                        let _ = status_clone.send(msg);
                                                    }

                                                    debug!("DHCP got {} protocol results", result.protocol_results.len());
                                                    let _ = status_clone.send(format!("[DEBUG] DHCP got {} protocol results", result.protocol_results.len()));

                                                    for protocol_result in result.protocol_results {
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
                                                    error!("DHCP action execution error: {}", e);
                                                    let _ = status_clone.send(format!("✗ DHCP action execution error: {}", e));
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            error!("DHCP failed to parse LLM response as actions: {}", e);
                                            let _ = status_clone.send(format!("✗ DHCP failed to parse LLM response: {}", e));
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("DHCP LLM generation error: {}", e);
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