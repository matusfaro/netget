//! IRC server implementation

use crate::network::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::llm::ollama_client::OllamaClient;
use crate::llm::prompt::PromptBuilder;
use crate::state::app_state::AppState;

/// Get LLM context and output format instructions for IRC stack
pub fn get_llm_prompt_config() -> (&'static str, &'static str) {
    let context = r#"You are handling IRC chat protocol (port 6667).
Respond to IRC commands like JOIN, PART, PRIVMSG, NICK, USER, PING, etc.
Use IRC response codes (e.g., 001 for welcome, 332 for topic)."#;

    let output_format = r#"IMPORTANT: Respond with a JSON object:
{
  "output": "IRC response message (null if no response)",
  "close_connection": false,  // Close this connection after sending
  "message": null  // Optional message for user
}"#;

    (context, output_format)
}

/// IRC server that forwards messages to LLM
pub struct IrcServer;

impl IrcServer {
    /// Spawn IRC server with integrated LLM handling
    pub async fn spawn_with_llm(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<SocketAddr> {
        let listener = TcpListener::bind(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("IRC server listening on {}", local_addr);

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        let connection_id = ConnectionId::new();
                        info!("Accepted IRC connection {} from {}", connection_id, peer_addr);

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();

                        tokio::spawn(async move {
                            if let Err(e) = handle_irc_with_llm(
                                stream,
                                connection_id,
                                llm_clone,
                                state_clone,
                                status_clone,
                            ).await {
                                error!("IRC connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept IRC connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Handle IRC connection with integrated LLM
async fn handle_irc_with_llm(
    stream: TcpStream,
    connection_id: ConnectionId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
) -> Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    let mut connection_memory = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                // Connection closed
                let _ = status_tx.send(format!("✗ IRC connection {} closed", connection_id));
                break;
            }
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                // Call LLM with the IRC line
                let model = app_state.get_ollama_model().await;
                let prompt_config = get_llm_prompt_config();

                // Build event description
                let event_description = format!("IRC message received on connection {}: {}", connection_id, trimmed);

                let prompt = PromptBuilder::build_network_event_prompt(
                    &app_state,
                    connection_id,
                    &connection_memory,
                    &event_description,
                    prompt_config,
                ).await;

                match llm_client.generate_llm_response(&model, &prompt).await {
                    Ok(response) => {
                        // Handle common actions and update connection memory
                        use crate::llm::response_handler;
                        let processed = response_handler::handle_llm_response(response, &app_state, &mut connection_memory).await;

                        // Send output if present
                        if let Some(output) = processed.output {
                            // Ensure IRC messages end with \r\n
                            let formatted = if output.ends_with("\r\n") {
                                output.clone()
                            } else if output.ends_with('\n') {
                                format!("{}\r", output)
                            } else {
                                format!("{}\r\n", output)
                            };

                            if let Err(e) = write_half.write_all(formatted.as_bytes()).await {
                                error!("Failed to send IRC response: {}", e);
                                break;
                            }
                            if let Err(e) = write_half.flush().await {
                                error!("Failed to flush IRC response: {}", e);
                                break;
                            }
                            let _ = status_tx.send(format!("→ IRC to {}: {}", connection_id, formatted.trim()));
                        }

                        // Handle close
                        if processed.close_connection {
                            let _ = status_tx.send(format!("✗ Closing IRC connection {}", connection_id));
                            break;
                        }
                    }
                    Err(e) => {
                        error!("LLM error for IRC: {}", e);
                        let _ = status_tx.send(format!("✗ LLM error for IRC: {}", e));
                    }
                }
            }
            Err(e) => {
                error!("IRC read error: {}", e);
                break;
            }
        }
    }

    Ok(())
}

/// Send an IRC response
pub async fn send_irc_response(
    write_half: &mut tokio::net::tcp::WriteHalf<'_>,
    response: &str,
) -> Result<()> {
    // Ensure IRC messages end with \r\n
    let formatted = if response.ends_with("\r\n") {
        response.to_string()
    } else if response.ends_with('\n') {
        format!("{}\r", response)
    } else {
        format!("{}\r\n", response)
    };

    write_half.write_all(formatted.as_bytes()).await?;
    write_half.flush().await?;
    Ok(())
}