//! WHOIS server implementation
pub mod actions;

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use actions::WHOIS_QUERY_EVENT;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

pub struct WhoisServer;

impl WhoisServer {
    /// Spawn WHOIS server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let listener = TcpListener::bind(listen_addr).await?;
        let local_addr = listener.local_addr()?;

        // INFO: Log lifecycle event
        console_info!(status_tx, "[INFO] WHOIS server (action-based) listening on {}");

        let protocol = Arc::new(actions::WhoisProtocol::new());

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, peer_addr)) => {
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);

                        // Add connection to ServerInstance
                        use crate::state::server::{
                            ConnectionState as ServerConnectionState, ConnectionStatus,
                            ProtocolConnectionInfo,
                        };
                        let now = std::time::Instant::now();
                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr: peer_addr,
                            local_addr,
                            bytes_sent: 0,
                            bytes_received: 0,
                            packets_sent: 0,
                            packets_received: 0,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::empty(),
                        };
                        app_state
                            .add_connection_to_server(server_id, conn_state)
                            .await;
                        console_info!(status_tx, "__UPDATE_UI__");

                        // DEBUG: Log connection summary
                        debug!("WHOIS client connected from {}", peer_addr);
                        let _ = status_tx
                            .send(format!("[DEBUG] WHOIS client connected from {}", peer_addr));

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();
                        let connection_id_clone = connection_id;

                        tokio::spawn(async move {
                            handle_whois_connection(
                                socket,
                                peer_addr,
                                llm_clone,
                                state_clone,
                                status_clone,
                                server_id,
                                protocol_clone,
                                connection_id_clone,
                            )
                            .await
                        });
                    }
                    Err(e) => {
                        // ERROR: Critical failure
                        console_error!(status_tx, "[ERROR] WHOIS accept error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

async fn handle_whois_connection(
    mut socket: tokio::net::TcpStream,
    peer_addr: SocketAddr,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    server_id: crate::state::ServerId,
    protocol: Arc<actions::WhoisProtocol>,
    connection_id: ConnectionId,
) {
    let (mut reader, mut writer) = tokio::io::split(&mut socket);
    let mut buffer = vec![0u8; 4096];

    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => {
                // DEBUG: Connection closed
                debug!("WHOIS client {} disconnected", peer_addr);
                let _ = status_tx
                    .send(format!("[DEBUG] WHOIS client {} disconnected", peer_addr));

                // Update connection status
                use crate::state::server::ConnectionStatus;
                app_state
                    .update_connection_status(server_id, connection_id, ConnectionStatus::Closed)
                    .await;
                console_info!(status_tx, "__UPDATE_UI__");
                break;
            }
            Ok(n) => {
                let query_data = buffer[..n].to_vec();
                let query_str = String::from_utf8_lossy(&query_data).to_string();

                // Update connection stats
                app_state
                    .update_connection_stats(
                        server_id,
                        connection_id,
                        Some(n as u64),
                        None,
                        Some(1),
                        None,
                    )
                    .await;

                // DEBUG: Log summary
                console_debug!(status_tx, "[DEBUG] WHOIS received {} bytes from {}");

                // TRACE: Log full payload
                console_trace!(status_tx, "[TRACE] WHOIS query data: {}", query_str.trim());

                // Parse query (trim whitespace and newlines)
                let query = query_str.trim().to_string();

                // Create event
                let event = Event::new(
                    &WHOIS_QUERY_EVENT,
                    serde_json::json!({
                        "query": query,
                    }),
                );

                // DEBUG: Log LLM call
                console_debug!(status_tx, "[DEBUG] WHOIS calling LLM for query from {}");

                // Call LLM
                match call_llm(
                    &llm_client,
                    &app_state,
                    server_id,
                    Some(connection_id),
                    &event,
                    protocol.as_ref(),
                )
                .await
                {
                    Ok(execution_result) => {
                        // Display messages from LLM
                        for message in &execution_result.messages {
                            console_info!(status_tx, "[INFO] {}", message);
                        }

                        // DEBUG: Log protocol results count
                        console_debug!(status_tx, "[DEBUG] WHOIS got {} protocol results");

                        // Send all outputs to client and check for close
                        let mut should_close = false;
                        for protocol_result in execution_result.protocol_results {
                            match protocol_result {
                                crate::llm::actions::protocol_trait::ActionResult::Output(output_data) => {
                                    if let Err(e) = writer.write_all(&output_data).await {
                                        // ERROR: Write failed
                                        error!("WHOIS write error: {}", e);
                                        let _ =
                                            status_tx.send(format!("[ERROR] WHOIS write error: {}", e));
                                        return;
                                    }

                                    // Update connection stats
                                    app_state
                                        .update_connection_stats(
                                            server_id,
                                            connection_id,
                                            None,
                                            Some(output_data.len() as u64),
                                            None,
                                            Some(1),
                                        )
                                        .await;

                                    // DEBUG: Log summary
                                    console_debug!(status_tx, "[DEBUG] WHOIS sent {} bytes to {}");

                                    // TRACE: Log full payload
                                    console_trace!(status_tx, "[TRACE] WHOIS response: {}");

                                    // INFO: User-facing message
                                    console_info!(status_tx, "→ WHOIS response to {} ({} bytes)");
                                }
                                crate::llm::actions::protocol_trait::ActionResult::CloseConnection => {
                                    should_close = true;
                                    debug!("WHOIS closing connection per LLM request");
                                    let _ = status_tx
                                        .send("[DEBUG] WHOIS closing connection per LLM request".to_string());
                                }
                                _ => {} // Ignore other action results
                            }
                        }

                        // Break loop if LLM requested connection close
                        if should_close {
                            break;
                        }
                    }
                    Err(e) => {
                        // ERROR: LLM call failed
                        console_error!(status_tx, "✗ WHOIS LLM error: {}", e);
                        break;
                    }
                }
            }
            Err(e) => {
                // ERROR: Read failed
                error!("WHOIS read error from {}: {}", peer_addr, e);
                let _ =
                    status_tx.send(format!("[ERROR] WHOIS read error from {}: {}", peer_addr, e));
                break;
            }
        }
    }

    // Update connection status to closed
    use crate::state::server::ConnectionStatus;
    app_state
        .update_connection_status(server_id, connection_id, ConnectionStatus::Closed)
        .await;
    console_info!(status_tx, "__UPDATE_UI__");
}
