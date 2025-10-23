//! DNS server implementation - simplified UDP-based

use crate::network::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::ollama_client::OllamaClient;
use crate::llm::prompt::PromptBuilder;
use crate::llm::{ActionResponse, execute_actions, NetworkContext, ProtocolActions};
use crate::network::DnsProtocol;
use crate::state::app_state::AppState;

/// Get LLM context and output format instructions for DNS stack
pub fn get_llm_protocol_prompt() -> (&'static str, &'static str) {
    let context = r#"You are handling DNS queries (port 53). Parse DNS queries and generate DNS responses.
Common query types: A (IPv4), AAAA (IPv6), MX (mail), NS (nameserver), TXT (text records)"#;

    let output_format = r#"IMPORTANT: Respond with a JSON object:
{
  "output": "DNS response data as hex or base64 (null if no response)",
  "message": null  // Optional message for user
}"#;

    (context, output_format)
}

/// DNS server that forwards queries to LLM
pub struct DnsServer;

impl DnsServer {
    /// Spawn DNS server with integrated LLM handling
    pub async fn spawn_with_llm(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<SocketAddr> {
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        info!("DNS server listening on {}", local_addr);

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 512]; // Standard DNS packet size

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let connection_id = ConnectionId::new();

                        // DEBUG: Log summary
                        debug!("DNS received {} bytes from {}", n, peer_addr);
                        let _ = status_tx.send(format!("[DEBUG] DNS received {} bytes from {}", n, peer_addr));

                        // TRACE: Log full payload (always hex for DNS)
                        let hex_str = hex::encode(&data);
                        trace!("DNS data (hex): {}", hex_str);
                        let _ = status_tx.send(format!("[TRACE] DNS data (hex): {}", hex_str));

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();

                        // Spawn task to handle DNS query with LLM
                        tokio::spawn(async move {
                            let model = state_clone.get_ollama_model().await;
                            let prompt_config = get_llm_protocol_prompt();

                            // Build event description
                            let event_description = format!(
                                "DNS query from {} ({} bytes)",
                                peer_addr, data.len()
                            );

                            let prompt = PromptBuilder::build_network_event_prompt(
                                &state_clone,
                                connection_id,
                                &event_description,
                                prompt_config,
                            ).await;

                            match llm_clone.generate(&model, &prompt).await {
                                Ok(llm_output) => {
                                    // LLM should return DNS response bytes
                                    // For now, send the output as-is
                                    let output_data = llm_output.as_bytes();
                                    if let Err(e) = socket_clone.send_to(output_data, peer_addr).await {
                                        error!("Failed to send DNS response: {}", e);
                                    } else {
                                        // DEBUG: Log summary
                                        debug!("DNS sent {} bytes to {}", output_data.len(), peer_addr);
                                        let _ = status_clone.send(format!("[DEBUG] DNS sent {} bytes to {}", output_data.len(), peer_addr));

                                        // TRACE: Log full payload (always hex for DNS)
                                        let hex_str = hex::encode(output_data);
                                        trace!("DNS sent (hex): {}", hex_str);
                                        let _ = status_clone.send(format!("[TRACE] DNS sent (hex): {}", hex_str));

                                        let _ = status_clone.send(format!(
                                            "→ DNS response to {} ({} bytes)",
                                            peer_addr, output_data.len()
                                        ));
                                    }
                                }
                                Err(e) => {
                                    error!("LLM error for DNS query: {}", e);
                                    let _ = status_clone.send(format!("✗ LLM error for DNS: {}", e));
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("DNS receive error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Spawn DNS server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<SocketAddr> {
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        info!("DNS server (action-based) listening on {}", local_addr);

        let protocol = Arc::new(DnsProtocol::new());

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 512]; // Standard DNS packet size

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let _connection_id = ConnectionId::new();

                        // DEBUG: Log summary
                        debug!("DNS received {} bytes from {}", n, peer_addr);
                        let _ = status_tx.send(format!("[DEBUG] DNS received {} bytes from {}", n, peer_addr));

                        // TRACE: Log full payload (always hex for DNS)
                        let hex_str = hex::encode(&data);
                        trace!("DNS data (hex): {}", hex_str);
                        let _ = status_tx.send(format!("[TRACE] DNS data (hex): {}", hex_str));

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            let model = state_clone.get_ollama_model().await;

                            // Build event description
                            let data_hex = data.iter().map(|b| format!("{:02x}", b)).collect::<String>();
                            let event_description = format!(
                                "DNS query from {} ({} bytes): {}",
                                peer_addr, data.len(), data_hex
                            );

                            // Create network context
                            let context = NetworkContext::DnsQuery {
                                peer_addr,
                                socket: socket_clone.clone(),
                                query_data: data.clone(),
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
                                                                error!("Failed to send DNS response: {}", e);
                                                            } else {
                                                                // DEBUG: Log summary
                                                                debug!("DNS sent {} bytes to {}", output_data.len(), peer_addr);
                                                                let _ = status_clone.send(format!("[DEBUG] DNS sent {} bytes to {}", output_data.len(), peer_addr));

                                                                // TRACE: Log full payload (always hex for DNS)
                                                                let hex_str = hex::encode(output_data);
                                                                trace!("DNS sent (hex): {}", hex_str);
                                                                let _ = status_clone.send(format!("[TRACE] DNS sent (hex): {}", hex_str));

                                                                let _ = status_clone.send(format!(
                                                                    "→ DNS response to {} ({} bytes)",
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
                                    error!("LLM error for DNS query: {}", e);
                                    let _ = status_clone.send(format!("✗ LLM error for DNS: {}", e));
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("DNS receive error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}