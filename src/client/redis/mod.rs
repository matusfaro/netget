//! Redis client implementation
pub mod actions;

pub use actions::RedisClientProtocol;

use crate::llm::actions::client_trait::Client;
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, trace};

use crate::client::redis::actions::{
    REDIS_CLIENT_CONNECTED_EVENT, REDIS_CLIENT_RESPONSE_RECEIVED_EVENT,
};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::logging::patterns;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

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

        info!(
            "Redis client {} {} {} (local: {})",
            client_id,
            patterns::REDIS_CLIENT_CONNECTED,
            remote_sock_addr,
            local_addr
        );

        // Update client state
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!("[CLIENT] Redis client {} connected", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Split stream
        let (read_half, write_half) = tokio::io::split(stream);
        let write_half_arc = Arc::new(Mutex::new(write_half));
        let write_half_for_connected = write_half_arc.clone();
        let mut reader = BufReader::new(read_half);

        // Call LLM with redis_connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let event = Event::new(
                &REDIS_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "remote_addr": remote_sock_addr.to_string(),
                }),
            );

            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &String::new(), // No memory yet for initial connection
                Some(&event),
                &crate::client::redis::actions::RedisClientProtocol::new(),
                &status_tx,
            )
            .await
            {
                Ok(result) => {
                    // Execute actions from LLM response
                    for action in result.actions {
                        if let Some(action_type) = action["type"].as_str() {
                            match action_type {
                                "execute_redis_command" => {
                                    if let Some(command) = action["command"].as_str() {
                                        let command_bytes = encode_redis_command(command);
                                        let mut write_guard = write_half_for_connected.lock().await;
                                        if let Err(e) = write_guard.write_all(&command_bytes).await {
                                            error!("Failed to send Redis command after connect: {}", e);
                                        } else if let Err(e) = write_guard.flush().await {
                                            error!("Failed to flush after connect: {}", e);
                                        } else {
                                            info!("{} {}", patterns::REDIS_CLIENT_SENT_COMMAND, command);
                                        }
                                    }
                                }
                                "disconnect" => {
                                    info!("LLM requested disconnect after connect");
                                    return Ok(local_addr);
                                }
                                _ => {
                                    trace!("Unknown action type after connect: {}", action_type);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error on redis_connected event: {}", e);
                }
            }
        }

        // Spawn read loop for Redis responses
        tokio::spawn(async move {
            loop {
                // Read Redis RESP response
                // Simplified: just read line-by-line
                let mut line = String::new();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        info!("Redis client {} {}", client_id, patterns::REDIS_CLIENT_DISCONNECTED);
                        app_state
                            .update_client_status(client_id, ClientStatus::Disconnected)
                            .await;
                        let _ = status_tx
                            .send(format!("[CLIENT] Redis client {} disconnected", client_id));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        break;
                    }
                    Ok(_) => {
                        trace!("Redis client {} received: {}", client_id, line.trim());

                        // Call LLM with response
                        if let Some(instruction) =
                            app_state.get_instruction_for_client(client_id).await
                        {
                            let protocol =
                                Arc::new(crate::client::redis::actions::RedisClientProtocol::new());
                            let event = Event::new(
                                &REDIS_CLIENT_RESPONSE_RECEIVED_EVENT,
                                serde_json::json!({
                                    "response": line.trim(),
                                }),
                            );

                            let memory = app_state
                                .get_memory_for_client(client_id)
                                .await
                                .unwrap_or_default();

                            match call_llm_for_client(
                                &llm_client,
                                &app_state,
                                client_id.to_string(),
                                &instruction,
                                &memory,
                                Some(&event),
                                protocol.as_ref(),
                                &status_tx,
                            )
                            .await
                            {
                                Ok(ClientLlmResult {
                                    actions,
                                    memory_updates,
                                }) => {
                                    // Update memory
                                    if let Some(mem) = memory_updates {
                                        app_state.set_memory_for_client(client_id, mem).await;
                                    }

                                    // Execute actions
                                    for action in actions {
                                        match protocol.execute_action(action) {
                                            Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data }) if name == "redis_command" => {
                                                if let Some(command_str) = data.get("command").and_then(|v| v.as_str()) {
                                                    let cmd_bytes = encode_redis_command(command_str);
                                                    let mut write_guard = write_half_arc.lock().await;
                                                    if let Ok(_) = write_guard.write_all(&cmd_bytes).await {
                                                        if let Ok(_) = write_guard.flush().await {
                                                            trace!("Redis client {} sent: {}", client_id, command_str);
                                                        }
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
                        error!("Redis client {} read error: {}", client_id, e);
                        app_state
                            .update_client_status(client_id, ClientStatus::Error(e.to_string()))
                            .await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

/// Encode a Redis command as a RESP array
///
/// Example: "PING" -> "*1\r\n$4\r\nPING\r\n"
/// Example: "SET key value" -> "*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n"
fn encode_redis_command(command: &str) -> Vec<u8> {
    // Split command into parts
    let parts: Vec<&str> = command.split_whitespace().collect();

    // Start with array length
    let mut result = format!("*{}\r\n", parts.len()).into_bytes();

    // Encode each part as a bulk string
    for part in parts {
        result.extend_from_slice(&format!("${}\r\n", part.len()).into_bytes());
        result.extend_from_slice(part.as_bytes());
        result.extend_from_slice(b"\r\n");
    }

    result
}
