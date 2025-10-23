//! SSH server implementation - simplified

use bytes::Bytes;
use crate::network::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::llm::ollama_client::OllamaClient;
use crate::llm::prompt::PromptBuilder;
use crate::state::app_state::AppState;

/// Get LLM context and output format instructions for SSH stack
pub fn get_llm_prompt_config() -> (&'static str, &'static str) {
    let context = r#"You are handling SSH protocol (port 22).
Handle SSH handshake, authentication, and shell sessions.
Respond with appropriate SSH protocol messages."#;

    let output_format = r#"IMPORTANT: Respond with a JSON object:
{
  "output": "SSH protocol data to send (null if no response)",
  "close_connection": false,  // Close this connection after sending
  "message": null,  // Optional message for user
  "set_connection_memory": null,  // Replace connection memory
  "append_connection_memory": null  // Append to connection memory
}"#;

    (context, output_format)
}

/// SSH server that forwards sessions to LLM
pub struct SshServer;

impl SshServer {
    /// Spawn SSH server with integrated LLM handling
    pub async fn spawn_with_llm(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<SocketAddr> {
        let listener = TcpListener::bind(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("SSH server listening on {}", local_addr);

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        let connection_id = ConnectionId::new();
                        info!("Accepted SSH connection {} from {}", connection_id, peer_addr);

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();

                        tokio::spawn(async move {
                            let (mut read_half, mut write_half) = stream.into_split();
                            let mut buffer = vec![0u8; 8192];
                            let mut connection_memory = String::new();

                            loop {
                                match read_half.read(&mut buffer).await {
                                    Ok(0) => {
                                        let _ = status_clone.send(format!("✗ SSH connection {} closed", connection_id));
                                        break;
                                    }
                                    Ok(n) => {
                                        let data = Bytes::copy_from_slice(&buffer[..n]);

                                        let model = state_clone.get_ollama_model().await;
                                        let prompt_config = get_llm_prompt_config();

                                        // Build event description
                                        let event_description = {
                                            let data_preview = if data.len() > 200 {
                                                format!("{} bytes (preview: {:?}...)", data.len(), &data[..200])
                                            } else {
                                                format!("{:?}", data)
                                            };
                                            format!("SSH data received on connection {}: {}", connection_id, data_preview)
                                        };

                                        let prompt = PromptBuilder::build_network_event_prompt(
                                            &state_clone,
                                            connection_id,
                                            &connection_memory,
                                            &event_description,
                                            prompt_config,
                                        ).await;

                                        match llm_clone.generate_llm_response(&model, &prompt).await {
                                            Ok(response) => {
                                                // Handle common actions and update connection memory
                                                use crate::llm::response_handler;
                                                let processed = response_handler::handle_llm_response(response, &state_clone, &mut connection_memory).await;

                                                // Send output
                                                if let Some(output) = processed.output {
                                                    if let Err(e) = write_half.write_all(output.as_bytes()).await {
                                                        error!("Failed to send SSH response: {}", e);
                                                        break;
                                                    }
                                                    let _ = status_clone.send(format!("→ SSH to {}: {} bytes", connection_id, output.len()));
                                                }

                                                // Handle close
                                                if processed.close_connection {
                                                    break;
                                                }
                                            }
                                            Err(e) => {
                                                error!("LLM error for SSH: {}", e);
                                                let _ = status_clone.send(format!("✗ LLM error for SSH: {}", e));
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("SSH read error: {}", e);
                                        break;
                                    }
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept SSH connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}