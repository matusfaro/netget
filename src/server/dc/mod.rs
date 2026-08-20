//! DC (Direct Connect) server implementation
pub mod actions;

use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::logging::emit::Log;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::server::DcProtocol;
use crate::state::app_state::AppState;
use actions::DC_COMMAND_RECEIVED_EVENT;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

/// DC server that forwards commands to LLM
pub struct DcServer;

impl DcServer {
    /// Spawn DC server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let listener =
            crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        Log::new(Some(&status_tx)).info(format!(
            "DC server (action-based) listening on {}",
            local_addr
        ));

        let protocol = Arc::new(DcProtocol::new());

        let task_registrar = app_state.clone();
        let accept_handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();

                        tokio::spawn(async move {
                            let (read_half, write_half) = tokio::io::split(stream);
                            let write_half_arc = Arc::new(tokio::sync::Mutex::new(write_half));

                            // Add connection to ServerInstance
                            use crate::state::server::{
                                ConnectionState as ServerConnectionState, ConnectionStatus,
                                ProtocolConnectionInfo,
                            };
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
                                protocol_info: ProtocolConnectionInfo::empty(),
                            };
                            state_clone
                                .add_connection_to_server(server_id, conn_state)
                                .await;
                            let _ = status_clone.send("__UPDATE_UI__".to_string());

                            // Register the client in the protocol's own state map. This was
                            // never called from anywhere, so `DcProtocol::clients` stayed
                            // permanently empty and every accessor that goes through
                            // `get_mut` - set_nickname, set_client_info, set_operator - was a
                            // silent no-op, while get_nickname always returned None.
                            protocol_clone.add_connection(connection_id).await;

                            // Send initial $Lock challenge
                            let lock_command =
                                "$Lock EXTENDEDPROTOCOLABCABCABCABCABCABC Pk=NetGetHub|";
                            {
                                let mut writer = write_half_arc.lock().await;
                                if let Err(e) = writer.write_all(lock_command.as_bytes()).await {
                                    Log::new(Some(&status_clone))
                                        .error(format!("Failed to send initial Lock: {}", e));
                                    return;
                                }
                                Log::new(Some(&status_clone))
                                    .debug(format!("DC sent {} bytes (Lock)", lock_command.len()));

                                // Update stats
                                state_clone
                                    .update_connection_stats(
                                        server_id,
                                        connection_id,
                                        Some(0),
                                        Some(lock_command.len() as u64),
                                        Some(0),
                                        Some(1),
                                    )
                                    .await;
                            }

                            let mut read_half = read_half;
                            let mut buffer = Vec::new();

                            loop {
                                let mut byte = [0u8; 1];
                                match read_half.read_exact(&mut byte).await {
                                    Ok(_) => {
                                        buffer.push(byte[0]);

                                        // Check for pipe delimiter
                                        if byte[0] == b'|' {
                                            // We have a complete command
                                            let command_bytes = buffer.clone();
                                            buffer.clear();

                                            // Convert to string
                                            let command_str =
                                                match String::from_utf8(command_bytes.clone()) {
                                                    Ok(s) => s,
                                                    Err(e) => {
                                                        Log::new(Some(&status_clone)).warn(
                                                            format!(
                                                                "DC received non-UTF8 data: {}",
                                                                e
                                                            ),
                                                        );
                                                        continue;
                                                    }
                                                };

                                            // Remove trailing pipe
                                            let command = command_str.trim_end_matches('|');

                                            // Byte-count summary and full command are FileOnly.
                                            let preview =
                                                crate::utils::truncate_for_log(command, 100);
                                            let log = Log::new(Some(&status_clone));
                                            log.debug(format!(
                                                "DC received {} bytes on connection {}: {}",
                                                command_bytes.len(),
                                                connection_id,
                                                preview
                                            ));
                                            log.trace(format!("DC command: {:?}", command));

                                            // Update receive stats
                                            state_clone
                                                .update_connection_stats(
                                                    server_id,
                                                    connection_id,
                                                    Some(command_bytes.len() as u64),
                                                    Some(0),
                                                    Some(1),
                                                    Some(0),
                                                )
                                                .await;

                                            // Record the nickname the client just claimed.
                                            //
                                            // Nothing ever called `set_nickname`, so
                                            // `client_nickname` below was null on every event
                                            // this hub has ever raised - the field is
                                            // documented as carrying the nickname "if set",
                                            // and it never was. NMDC announces it in exactly
                                            // two places: `$ValidateNick <nick>` at login, and
                                            // the second field of `$MyINFO $ALL <nick> ...`.
                                            if let Some(rest) =
                                                command.strip_prefix("$ValidateNick ")
                                            {
                                                if let Some(nick) = rest.split_whitespace().next() {
                                                    protocol_clone
                                                        .set_nickname(
                                                            connection_id,
                                                            nick.to_string(),
                                                        )
                                                        .await;
                                                }
                                            } else if let Some(rest) =
                                                command.strip_prefix("$MyINFO ")
                                            {
                                                let mut fields = rest.split_whitespace();
                                                if fields.next() == Some("$ALL") {
                                                    if let Some(nick) = fields.next() {
                                                        protocol_clone
                                                            .set_nickname(
                                                                connection_id,
                                                                nick.to_string(),
                                                            )
                                                            .await;
                                                    }
                                                }
                                            }

                                            // Parse command type
                                            let command_type = if command.starts_with('$') {
                                                command
                                                    .split_whitespace()
                                                    .next()
                                                    .unwrap_or("$Unknown")
                                                    .trim_start_matches('$')
                                            } else if command.starts_with('<') {
                                                "Chat"
                                            } else {
                                                "Unknown"
                                            };

                                            // Get client nickname if available
                                            let client_nickname =
                                                protocol_clone.get_nickname(&connection_id).await;

                                            let event = Event::new(
                                                &DC_COMMAND_RECEIVED_EVENT,
                                                serde_json::json!({
                                                    "command": command,
                                                    "command_type": command_type,
                                                    "client_nickname": client_nickname,
                                                }),
                                            );

                                            Log::new(Some(&status_clone)).debug(format!(
                                                "DC calling LLM for connection {}",
                                                connection_id
                                            ));

                                            let result = call_llm(
                                                &llm_clone,
                                                &state_clone,
                                                server_id,
                                                Some(connection_id),
                                                &event,
                                                protocol_clone.as_ref(),
                                            )
                                            .await;

                                            match result {
                                                Ok(execution_result) => {
                                                    let log = Log::new(Some(&status_clone));
                                                    for message in &execution_result.messages {
                                                        log.info(format!("{}", message));
                                                    }

                                                    log.debug(format!(
                                                        "DC got {} protocol results",
                                                        execution_result.protocol_results.len()
                                                    ));

                                                    for protocol_result in
                                                        execution_result.protocol_results
                                                    {
                                                        match protocol_result {
                                                            ActionResult::Output(data) => {
                                                                let mut write =
                                                                    write_half_arc.lock().await;
                                                                if let Err(e) =
                                                                    write.write_all(&data).await
                                                                {
                                                                    log.error(format!("Failed to write DC response: {}", e));
                                                                    break;
                                                                }
                                                                let _ = write.flush().await;

                                                                log.debug(format!("DC sent {} bytes on connection {}", data.len(), connection_id));
                                                                log.trace(format!(
                                                                    "DC sent data: {:?}",
                                                                    String::from_utf8_lossy(&data)
                                                                ));
                                                            }
                                                            ActionResult::CloseConnection => {
                                                                log.debug(format!(
                                                                    "DC closing connection {}",
                                                                    connection_id
                                                                ));
                                                                break;
                                                            }
                                                            _ => {}
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    Log::new(Some(&status_clone))
                                                        .warn(format!("LLM call failed: {}", e));
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        if e.kind() == std::io::ErrorKind::UnexpectedEof {
                                            Log::new(Some(&status_clone)).debug(format!(
                                                "DC connection {} closed by client",
                                                connection_id
                                            ));
                                        } else {
                                            Log::new(Some(&status_clone)).error(format!(
                                                "DC read error on connection {}: {}",
                                                connection_id, e
                                            ));
                                        }
                                        break;
                                    }
                                }
                            }

                            // Clean up connection
                            protocol_clone.remove_connection(&connection_id).await;
                            state_clone
                                .remove_connection_from_server(server_id, connection_id)
                                .await;
                            let _ = status_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        Log::new(Some(&status_tx))
                            .error(format!("Failed to accept DC connection: {}", e));
                    }
                }
            }
        });

        // Register the accept loop so stop_server can abort it and release the port.
        task_registrar
            .register_server_task(server_id, accept_handle)
            .await;

        Ok(local_addr)
    }
}
