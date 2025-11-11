//! Socket File client implementation
pub mod actions;

pub use actions::SocketFileClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, trace};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::socket_file::actions::SOCKET_FILE_CLIENT_DATA_RECEIVED_EVENT;

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-client data for LLM handling
struct ClientData {
    state: ConnectionState,
    queued_data: Vec<u8>,
    memory: String,
}

/// Socket File client that connects to a Unix domain socket
pub struct SocketFileClient;

impl SocketFileClient {
    /// Connect to a Unix domain socket with integrated LLM actions
    pub async fn connect_with_llm_actions(
        socket_path: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Connect to Unix domain socket
        let stream = UnixStream::connect(&socket_path)
            .await
            .context(format!("Failed to connect to Unix socket {}", socket_path))?;

        // Unix sockets don't have traditional socket addresses, so we create a dummy one
        // The socket path is stored in the client's remote_addr field in app_state
        let dummy_addr = SocketAddr::from(([127, 0, 0, 1], 0));


        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        console_info!(status_tx, "[CLIENT] Socket File client {} connected to {}", client_id, socket_path);
        console_info!(status_tx, "__UPDATE_UI__");

        // Split stream
        let (mut read_half, write_half) = stream.into_split();
        let write_half_arc = Arc::new(Mutex::new(write_half));

        // Initialize client data
        let client_data = Arc::new(Mutex::new(ClientData {
            state: ConnectionState::Idle,
            queued_data: Vec::new(),
            memory: String::new(),
        }));

        // Spawn read loop
        tokio::spawn(async move {
            let mut buffer = vec![0u8; 8192];

            loop {
                match read_half.read(&mut buffer).await {
                    Ok(0) => {
                        app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                        console_info!(status_tx, "[CLIENT] Socket File client {} disconnected", client_id);
                        console_info!(status_tx, "__UPDATE_UI__");
                        break;
                    }
                    Ok(n) => {
                        let data = buffer[..n].to_vec();
                        trace!("Socket File client {} received {} bytes", client_id, n);

                        // Handle data with LLM
                        let mut client_data_lock = client_data.lock().await;

                        match client_data_lock.state {
                            ConnectionState::Idle => {
                                // Process immediately
                                client_data_lock.state = ConnectionState::Processing;
                                drop(client_data_lock);

                                // Call LLM
                                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                                    let protocol = Arc::new(crate::client::socket_file::actions::SocketFileClientProtocol::new());
                                    let event = Event::new(
                                        &SOCKET_FILE_CLIENT_DATA_RECEIVED_EVENT,
                                        serde_json::json!({
                                            "data_hex": hex::encode(&data),
                                            "data_length": data.len(),
                                        }),
                                    );

                                    match call_llm_for_client(
                                        &llm_client,
                                        &app_state,
                                        client_id.to_string(),
                                        &instruction,
                                        &client_data.lock().await.memory,
                                        Some(&event),
                                        protocol.as_ref(),
                                        &status_tx,
                                    ).await {
                                        Ok(ClientLlmResult { actions, memory_updates }) => {
                                            // Update memory
                                            if let Some(mem) = memory_updates {
                                                client_data.lock().await.memory = mem;
                                            }

                                            // Execute actions
                                            for action in actions {
                                                use crate::llm::actions::client_trait::Client;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};
                                                match protocol.as_ref().execute_action(action) {
                                                    Ok(crate::llm::actions::client_trait::ClientActionResult::SendData(bytes)) => {
                                                        if let Ok(_) = write_half_arc.lock().await.write_all(&bytes).await {
                                                            trace!("Socket File client {} sent {} bytes", client_id, bytes.len());
                                                        }
                                                    }
                                                    Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                                                        info!("Socket File client {} disconnecting", client_id);
                                                        break;
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            error!("LLM error for Socket File client {}: {}", client_id, e);
                                        }
                                    }
                                }

                                // Process queued data if any
                                let mut client_data_lock = client_data.lock().await;
                                if !client_data_lock.queued_data.is_empty() {
                                    client_data_lock.queued_data.clear();
                                }
                                client_data_lock.state = ConnectionState::Idle;
                            }
                            ConnectionState::Processing => {
                                // Queue data
                                client_data_lock.queued_data.extend_from_slice(&data);
                                client_data_lock.state = ConnectionState::Accumulating;
                            }
                            ConnectionState::Accumulating => {
                                // Continue queuing
                                client_data_lock.queued_data.extend_from_slice(&data);
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

        Ok(dummy_addr)
    }
}
