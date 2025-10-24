//! DHCP server implementation - simplified UDP-based

use crate::network::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::ollama_client::OllamaClient;
use crate::llm::call_llm_with_protocol;
use crate::network::DhcpProtocol;
use crate::state::app_state::AppState;

/// Get LLM context and output format instructions for DHCP stack
pub fn get_llm_protocol_prompt() -> (&'static str, &'static str) {
    let context = r#"You are handling DHCP requests (ports 67/68). Respond to DHCP DISCOVER, REQUEST, and other messages.
Provide IP address assignments, subnet masks, gateways, and DNS servers."#;

    let output_format = r#"IMPORTANT: Respond with a JSON object:
{
  "output": "DHCP response data (null if no response)",
  "message": null  // Optional message for user
}"#;

    (context, output_format)
}

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

        let protocol = Arc::new(DhcpProtocol::new());

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 1500];

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let connection_id = ConnectionId::new();

                        // Add connection to ServerInstance (DHCP "connection" = recent request)
                        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
                        let now = std::time::Instant::now();
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
                                recent_requests: vec![("request".to_string(), now)],
                            },
                        };
                        app_state.add_connection_to_server(server_id, conn_state).await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        // DEBUG: Log summary
                        debug!("DHCP received {} bytes from {}", n, peer_addr);
                        let _ = status_tx.send(format!("[DEBUG] DHCP received {} bytes from {}", n, peer_addr));

                        // TRACE: Log full payload (always hex for DHCP)
                        let hex_str = hex::encode(&data);
                        trace!("DHCP data (hex): {}", hex_str);
                        let _ = status_tx.send(format!("[TRACE] DHCP data (hex): {}", hex_str));

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            let event_description = format!("DHCP request from {} ({} bytes)", peer_addr, data.len());

                            // Use action helper
                            match call_llm_with_protocol(
                                &llm_clone,
                                &state_clone,
                                server_id,
                                &event_description,
                                protocol_clone.as_ref(),
                            ).await {
                                Ok(result) => {
                                    for msg in result.messages {
                                        let _ = status_clone.send(msg);
                                    }
                                    for protocol_result in result.protocol_results {
                                        if let Some(output_data) = protocol_result.get_all_output().first() {
                                            let _ = socket_clone.send_to(output_data, peer_addr).await;

                                            // DEBUG: Log summary
                                            debug!("DHCP sent {} bytes to {}", output_data.len(), peer_addr);
                                            let _ = status_clone.send(format!("[DEBUG] DHCP sent {} bytes to {}", output_data.len(), peer_addr));

                                            // TRACE: Log full payload (always hex for DHCP)
                                            let hex_str = hex::encode(output_data);
                                            trace!("DHCP sent (hex): {}", hex_str);
                                            let _ = status_clone.send(format!("[TRACE] DHCP sent (hex): {}", hex_str));

                                            let _ = status_clone.send(format!("→ DHCP response to {} ({} bytes)", peer_addr, output_data.len()));
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("DHCP LLM error: {}", e);
                                    let _ = status_clone.send(format!("✗ DHCP LLM error: {}", e));
                                }
                            }
                        });
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
}