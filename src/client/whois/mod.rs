//! WHOIS client implementation
pub mod actions;

pub use actions::WhoisClientProtocol;

use anyhow::{Context, Result};
use crate::llm::actions::client_trait::Client;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::whois::actions::{WHOIS_CLIENT_CONNECTED_EVENT, WHOIS_CLIENT_RESPONSE_RECEIVED_EVENT};
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// WHOIS client that connects to a WHOIS server
pub struct WhoisClient;

impl WhoisClient {
    /// Connect to a WHOIS server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Connect to WHOIS server
        let stream = TcpStream::connect(&remote_addr)
            .await
            .context(format!("Failed to connect to WHOIS server at {}", remote_addr))?;

        let local_addr = stream.local_addr()?;
        let remote_sock_addr = stream.peer_addr()?;


        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        console_info!(status_tx, "[CLIENT] WHOIS client {} connected", client_id);
        console_info!(status_tx, "__UPDATE_UI__");

        // Split stream
        let (mut read_half, write_half) = tokio::io::split(stream);
        let write_half_arc = Arc::new(Mutex::new(write_half));

        // Spawn task to handle LLM interaction
        tokio::spawn(async move {
            // Call LLM with connected event to get initial query
            if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                let protocol = Arc::new(WhoisClientProtocol::new());
                let event = Event::new(
                    &WHOIS_CLIENT_CONNECTED_EVENT,
                    serde_json::json!({
                        "remote_addr": remote_addr,
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

                        // Execute actions (should include query_whois)
                        let mut query_to_send: Option<String> = None;
                        for action in actions {
                            match protocol.execute_action(action) {
                                Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data }) if name == "whois_query" => {
                                    if let Some(query) = data.get("query").and_then(|v| v.as_str()) {
                                        query_to_send = Some(query.to_string());
                                    }
                                }
                                Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                                    info!("WHOIS client {} disconnecting before query", client_id);
                                    return;
                                }
                                _ => {}
                            }
                        }

                        // Send query if we got one
                        if let Some(query) = query_to_send {
                            debug!("WHOIS client {} querying: {}", client_id, query);
                            let query_bytes = format!("{}\r\n", query);

                            if let Err(e) = write_half_arc.lock().await.write_all(query_bytes.as_bytes()).await {
                                app_state.update_client_status(client_id, ClientStatus::Error(e.to_string())).await;
                                console_error!(status_tx, "__UPDATE_UI__");
                                return;
                            }

                            trace!("WHOIS client {} sent query: {}", client_id, query);

                            // Read full response (WHOIS servers close after sending response)
                            let mut response = String::new();
                            match read_half.read_to_string(&mut response).await {
                                Ok(bytes_read) => {
                                    debug!("WHOIS client {} received {} bytes", client_id, bytes_read);
                                    trace!("WHOIS response:\n{}", response);

                                    // Call LLM with response
                                    let event = Event::new(
                                        &WHOIS_CLIENT_RESPONSE_RECEIVED_EVENT,
                                        serde_json::json!({
                                            "response": response,
                                            "query": query,
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
                                        Ok(ClientLlmResult { actions: _, memory_updates }) => {
                                            // Update memory
                                            if let Some(mem) = memory_updates {
                                                app_state.set_memory_for_client(client_id, mem).await;
                                            }
                                        }
                                        Err(e) => {
                                            error!("LLM error for WHOIS client {}: {}", client_id, e);
                                        }
                                    }

                                    // WHOIS is one-shot, connection closes after response
                                    app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                                    console_info!(status_tx, "[CLIENT] WHOIS client {} disconnected", client_id);
                                    console_info!(status_tx, "__UPDATE_UI__");
                                }
                                Err(e) => {
                                    app_state.update_client_status(client_id, ClientStatus::Error(e.to_string())).await;
                                    console_error!(status_tx, "__UPDATE_UI__");
                                }
                            }
                        } else {
                            app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                            console_info!(status_tx, "__UPDATE_UI__");
                        }
                    }
                    Err(e) => {
                        app_state.update_client_status(client_id, ClientStatus::Error(e.to_string())).await;
                        console_error!(status_tx, "__UPDATE_UI__");
                    }
                }
            }
        });

        Ok(local_addr)
    }
}
