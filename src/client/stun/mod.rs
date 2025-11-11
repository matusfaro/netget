//! STUN client implementation for NAT traversal discovery
pub mod actions;

pub use actions::StunClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::stun::actions::{STUN_CLIENT_CONNECTED_EVENT, STUN_CLIENT_BINDING_RESPONSE_EVENT};
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// STUN client for discovering external IP/port behind NAT
pub struct StunClient;

impl StunClient {
    /// Connect to a STUN server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        info!("STUN client {} initializing for {}", client_id, remote_addr);

        // Bind local UDP socket (0.0.0.0:0 for any address/port)
        let local_addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let udp_socket = tokio::net::UdpSocket::bind(local_addr)
            .await
            .context("Failed to bind UDP socket")?;

        let bound_addr = udp_socket.local_addr()?;
        info!("STUN client {} bound to local address {}", client_id, bound_addr);

        // Store socket and STUN server address in protocol_data
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "stun_server".to_string(),
                serde_json::json!(remote_addr),
            );
            client.set_protocol_field(
                "local_addr".to_string(),
                serde_json::json!(bound_addr.to_string()),
            );
        }).await;

        // Update status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        console_info!(status_tx, "[CLIENT] STUN client {} ready for {}", client_id, remote_addr);
        console_info!(status_tx, "__UPDATE_UI__");

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(StunClientProtocol::new());
            let event = Event::new(
                &STUN_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "local_addr": bound_addr.to_string(),
                    "stun_server": remote_addr,
                }),
            );

            let llm_clone = llm_client.clone();
            let app_state_clone = app_state.clone();
            let status_tx_clone = status_tx.clone();

            tokio::spawn(async move {
                match call_llm_for_client(
                    &llm_clone,
                    &app_state_clone,
                    client_id.to_string(),
                    &instruction,
                    "",
                    Some(&event),
                    protocol.as_ref(),
                    &status_tx_clone,
                ).await {
                    Ok(ClientLlmResult { actions, memory_updates }) => {
                        // Update memory
                        if let Some(mem) = memory_updates {
                            app_state_clone.set_memory_for_client(client_id, mem).await;
                        }

                        // Execute actions
                        for action in actions {
                            if let Err(e) = Self::execute_stun_action(
                                client_id,
                                action,
                                app_state_clone.clone(),
                                llm_clone.clone(),
                                status_tx_clone.clone(),
                            ).await {
                                error!("Failed to execute STUN action: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("LLM error for STUN client {}: {}", client_id, e);
                    }
                }
            });
        }

        // Spawn background task that monitors for client removal
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("STUN client {} stopped", client_id);
                    break;
                }
            }
        });

        Ok(bound_addr)
    }

    /// Execute a STUN action (internal helper)
    async fn execute_stun_action(
        client_id: ClientId,
        action: serde_json::Value,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        use crate::llm::actions::client_trait::Client;
        let protocol = Arc::new(StunClientProtocol::new());

        match protocol.as_ref().execute_action(action)? {
            crate::llm::actions::client_trait::ClientActionResult::Custom { name, data: _ } => {
                if name == "send_binding_request" {
                    Self::send_binding_request(client_id, app_state, llm_client, status_tx).await?;
                }
            }
            crate::llm::actions::client_trait::ClientActionResult::Disconnect => {
                app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                console_info!(status_tx, "[CLIENT] STUN client {} disconnected", client_id);
                console_info!(status_tx, "__UPDATE_UI__");
            }
            _ => {}
        }

        Ok(())
    }

    /// Send STUN binding request
    pub async fn send_binding_request(
        client_id: ClientId,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get STUN server address from client
        let stun_server = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("stun_server")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No STUN server found")?;

        info!("STUN client {} sending binding request to {}", client_id, stun_server);

        // Resolve STUN server address (may be hostname:port)
        let stun_sock_addr: SocketAddr = tokio::net::lookup_host(&stun_server)
            .await
            .context(format!("Failed to resolve STUN server: {}", stun_server))?
            .next()
            .context("No addresses found for STUN server")?;

        // Bind a new UDP socket for the query
        let local_addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let udp_socket = tokio::net::UdpSocket::bind(local_addr)
            .await
            .context("Failed to bind UDP socket for STUN query")?;

        // Create STUN client and query external address
        let stun_client = stunclient::StunClient::new(stun_sock_addr);

        match stun_client.query_external_address_async(&udp_socket).await {
            Ok(external_addr) => {
                info!("STUN client {} discovered external address: {}", client_id, external_addr);

                let local_addr = udp_socket.local_addr()?;

                // Call LLM with binding response
                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                    let protocol = Arc::new(StunClientProtocol::new());
                    let event = Event::new(
                        &STUN_CLIENT_BINDING_RESPONSE_EVENT,
                        serde_json::json!({
                            "external_ip": external_addr.ip().to_string(),
                            "external_port": external_addr.port(),
                            "external_addr": external_addr.to_string(),
                            "local_addr": local_addr.to_string(),
                            "stun_server": stun_server,
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

                            // Note: We don't execute follow-up actions here to avoid recursion
                            // The LLM response is primarily for interpretation/logging
                        }
                        Err(e) => {
                            error!("LLM error for STUN client {}: {}", client_id, e);
                        }
                    }
                }

                Ok(())
            }
            Err(e) => {
                console_error!(status_tx, "[ERROR] STUN binding request failed: {}", e);
                Err(e.into())
            }
        }
    }
}
