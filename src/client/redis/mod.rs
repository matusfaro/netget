//! Redis client implementation
pub mod actions;

pub use actions::RedisClientProtocol;

use anyhow::{Context, Result};
use crate::llm::actions::client_trait::Client;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, trace};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::redis::actions::REDIS_CLIENT_RESPONSE_RECEIVED_EVENT;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// Redis client that connects to a Redis server
pub struct RedisClient;

impl RedisClient {
    /// Connect to a Redis server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Connect to Redis server
        let stream = TcpStream::connect(&remote_addr)
            .await
            .context(format!("Failed to connect to Redis at {}", remote_addr))?;

        let local_addr = stream.local_addr()?;
        let remote_sock_addr = stream.peer_addr()?;


        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        console_info!(status_tx, "[CLIENT] Redis client {} connected", client_id);
        console_info!(status_tx, "__UPDATE_UI__");

        // Split stream
        let (read_half, write_half) = tokio::io::split(stream);
        let write_half_arc = Arc::new(Mutex::new(write_half));
        let mut reader = BufReader::new(read_half);

        // Spawn read loop for Redis responses
        tokio::spawn(async move {
            loop {
                // Read Redis RESP response
                // Simplified: just read line-by-line
                let mut line = String::new();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                        console_info!(status_tx, "[CLIENT] Redis client {} disconnected", client_id);
                        console_info!(status_tx, "__UPDATE_UI__");
                        break;
                    }
                    Ok(_) => {
                        trace!("Redis client {} received: {}", client_id, line.trim());

                        // Call LLM with response
                        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                            let protocol = Arc::new(crate::client::redis::actions::RedisClientProtocol::new());
                            let event = Event::new(
                                &REDIS_CLIENT_RESPONSE_RECEIVED_EVENT,
                                serde_json::json!({
                                    "response": line.trim(),
                                }),
                            );

                            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

                            match call_llm_for_client(
                                &llm_client,
                                &app_state,
                                client_id.to_string(),
                                &instruction,
                                &memory,
                                Some(&event),
                                protocol.as_ref(),
                                &status_tx,
                            ).await {
                                Ok(ClientLlmResult { actions, memory_updates }) => {
                                    // Update memory
                                    if let Some(mem) = memory_updates {
                                        app_state.set_memory_for_client(client_id, mem).await;
                                    }

                                    // Execute actions
                                    for action in actions {
                                        match protocol.execute_action(action) {
                                            Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data }) if name == "redis_command" => {
                                                if let Some(command_str) = data.get("command").and_then(|v| v.as_str()) {
                                                    let cmd = format!("{}\r\n", command_str);
                                                    if let Ok(_) = write_half_arc.lock().await.write_all(cmd.as_bytes()).await {
                                                        trace!("Redis client {} sent: {}", client_id, command_str);
                                                    }
                                                }
                                            }
                                            Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                                                info!("Redis client {} disconnecting", client_id);
                                                break;
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("LLM error for Redis client {}: {}", client_id, e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        app_state.update_client_status(client_id, ClientStatus::Error(e.to_string())).await;
                        console_error!(status_tx, "__UPDATE_UI__");
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}
