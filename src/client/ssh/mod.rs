//! SSH client implementation
pub mod actions;

pub use actions::SshClientProtocol;

use anyhow::{Context, Result};
use russh::client::{self, Handle};
use russh::*;
use russh_keys::*;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace, warn};

use crate::client::ssh::actions::{SSH_CLIENT_CONNECTED_EVENT, SSH_CLIENT_OUTPUT_RECEIVED_EVENT};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::{Client, ClientActionResult};
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// Per-client data for LLM handling
struct ClientData {
    memory: String,
}

/// SSH client handler
struct ClientHandler;

#[async_trait::async_trait]
impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &key::PublicKey,
    ) -> Result<bool, Self::Error> {
        // Accept all server keys (for testing)
        // In production, this should verify against known_hosts
        Ok(true)
    }
}

/// SSH client that connects to a remote SSH server
pub struct SshClient;

impl SshClient {
    /// Connect to an SSH server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        // Parse startup parameters
        let params = startup_params.context("Missing required startup parameters for SSH client")?;

        let username = params.get_string("username");
        let password = params.get_optional_string("password");
        let auth_method = params.get_optional_string("auth_method")
            .unwrap_or_else(|| "password".to_string());

        if auth_method != "password" {
            return Err(anyhow::anyhow!("Only password authentication is currently supported"));
        }

        if password.is_none() {
            return Err(anyhow::anyhow!("Password is required for password authentication"));
        }

        let password = password.unwrap();

        console_info!(status_tx, "[CLIENT] SSH connecting to {} as {}", remote_addr, username);

        // Parse address
        let addr = match remote_addr.parse::<SocketAddr>() {
            Ok(addr) => addr,
            Err(_) => {
                // Try to resolve hostname
                let parts: Vec<&str> = remote_addr.split(':').collect();
                if parts.len() != 2 {
                    return Err(anyhow::anyhow!("Invalid address format: {}", remote_addr));
                }
                let host = parts[0];
                let port: u16 = parts[1].parse()
                    .context("Invalid port number")?;

                let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
                    .await
                    .context(format!("Failed to resolve hostname: {}", host))?
                    .collect();

                addrs.first().cloned()
                    .ok_or_else(|| anyhow::anyhow!("No addresses found for {}", host))?
            }
        };

        // Create SSH config
        let config = client::Config {
            inactivity_timeout: Some(std::time::Duration::from_secs(300)),
            ..<_>::default()
        };

        // Connect to SSH server
        let mut session = client::connect(Arc::new(config), addr, ClientHandler)
            .await
            .context("Failed to connect to SSH server")?;

        // Authenticate
        let auth_result = session
            .authenticate_password(username.clone(), password)
            .await
            .context("SSH authentication failed")?;

        if !auth_result {
            return Err(anyhow::anyhow!("SSH authentication failed: incorrect credentials"));
        }

        console_info!(status_tx, "[CLIENT] SSH client {} authenticated", client_id);

        // Update client status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        console_info!(status_tx, "__UPDATE_UI__");

        // Get local address (use the connected socket address)
        let local_addr = addr; // russh doesn't expose local addr easily, using remote for now

        // Trigger connected event to LLM
        let protocol = Arc::new(SshClientProtocol::new());
        let connected_event = Event::new(
            &SSH_CLIENT_CONNECTED_EVENT,
            serde_json::json!({
                "remote_addr": remote_addr,
                "username": username,
            }),
        );

        // Initialize client data
        let client_data = Arc::new(Mutex::new(ClientData {
            memory: String::new(),
        }));

        // Clone for the spawned task
        let session_arc = Arc::new(Mutex::new(session));
        let client_data_clone = client_data.clone();
        let protocol_clone = protocol.clone();
        let llm_client_clone = llm_client.clone();
        let app_state_clone = app_state.clone();
        let status_tx_clone = status_tx.clone();

        // Call LLM with connected event
        tokio::spawn(async move {
            if let Some(instruction) = app_state_clone.get_instruction_for_client(client_id).await {
                // Call LLM with connected event
                match call_llm_for_client(
                    &llm_client_clone,
                    &app_state_clone,
                    client_id.to_string(),
                    &instruction,
                    &client_data_clone.lock().await.memory,
                    Some(&connected_event),
                    protocol_clone.as_ref(),
                    &status_tx_clone,
                )
                .await
                {
                    Ok(ClientLlmResult {
                        actions,
                        memory_updates,
                    }) => {
                        // Update memory
                        if let Some(mem) = memory_updates {
                            client_data_clone.lock().await.memory = mem;
                        }

                        // Execute initial actions
                        for action in actions {
                            if let Err(e) = Self::execute_ssh_action(
                                &session_arc,
                                &protocol_clone,
                                action,
                                client_id,
                                &llm_client_clone,
                                &app_state_clone,
                                &status_tx_clone,
                                &client_data_clone,
                            )
                            .await
                            {
                                error!("Error executing SSH action: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("LLM error for SSH client {}: {}", client_id, e);
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Execute an SSH action (helper function)
    async fn execute_ssh_action(
        session_arc: &Arc<Mutex<Handle<ClientHandler>>>,
        protocol: &Arc<SshClientProtocol>,
        action: serde_json::Value,
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        client_data: &Arc<Mutex<ClientData>>,
    ) -> Result<()> {
        match protocol.as_ref().execute_action(action.clone())? {
            ClientActionResult::Custom { name, data } if name == "execute_command" => {
                let command = data
                    .get("command")
                    .and_then(|v| v.as_str())
                    .context("Missing command in action data")?;

                console_info!(status_tx, "[CLIENT] SSH executing: {}", command);

                // Open channel and execute command
                let session = session_arc.lock().await;
                let mut channel = session
                    .channel_open_session()
                    .await
                    .context("Failed to open SSH channel")?;

                channel
                    .exec(true, command)
                    .await
                    .context("Failed to execute command")?;

                // Read output
                let mut output = Vec::new();
                let mut exit_code: Option<u32> = None;

                loop {
                    match channel.wait().await {
                        Some(ChannelMsg::Data { ref data }) => {
                            output.extend_from_slice(data);
                            trace!("SSH client {} received {} bytes of output", client_id, data.len());
                        }
                        Some(ChannelMsg::ExitStatus { exit_status }) => {
                            exit_code = Some(exit_status);
                            debug!("SSH command exit status: {}", exit_status);
                        }
                        Some(ChannelMsg::Eof) => {
                            debug!("SSH channel EOF");
                            break;
                        }
                        Some(_) => {}
                        None => break,
                    }
                }

                let output_str = String::from_utf8_lossy(&output).to_string();
                trace!("SSH command output: {}", output_str);

                // Call LLM with output
                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                    let mut event_data = serde_json::json!({
                        "output": output_str,
                    });

                    if let Some(code) = exit_code {
                        event_data["exit_code"] = serde_json::json!(code);
                    }

                    let output_event = Event::new(&SSH_CLIENT_OUTPUT_RECEIVED_EVENT, event_data);

                    match call_llm_for_client(
                        llm_client,
                        app_state,
                        client_id.to_string(),
                        &instruction,
                        &client_data.lock().await.memory,
                        Some(&output_event),
                        protocol.as_ref(),
                        status_tx,
                    )
                    .await
                    {
                        Ok(ClientLlmResult {
                            actions,
                            memory_updates,
                        }) => {
                            // Update memory
                            if let Some(mem) = memory_updates {
                                client_data.lock().await.memory = mem;
                            }

                            // Execute follow-up actions
                            for next_action in actions {
                                // Recursive call for follow-up commands (boxed to avoid infinite size)
                                let session_clone = session_arc.clone();
                                let protocol_clone = protocol.clone();
                                let llm_clone = llm_client.clone();
                                let app_clone = app_state.clone();
                                let status_clone = status_tx.clone();
                                let data_clone = client_data.clone();

                                if let Err(e) = Box::pin(Self::execute_ssh_action(
                                    &session_clone,
                                    &protocol_clone,
                                    next_action,
                                    client_id,
                                    &llm_clone,
                                    &app_clone,
                                    &status_clone,
                                    &data_clone,
                                ))
                                .await
                                {
                                    error!("Error executing follow-up SSH action: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            error!("LLM error for SSH client {}: {}", client_id, e);
                        }
                    }
                }

                Ok(())
            }
            ClientActionResult::Disconnect => {
                console_info!(status_tx, "[CLIENT] SSH client {} disconnecting", client_id);
                app_state
                    .update_client_status(client_id, ClientStatus::Disconnected)
                    .await;
                console_info!(status_tx, "__UPDATE_UI__");

                // Close session
                let session = session_arc.lock().await;
                session.disconnect(Disconnect::ByApplication, "", "en").await?;

                Ok(())
            }
            ClientActionResult::WaitForMore => {
                // No-op for SSH (commands are discrete)
                Ok(())
            }
            _ => {
                warn!("Unexpected action result for SSH client");
                Ok(())
            }
        }
    }
}
