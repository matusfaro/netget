//! DHCP server implementation - simplified UDP-based

use crate::network::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::llm::ollama_client::OllamaClient;
use crate::llm::prompt::PromptBuilder;
use crate::state::app_state::AppState;

/// Get LLM context and output format instructions for DHCP stack
pub fn get_llm_prompt_config() -> (&'static str, &'static str) {
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
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
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
                        let connection_id = ConnectionId::new();

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();

                        tokio::spawn(async move {
                            let model = state_clone.get_ollama_model().await;
                            let prompt_config = get_llm_prompt_config();
                            let conn_memory = String::new();

                            // Build event description
                            let event_description = format!(
                                "DHCP request from {} ({} bytes)",
                                peer_addr, data.len()
                            );

                            let prompt = PromptBuilder::build_network_event_prompt(
                                &state_clone,
                                connection_id,
                                &conn_memory,
                                &event_description,
                                prompt_config,
                            ).await;

                            match llm_clone.generate(&model, &prompt).await {
                                Ok(llm_output) => {
                                    if let Err(e) = socket_clone.send_to(llm_output.as_bytes(), peer_addr).await {
                                        error!("Failed to send DHCP response: {}", e);
                                    } else {
                                        let _ = status_clone.send(format!(
                                            "→ DHCP response to {} ({} bytes)",
                                            peer_addr, llm_output.len()
                                        ));
                                    }
                                }
                                Err(e) => {
                                    error!("LLM error for DHCP: {}", e);
                                    let _ = status_clone.send(format!("✗ LLM error for DHCP: {}", e));
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