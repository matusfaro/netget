//! SSH server implementation - simplified

use bytes::Bytes;
use crate::network::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::ollama_client::OllamaClient;
use crate::llm::prompt::PromptBuilder;
use crate::llm::{ActionResponse, execute_actions, NetworkContext, ProtocolActions, ActionResult};
use crate::network::SshProtocol;
use crate::state::app_state::AppState;

/// Get LLM context and output format instructions for SSH stack
pub fn get_llm_protocol_prompt() -> (&'static str, &'static str) {
    let context = r#"You are handling SSH protocol (port 22).
Handle SSH handshake, authentication, and shell sessions.
Respond with appropriate SSH protocol messages."#;

    let output_format = r#"IMPORTANT: Respond with a JSON object:
{
  "output": "SSH protocol data to send (null if no response)",
  "close_connection": false,  // Close this connection after sending
  "message": null,  // Optional message for user
  "set_memory": null,  // Replace memory
  "append_memory": null  // Append to memory
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
        let listener = crate::network::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
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

                            loop {
                                match read_half.read(&mut buffer).await {
                                    Ok(0) => {
                                        let _ = status_clone.send(format!("✗ SSH connection {} closed", connection_id));
                                        break;
                                    }
                                    Ok(n) => {
                                        let data = Bytes::copy_from_slice(&buffer[..n]);

                                        // DEBUG: Log summary with data preview
                                        if data.iter().all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace()) {
                                            let data_str = String::from_utf8_lossy(&data);
                                            let preview = if data_str.len() > 100 {
                                                format!("{}...", &data_str[..100])
                                            } else {
                                                data_str.to_string()
                                            };
                                            debug!("SSH received {} bytes on connection {}: {}", n, connection_id, preview);
                                            let _ = status_clone.send(format!("[DEBUG] SSH received {} bytes on connection {}: {}", n, connection_id, preview));

                                            // TRACE: Log full text payload
                                            trace!("SSH data (text): {:?}", data_str);
                                            let _ = status_clone.send(format!("[TRACE] SSH data (text): {:?}", data_str));
                                        } else {
                                            debug!("SSH received {} bytes on connection {} (binary data)", n, connection_id);
                                            let _ = status_clone.send(format!("[DEBUG] SSH received {} bytes on connection {} (binary data)", n, connection_id));

                                            // TRACE: Log full hex payload
                                            let hex_str = hex::encode(&data);
                                            trace!("SSH data (hex): {}", hex_str);
                                            let _ = status_clone.send(format!("[TRACE] SSH data (hex): {}", hex_str));
                                        }

                                        let model = state_clone.get_ollama_model().await;
                                        let prompt_config = get_llm_protocol_prompt();

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
                                            &event_description,
                                            prompt_config,
                                        ).await;

                                        match llm_clone.generate_llm_response(&model, &prompt).await {
                                            Ok(response) => {
                                                // Handle common actions
                                                use crate::llm::response_handler;
                                                let processed = response_handler::handle_llm_response(response, &state_clone).await;

                                                // Send output
                                                if let Some(output) = processed.output {
                                                    let output_data = output.as_bytes();
                                                    if let Err(e) = write_half.write_all(output_data).await {
                                                        error!("Failed to send SSH response: {}", e);
                                                        break;
                                                    }

                                                    // DEBUG: Log summary with data preview
                                                    if output_data.iter().all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace()) {
                                                        let data_str = String::from_utf8_lossy(output_data);
                                                        let preview = if data_str.len() > 100 {
                                                            format!("{}...", &data_str[..100])
                                                        } else {
                                                            data_str.to_string()
                                                        };
                                                        debug!("SSH sent {} bytes on connection {}: {}", output_data.len(), connection_id, preview);
                                                        let _ = status_clone.send(format!("[DEBUG] SSH sent {} bytes on connection {}: {}", output_data.len(), connection_id, preview));

                                                        // TRACE: Log full text payload
                                                        trace!("SSH sent (text): {:?}", data_str);
                                                        let _ = status_clone.send(format!("[TRACE] SSH sent (text): {:?}", data_str));
                                                    } else {
                                                        debug!("SSH sent {} bytes on connection {} (binary data)", output_data.len(), connection_id);
                                                        let _ = status_clone.send(format!("[DEBUG] SSH sent {} bytes on connection {} (binary data)", output_data.len(), connection_id));

                                                        // TRACE: Log full hex payload
                                                        let hex_str = hex::encode(output_data);
                                                        trace!("SSH sent (hex): {}", hex_str);
                                                        let _ = status_clone.send(format!("[TRACE] SSH sent (hex): {}", hex_str));
                                                    }

                                                    let _ = status_clone.send(format!("→ SSH to {}: {} bytes", connection_id, output_data.len()));
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

    /// Spawn SSH server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        send_first: bool,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let listener = crate::network::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("SSH server (action-based) listening on {}", local_addr);

        let protocol = Arc::new(SshProtocol::new());

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new();
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            let (mut read_half, write_half) = tokio::io::split(stream);
                            let write_half_arc = Arc::new(tokio::sync::Mutex::new(write_half));

                            // Add connection to ServerInstance
                            use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus, ProtocolState};
                            let now = std::time::Instant::now();
                            let conn_state = ServerConnectionState {
                                id: connection_id,
                                remote_addr,
                                local_addr: local_addr_conn,
                                bytes_sent: 0,
                                bytes_received: 0,
                                packets_sent: 0,
                                packets_received: 0,
                                last_activity: now,
                                status: ConnectionStatus::Active,
                                status_changed_at: now,
                                protocol_info: ProtocolConnectionInfo::Ssh {
                                    write_half: write_half_arc.clone(),
                                    state: ProtocolState::Idle,
                                    queued_data: Vec::new(),
                                },
                            };
                            state_clone.add_connection_to_server(server_id, conn_state).await;
                            let _ = status_clone.send("__UPDATE_UI__".to_string());
                            let model = state_clone.get_ollama_model().await;

                            // Send banner if requested
                            if send_first {
                                let context = NetworkContext::SshConnection { connection_id, write_half: write_half_arc.clone(), status_tx: status_clone.clone() };
                                let protocol_actions = protocol_clone.get_sync_actions(&context);
                                let prompt = PromptBuilder::build_network_event_action_prompt(
                                    &state_clone, "SSH connection opened", protocol_actions).await;

                                if let Ok(llm_output) = llm_clone.generate(&model, &prompt).await {
                                    if let Ok(action_response) = ActionResponse::from_str(&llm_output) {
                                        if let Ok(result) = execute_actions(action_response.actions, &state_clone,
                                            Some(protocol_clone.as_ref()), Some(&context)).await {
                                            for protocol_result in result.protocol_results {
                                                if let ActionResult::Output(data) = protocol_result {
                                                    let mut write = write_half_arc.lock().await;
                                                    let _ = write.write_all(&data).await;
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // Read loop
                            let mut buffer = vec![0u8; 8192];
                            loop {
                                match read_half.read(&mut buffer).await {
                                    Ok(0) => break,
                                    Ok(n) => {
                                        let data = Bytes::copy_from_slice(&buffer[..n]);

                                        // DEBUG: Log summary with data preview
                                        if data.iter().all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace()) {
                                            let data_str = String::from_utf8_lossy(&data);
                                            let preview = if data_str.len() > 100 {
                                                format!("{}...", &data_str[..100])
                                            } else {
                                                data_str.to_string()
                                            };
                                            debug!("SSH received {} bytes on connection {}: {}", n, connection_id, preview);
                                            let _ = status_clone.send(format!("[DEBUG] SSH received {} bytes on connection {}: {}", n, connection_id, preview));

                                            // TRACE: Log full text payload
                                            trace!("SSH data (text): {:?}", data_str);
                                            let _ = status_clone.send(format!("[TRACE] SSH data (text): {:?}", data_str));
                                        } else {
                                            debug!("SSH received {} bytes on connection {} (binary data)", n, connection_id);
                                            let _ = status_clone.send(format!("[DEBUG] SSH received {} bytes on connection {} (binary data)", n, connection_id));

                                            // TRACE: Log full hex payload
                                            let hex_str = hex::encode(&data);
                                            trace!("SSH data (hex): {}", hex_str);
                                            let _ = status_clone.send(format!("[TRACE] SSH data (hex): {}", hex_str));
                                        }

                                        let event_description = format!("SSH data received: {:?}", data);
                                        let context = NetworkContext::SshConnection { connection_id, write_half: write_half_arc.clone(), status_tx: status_clone.clone() };
                                        let protocol_actions = protocol_clone.get_sync_actions(&context);
                                        let prompt = PromptBuilder::build_network_event_action_prompt(
                                            &state_clone, &event_description, protocol_actions).await;

                                        if let Ok(llm_output) = llm_clone.generate(&model, &prompt).await {
                                            if let Ok(action_response) = ActionResponse::from_str(&llm_output) {
                                                if let Ok(result) = execute_actions(action_response.actions, &state_clone,
                                                    Some(protocol_clone.as_ref()), Some(&context)).await {
                                                    for protocol_result in result.protocol_results {
                                                        match protocol_result {
                                                            ActionResult::Output(data) => {
                                                                let mut write = write_half_arc.lock().await;
                                                                let _ = write.write_all(&data).await;

                                                                // DEBUG: Log summary
                                                                debug!("SSH sent {} bytes on connection {}", data.len(), connection_id);
                                                                let _ = status_clone.send(format!("[DEBUG] SSH sent {} bytes on connection {}", data.len(), connection_id));

                                                                // TRACE: Log full payload
                                                                if data.iter().all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace()) {
                                                                    let data_str = String::from_utf8_lossy(&data);
                                                                    trace!("SSH sent (text): {:?}", data_str);
                                                                    let _ = status_clone.send(format!("[TRACE] SSH sent (text): {:?}", data_str));
                                                                } else {
                                                                    let hex_str = hex::encode(&data);
                                                                    trace!("SSH sent (hex): {}", hex_str);
                                                                    let _ = status_clone.send(format!("[TRACE] SSH sent (hex): {}", hex_str));
                                                                }
                                                            }
                                                            ActionResult::CloseConnection => break,
                                                            _ => {}
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Err(_) => break,
                                }
                            }

                            // Connection closed - mark as closed
                            state_clone.close_connection_on_server(server_id, connection_id).await;
                            let _ = status_clone.send("__UPDATE_UI__".to_string());
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