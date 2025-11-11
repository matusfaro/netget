//! BOOTP server implementation
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
use crate::server::BootpProtocol;
use crate::state::app_state::AppState;
use actions::BOOTP_REQUEST_EVENT;

use crate::{console_debug, console_error, console_info, console_trace, console_warn};
#[cfg(feature = "bootp")]
use actions::BootpRequestContext;
#[cfg(feature = "bootp")]
use dhcproto::{v4, Decodable, Decoder};

/// BOOTP server that forwards requests to LLM
pub struct BootpServer;

impl BootpServer {
    /// Spawn BOOTP server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        info!("BOOTP server (action-based) listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] BOOTP server listening on {}", local_addr));

        let protocol = Arc::new(BootpProtocol::new());

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 1500];

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);

                        // DEBUG: Log summary
                        console_debug!(status_tx, "BOOTP received {} bytes from {}", n, peer_addr);

                        // TRACE: Log full payload (always hex for BOOTP)
                        let hex_str = hex::encode(&data);
                        console_trace!(status_tx, "BOOTP data (hex): {}", hex_str);

                        #[cfg(feature = "bootp")]
                        let parsed_info = Self::parse_bootp_message(&data);

                        #[cfg(not(feature = "bootp"))]
                        let parsed_info: Option<(
                            String,
                            Option<BootpRequestContext>,
                        )> = None;

                        // Add connection to ServerInstance
                        use crate::state::server::{
                            ConnectionState as ServerConnectionState, ConnectionStatus,
                            ProtocolConnectionInfo,
                        };
                        let now = std::time::Instant::now();

                        #[cfg(feature = "bootp")]
                        let _request_type = parsed_info
                            .as_ref()
                            .map(|(desc, _)| desc.clone())
                            .unwrap_or_else(|| "unknown".to_string());

                        #[cfg(not(feature = "bootp"))]
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
                        app_state
                            .add_connection_to_server(server_id, conn_state)
                            .await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            #[cfg(feature = "bootp")]
                            if let Some((_, context)) = parsed_info.as_ref() {
                                if let Some(ctx) = context {
                                    protocol_clone.set_request_context(ctx.clone());
                                }
                            }

                            // Extract event data
                            #[cfg(feature = "bootp")]
                            let (op_code, client_mac, client_ip) =
                                if let Some((_, Some(ctx))) = &parsed_info {
                                    (
                                        format!("{:?}", ctx.op),
                                        hex::encode(&ctx.chaddr),
                                        ctx.ciaddr.to_string(),
                                    )
                                } else {
                                    (
                                        "unknown".to_string(),
                                        "unknown".to_string(),
                                        "0.0.0.0".to_string(),
                                    )
                                };

                            #[cfg(not(feature = "bootp"))]
                            let (op_code, client_mac, client_ip) = (
                                "unknown".to_string(),
                                "unknown".to_string(),
                                "0.0.0.0".to_string(),
                            );

                            let event_data = serde_json::json!({
                                "op_code": op_code,
                                "client_mac": client_mac,
                                "client_ip": client_ip
                            });

                            let event = Event::new(&BOOTP_REQUEST_EVENT, event_data);

                            debug!("BOOTP calling LLM for request from {}", peer_addr);
                            let _ = status_clone.send(format!(
                                "[DEBUG] BOOTP calling LLM for request from {}",
                                peer_addr
                            ));

                            match call_llm(
                                &llm_clone,
                                &state_clone,
                                server_id,
                                None,
                                &event,
                                protocol_clone.as_ref(),
                            )
                            .await
                            {
                                Ok(execution_result) => {
                                    for message in &execution_result.messages {
                                        info!("{}", message);
                                        let _ = status_clone.send(format!("[INFO] {}", message));
                                    }

                                    debug!(
                                        "BOOTP got {} protocol results",
                                        execution_result.protocol_results.len()
                                    );
                                    let _ = status_clone.send(format!(
                                        "[DEBUG] BOOTP got {} protocol results",
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
                                                "BOOTP sent {} bytes to {}",
                                                output_data.len(),
                                                peer_addr
                                            );
                                            let _ = status_clone.send(format!(
                                                "[DEBUG] BOOTP sent {} bytes to {}",
                                                output_data.len(),
                                                peer_addr
                                            ));

                                            // TRACE: Log full payload
                                            let hex_str = hex::encode(output_data);
                                            trace!("BOOTP sent (hex): {}", hex_str);
                                            let _ = status_clone.send(format!(
                                                "[TRACE] BOOTP sent (hex): {}",
                                                hex_str
                                            ));

                                            let _ = status_clone.send(format!(
                                                "→ BOOTP response to {} ({} bytes)",
                                                peer_addr,
                                                output_data.len()
                                            ));
                                        } else {
                                            debug!("BOOTP protocol result has no output data");
                                            let _ = status_clone.send(
                                                "[DEBUG] BOOTP protocol result has no output data"
                                                    .to_string(),
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("BOOTP LLM call failed: {}", e);
                                    let _ = status_clone.send(format!("✗ BOOTP LLM error: {}", e));
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("BOOTP receive error: {}", e);
                        let _ = status_tx.send(format!("✗ BOOTP receive error: {}", e));
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    #[cfg(feature = "bootp")]
    fn parse_bootp_message(data: &[u8]) -> Option<(String, Option<BootpRequestContext>)> {
        match v4::Message::decode(&mut Decoder::new(data)) {
            Ok(msg) => {
                let op = msg.opcode();

                // Build human-readable description
                let mac_str = hex::encode(msg.chaddr());
                let description = format!(
                    "BOOTP {} from client MAC {} (transaction ID: 0x{:08x}, client IP: {})",
                    format!("{:?}", op),
                    mac_str,
                    msg.xid(),
                    msg.ciaddr()
                );

                // Create context for action execution
                let context = BootpRequestContext {
                    xid: msg.xid(),
                    chaddr: msg.chaddr().to_vec(),
                    op: op,
                    ciaddr: msg.ciaddr(),
                    giaddr: msg.giaddr(),
                    sname: msg
                        .sname()
                        .map(|s| String::from_utf8_lossy(s).trim_matches('\0').to_string())
                        .unwrap_or_default(),
                    file: msg
                        .fname()
                        .map(|f| String::from_utf8_lossy(f).trim_matches('\0').to_string())
                        .unwrap_or_default(),
                };

                Some((description, Some(context)))
            }
            Err(e) => {
                tracing::warn!("Failed to parse BOOTP message: {}", e);
                None
            }
        }
    }
}
