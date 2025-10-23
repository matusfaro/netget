//! UDP server implementation for raw UDP stack

use crate::network::connection::ConnectionId;
use crate::network::udp_actions::UdpProtocol;
use anyhow::Result;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info};

use crate::llm::ollama_client::OllamaClient;
use crate::llm::prompt::PromptBuilder;
use crate::llm::{execute_actions, ActionResponse, NetworkContext, ProtocolActions};
use crate::state::app_state::AppState;

/// Get LLM context and output format instructions for UDP stack
pub fn get_llm_protocol_prompt() -> (&'static str, &'static str) {
    let context = r#"You are handling raw UDP datagrams. Each packet is independent.
Common UDP protocols: DNS (port 53), DHCP (67/68), NTP (123), SNMP (161)"#;

    let output_format = r#"IMPORTANT: Respond with a JSON object:
{
  "output": "response data to send back (null if no response)",
  "message": null  // Optional message for user
}"#;

    (context, output_format)
}

/// UDP server that manages UDP connections
pub struct UdpServer;

impl UdpServer {
    /// Spawn UDP server with integrated LLM handling
    pub async fn spawn_with_llm(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<SocketAddr> {
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        info!("UDP server listening on {}", local_addr);

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 65535]; // Maximum UDP datagram size

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let connection_id = ConnectionId::new();

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();

                        tokio::spawn(async move {
                            let model = state_clone.get_ollama_model().await;
                            let prompt_config = get_llm_protocol_prompt();

                            // Build event description
                            let event_description = {
                                let data_preview = if data.len() > 200 {
                                    format!("{} bytes from {} (preview: {:?}...)", data.len(), peer_addr, &data[..200])
                                } else {
                                    format!("{} bytes from {}: {:?}", data.len(), peer_addr, data)
                                };
                                format!("UDP datagram received: {}", data_preview)
                            };

                            let prompt = PromptBuilder::build_network_event_prompt(
                                &state_clone,
                                connection_id,
                                &event_description,
                                prompt_config,
                            ).await;

                            match llm_clone.generate(&model, &prompt).await {
                                Ok(llm_output) => {
                                    if let Err(e) = socket_clone.send_to(llm_output.as_bytes(), peer_addr).await {
                                        error!("Failed to send UDP response: {}", e);
                                    } else {
                                        let _ = status_clone.send(format!(
                                            "→ UDP response to {} ({} bytes)",
                                            peer_addr, llm_output.len()
                                        ));
                                    }
                                }
                                Err(e) => {
                                    error!("LLM error for UDP: {}", e);
                                    let _ = status_clone.send(format!("✗ LLM error for UDP: {}", e));
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("UDP receive error: {}", e);
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Spawn UDP server with new action-based LLM handling
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<SocketAddr> {
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        info!("UDP server listening on {} (action-based)", local_addr);

        let protocol = Arc::new(UdpProtocol::with_socket(socket.clone()));

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 65535]; // Maximum UDP datagram size

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let _connection_id = ConnectionId::new();

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            let model = state_clone.get_ollama_model().await;

                            // Build event description
                            let event_description = {
                                let data_preview = if data.len() > 200 {
                                    format!("{} bytes from {} (preview: {:?}...)", data.len(), peer_addr, &data[..200])
                                } else {
                                    format!("{} bytes from {}: {:?}", data.len(), peer_addr, data)
                                };
                                format!("UDP datagram received: {}", data_preview)
                            };

                            // Create network context
                            let context = NetworkContext::UdpDatagram {
                                peer_addr,
                                socket: socket_clone.clone(),
                                status_tx: status_clone.clone(),
                            };

                            // Get protocol sync actions
                            let protocol_actions = protocol_clone.get_sync_actions(&context);

                            // Build prompt
                            let prompt = PromptBuilder::build_network_event_action_prompt(
                                &state_clone,
                                &event_description,
                                protocol_actions,
                            ).await;

                            // Call LLM
                            match llm_clone.generate(&model, &prompt).await {
                                Ok(llm_output) => {
                                    debug!("LLM UDP response: {}", llm_output);

                                    // Parse action response
                                    match ActionResponse::from_str(&llm_output) {
                                        Ok(action_response) => {
                                            // Execute actions
                                            match execute_actions(
                                                action_response.actions,
                                                &state_clone,
                                                Some(protocol_clone.as_ref()),
                                                Some(&context),
                                            ).await {
                                                Ok(result) => {
                                                    // Display messages
                                                    for msg in result.messages {
                                                        let _ = status_clone.send(msg);
                                                    }

                                                    // Handle protocol results
                                                    for protocol_result in result.protocol_results {
                                                        if let Some(output_data) = protocol_result.get_all_output().first() {
                                                            if let Err(e) = socket_clone.send_to(output_data, peer_addr).await {
                                                                error!("Failed to send UDP response: {}", e);
                                                            } else {
                                                                let _ = status_clone.send(format!(
                                                                    "→ UDP response to {} ({} bytes)",
                                                                    peer_addr, output_data.len()
                                                                ));
                                                            }
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    error!("Failed to execute actions: {}", e);
                                                    let _ = status_clone.send(format!("✗ Action execution error: {}", e));
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            error!("Failed to parse action response: {}", e);
                                            let _ = status_clone.send(format!("✗ Parse error: {}", e));
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("LLM error for UDP: {}", e);
                                    let _ = status_clone.send(format!("✗ LLM error for UDP: {}", e));
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("UDP receive error: {}", e);
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Shared UDP socket for sending responses
pub type SharedUdpSocket = Arc<Mutex<Arc<UdpSocket>>>;

/// Map from connection ID to peer address for UDP responses
pub type UdpPeerMap = Arc<Mutex<HashMap<ConnectionId, SocketAddr>>>;