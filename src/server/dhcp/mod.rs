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
use crate::protocol::Event;
use crate::server::DhcpProtocol;
use crate::state::app_state::AppState;
use actions::DHCP_REQUEST_EVENT;

use crate::{console_debug, console_trace};
use actions::DhcpRequestContext;
use dhcproto::{v4, Decodable, Decoder};

/// Render a hardware address the way the `dhcp_request` event reports it: lower-case hex
/// with no separators, e.g. `001122334455`.
///
/// This matches the format the BOOTP sibling and the E2E suites expect. The reply actions
/// accept either this or the colon-separated form when `client_mac` is passed explicitly.
pub fn format_mac(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

/// DHCP server that forwards requests to LLM
pub struct DhcpServer;

impl DhcpServer {
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

        let task_registrar = app_state.clone();
        let accept_handle = tokio::spawn(async move {
            let mut buffer = vec![0u8; 1500];

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);

                        // DEBUG: Log summary
                        console_debug!(status_tx, "DHCP received {} bytes from {}", n, peer_addr);

                        // TRACE: Log full payload (always hex for DHCP)
                        let hex_str = hex::encode(&data);
                        console_trace!(status_tx, "DHCP data (hex): {}", hex_str);

                        // A datagram that does not decode - or decodes without a DHCP message
                        // type option - is not a DHCP request. Raising `dhcp_request` for it
                        // spent an LLM round trip and handed the model an event reading
                        // "unknown" in every field, out of which no reply could be built:
                        // `base_reply` has no transaction id to echo and errors. Drop it.
                        let Some((_, Some(request_ctx))) = Self::parse_dhcp_message(&data) else {
                            tracing::warn!(
                                "Dropping non-DHCP datagram ({} bytes) from {}",
                                n,
                                peer_addr
                            );
                            let _ = status_tx.send(format!(
                                "[WARN] Dropping non-DHCP datagram ({} bytes) from {}",
                                n, peer_addr
                            ));
                            continue;
                        };

                        // Add connection to ServerInstance
                        use crate::state::server::{
                            ConnectionState as ServerConnectionState, ConnectionStatus,
                            ProtocolConnectionInfo,
                        };
                        let now = std::time::Instant::now();

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
                        app_state
                            .add_connection_to_server(server_id, conn_state)
                            .await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();

                        tokio::spawn(async move {
                            // One protocol instance per request. The instance carries the
                            // request context (xid, chaddr, giaddr, broadcast flag) used to
                            // build the reply, so two clients whose LLM calls overlap can
                            // never echo each other's transaction ID.
                            let protocol = DhcpProtocol::new();
                            protocol.set_request_context(request_ctx.clone());

                            let message_type = format!("{:?}", request_ctx.message_type);
                            let client_mac = crate::server::dhcp::format_mac(&request_ctx.chaddr);
                            let requested_ip = request_ctx.requested_ip.map(|ip| ip.to_string());
                            let xid = Some(request_ctx.xid);
                            let client_ip = request_ctx.ciaddr.to_string();
                            let gateway_ip = request_ctx.giaddr.to_string();

                            let mut event_data = serde_json::json!({
                                "message_type": message_type,
                                "client_mac": client_mac,
                                "xid": xid,
                                "client_ip": client_ip,
                                "gateway_ip": gateway_ip
                            });
                            if let Some(ip) = requested_ip {
                                event_data["requested_ip"] = serde_json::json!(ip);
                            }

                            let event = Event::new(&DHCP_REQUEST_EVENT, event_data);

                            debug!("DHCP calling LLM for request from {}", peer_addr);
                            let _ = status_clone.send(format!(
                                "[DEBUG] DHCP calling LLM for request from {}",
                                peer_addr
                            ));

                            match call_llm(
                                &llm_clone,
                                &state_clone,
                                server_id,
                                None,
                                &event,
                                &protocol,
                            )
                            .await
                            {
                                Ok(execution_result) => {
                                    for message in &execution_result.messages {
                                        info!("{}", message);
                                        let _ = status_clone.send(format!("[INFO] {}", message));
                                    }

                                    debug!(
                                        "DHCP got {} protocol results",
                                        execution_result.protocol_results.len()
                                    );
                                    let _ = status_clone.send(format!(
                                        "[DEBUG] DHCP got {} protocol results",
                                        execution_result.protocol_results.len()
                                    ));

                                    for protocol_result in execution_result.protocol_results {
                                        if let Some(output_data) =
                                            protocol_result.get_all_output().first()
                                        {
                                            let _ =
                                                socket_clone.send_to(output_data, peer_addr).await;

                                            // DEBUG: Log summary
                                            debug!(
                                                "DHCP sent {} bytes to {}",
                                                output_data.len(),
                                                peer_addr
                                            );
                                            let _ = status_clone.send(format!(
                                                "[DEBUG] DHCP sent {} bytes to {}",
                                                output_data.len(),
                                                peer_addr
                                            ));

                                            // TRACE: Log full payload
                                            let hex_str = hex::encode(output_data);
                                            trace!("DHCP sent (hex): {}", hex_str);
                                            let _ = status_clone.send(format!(
                                                "[TRACE] DHCP sent (hex): {}",
                                                hex_str
                                            ));

                                            let _ = status_clone.send(format!(
                                                "→ DHCP response to {} ({} bytes)",
                                                peer_addr,
                                                output_data.len()
                                            ));
                                        } else {
                                            debug!("DHCP protocol result has no output data");
                                            let _ = status_clone.send(
                                                "[DEBUG] DHCP protocol result has no output data"
                                                    .to_string(),
                                            );
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

        // Register the recv loop so stop_server can abort it and release the port.
        task_registrar
            .register_server_task(server_id, accept_handle)
            .await;

        Ok(local_addr)
    }

    /// Decode a datagram into `(description, request context)`.
    ///
    /// `None` means the datagram is not usable as a DHCP request: it did not decode, its
    /// `hlen` is out of range, or it carries no message-type option. The caller drops those.
    fn parse_dhcp_message(data: &[u8]) -> Option<(String, Option<DhcpRequestContext>)> {
        use std::net::Ipv4Addr;

        match v4::Message::decode(&mut Decoder::new(data)) {
            Ok(msg) => {
                // `hlen` is read straight off the wire without validation, and
                // `Message::chaddr()` slices a fixed [u8; 16] with it - a datagram
                // declaring hlen > 16 panics inside dhcproto. This runs in the socket
                // task, so that panic would take the whole server down. Reject instead.
                if msg.hlen() as usize > 16 {
                    tracing::warn!(
                        "Dropping DHCP datagram with invalid hlen {} (max 16)",
                        msg.hlen()
                    );
                    return None;
                }

                // Extract message type from options
                let message_type = msg.opts().get(v4::OptionCode::MessageType).and_then(|opt| {
                    if let v4::DhcpOption::MessageType(mt) = opt {
                        Some(*mt)
                    } else {
                        None
                    }
                });

                let message_type_str = message_type
                    .as_ref()
                    .map(|mt| format!("{:?}", mt))
                    .unwrap_or_else(|| "Unknown".to_string());

                // Extract requested IP from options if present
                let requested_ip =
                    msg.opts()
                        .get(v4::OptionCode::RequestedIpAddress)
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
                    message_type_str,
                    mac_str,
                    msg.xid()
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
                    giaddr: msg.giaddr(),
                    broadcast: msg.flags().broadcast(),
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
