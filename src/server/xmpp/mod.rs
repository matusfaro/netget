//! XMPP server implementation
pub mod actions;

use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::logging::emit::Log;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use actions::{XmppProtocol, XMPP_DATA_RECEIVED_EVENT};
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

/// Upper bound on the un-consumed XML a single connection may accumulate.
///
/// `wait_for_more` deliberately keeps the buffer across events, so without a ceiling a peer
/// that never completes a stanza - or a model that answers `wait_for_more` forever - grows it
/// without limit, and every subsequent event re-sends the whole thing to the model.
const MAX_XMPP_BUFFER_BYTES: usize = 256 * 1024;

/// XMPP server that forwards XML stanzas to LLM
pub struct XmppServer;

impl XmppServer {
    /// Spawn XMPP server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        domain: String,
    ) -> Result<SocketAddr> {
        let listener =
            crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        Log::new(Some(&status_tx)).info(format!(
            "XMPP server (action-based) listening on {} for domain {}",
            local_addr, domain
        ));

        let protocol = Arc::new(XmppProtocol::with_domain(domain));

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

                        Log::new(Some(&status_clone)).debug(format!(
                            "XMPP connection {} from {}",
                            connection_id, remote_addr
                        ));

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

                            // Peer messaging: the dashboard's "message this peer" /
                            // "disconnect this peer" inject actions into THIS connection through
                            // the same executor the LLM path uses. Registered before the first
                            // LLM call because a manual `*` rule can park it for minutes and the
                            // operator must be able to reach the connection while it waits. Every
                            // XMPP wire verb returns Output / Multiple / CloseConnection, so the
                            // generic peer task covers the whole vocabulary (no Custom gap).
                            let peer_rx = crate::server::peer_support::register_peer_channel(
                                &state_clone,
                                server_id,
                                connection_id.as_u32(),
                            )
                            .await;
                            crate::server::peer_support::spawn_peer_command_task(
                                peer_rx,
                                protocol_clone.clone(),
                                state_clone.clone(),
                                server_id,
                                connection_id.as_u32(),
                                write_half_arc.clone(),
                                status_clone.clone(),
                            );

                            // Create XML buffer for streaming parsing
                            let mut read_half = read_half;
                            let mut buffer = Vec::new();
                            let mut temp_buf = vec![0u8; 4096];

                            loop {
                                match read_half.read(&mut temp_buf).await {
                                    Ok(0) => {
                                        Log::new(Some(&status_clone)).debug(format!(
                                            "XMPP connection {} closed by client",
                                            connection_id
                                        ));
                                        break;
                                    }
                                    Ok(n) => {
                                        buffer.extend_from_slice(&temp_buf[..n]);

                                        // Live ↓ counter + last_activity for the dashboard.
                                        state_clone
                                            .update_connection_stats(
                                                server_id,
                                                connection_id,
                                                Some(n as u64),
                                                None,
                                                Some(1),
                                                None,
                                            )
                                            .await;

                                        if buffer.len() > MAX_XMPP_BUFFER_BYTES {
                                            Log::new(Some(&status_clone)).error(format!(
                                                "XMPP connection {} buffered {} bytes without \
                                                 completing a stanza (limit {}), closing",
                                                connection_id,
                                                buffer.len(),
                                                MAX_XMPP_BUFFER_BYTES
                                            ));
                                            break;
                                        }

                                        // Byte-count summary and full XML are FileOnly.
                                        let log = Log::new(Some(&status_clone));
                                        log.debug(format!(
                                            "XMPP received {} bytes on connection {}",
                                            n, connection_id
                                        ));
                                        log.trace(format!(
                                            "XMPP data (XML): {}",
                                            String::from_utf8_lossy(&buffer)
                                        ));

                                        // Try to parse XML stanzas from buffer
                                        // For simplicity, we'll pass the entire buffer to LLM for parsing
                                        // A more sophisticated implementation would parse individual stanzas

                                        let xml_data = String::from_utf8_lossy(&buffer).to_string();

                                        // Create event for LLM
                                        let event = Event::new(
                                            &XMPP_DATA_RECEIVED_EVENT,
                                            serde_json::json!({
                                                "xml_data": xml_data
                                            }),
                                        );

                                        log.debug(format!(
                                            "XMPP calling LLM for connection {}",
                                            connection_id
                                        ));

                                        match call_llm(
                                            &llm_clone,
                                            &state_clone,
                                            server_id,
                                            Some(connection_id),
                                            &event,
                                            protocol_clone.as_ref(),
                                        )
                                        .await
                                        {
                                            Ok(execution_result) => {
                                                for message in &execution_result.messages {
                                                    log.info(format!("{}", message));
                                                }

                                                log.debug(format!(
                                                    "XMPP got {} protocol results",
                                                    execution_result.protocol_results.len()
                                                ));

                                                let mut should_close = false;
                                                let mut wait_for_more = false;

                                                for protocol_result in
                                                    execution_result.protocol_results
                                                {
                                                    match protocol_result {
                                                        ActionResult::Output(data) => {
                                                            let xml_str =
                                                                String::from_utf8_lossy(&data);
                                                            let mut write =
                                                                write_half_arc.lock().await;
                                                            let _ = write.write_all(&data).await;
                                                            let _ = write.flush().await;
                                                            drop(write);

                                                            // Live ↑ counter + last_activity.
                                                            state_clone
                                                                .update_connection_stats(
                                                                    server_id,
                                                                    connection_id,
                                                                    None,
                                                                    Some(data.len() as u64),
                                                                    None,
                                                                    Some(1),
                                                                )
                                                                .await;

                                                            // Byte-count summary and full XML FileOnly.
                                                            log.debug(format!("XMPP sent {} bytes on connection {}", data.len(), connection_id));
                                                            log.trace(format!(
                                                                "XMPP sent (XML): {}",
                                                                xml_str
                                                            ));
                                                        }
                                                        ActionResult::CloseConnection => {
                                                            should_close = true;
                                                        }
                                                        ActionResult::WaitForMore => {
                                                            // Keep buffer and wait for more data
                                                            wait_for_more = true;
                                                            log.debug("XMPP waiting for more data");
                                                        }
                                                        _ => {}
                                                    }
                                                }

                                                if should_close {
                                                    break;
                                                }

                                                // Only consume the buffer once the model has
                                                // acted on it. Clearing unconditionally made
                                                // `wait_for_more` a no-op: the partial stanza
                                                // it asked to hold on to was thrown away, and
                                                // the continuation arrived without its opening
                                                // tag.
                                                if !wait_for_more {
                                                    buffer.clear();
                                                }
                                            }
                                            Err(e) => {
                                                // Keep the buffer: the model never saw this
                                                // data, so dropping it would lose a stanza.
                                                log.warn(format!("XMPP LLM call failed: {}", e));
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        Log::new(Some(&status_clone)).error(format!(
                                            "XMPP read error on connection {}: {}",
                                            connection_id, e
                                        ));
                                        break;
                                    }
                                }
                            }

                            // Every exit path - EOF, buffer overflow, close_stream, read error -
                            // breaks out of the loop and lands here. Drop the peer handle so the
                            // dashboard stops offering to message a dead connection; idempotent
                            // with the peer task's own removal on an injected close.
                            state_clone
                                .remove_peer_handle(server_id, connection_id.as_u32())
                                .await;

                            // Connection closed - mark as closed
                            state_clone
                                .close_connection_on_server(server_id, connection_id)
                                .await;
                            let _ = status_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        Log::new(Some(&status_tx))
                            .error(format!("Failed to accept XMPP connection: {}", e));
                        break;
                    }
                }
            }
        });

        task_registrar
            .register_server_task(server_id, accept_handle)
            .await;

        Ok(local_addr)
    }
}
