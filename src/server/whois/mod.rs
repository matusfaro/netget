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
        info!("WHOIS server (action-based) listening on {}", local_addr);
        let _ = status_tx.send(format!(
            "[INFO] WHOIS server (action-based) listening on {}",
            local_addr
        ));

        let protocol = Arc::new(actions::WhoisProtocol::new());

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, peer_addr)) => {
                        let connection_id = ConnectionId::new();

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
                            protocol_info: ProtocolConnectionInfo::Whois {
                                recent_queries: vec![],
                            },
                        };
                        app_state
                            .add_connection_to_server(server_id, conn_state)
                            .await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

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
                        error!("WHOIS accept error: {}", e);
                        let _ = status_tx.send(format!("[ERROR] WHOIS accept error: {}", e));
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
                let _ = status_tx.send("__UPDATE_UI__".to_string());
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
                debug!("WHOIS received {} bytes from {}", n, peer_addr);
                let _ = status_tx.send(format!(
                    "[DEBUG] WHOIS received {} bytes from {}",
                    n, peer_addr
                ));

                // TRACE: Log full payload
                trace!("WHOIS query data: {}", query_str.trim());
                let _ = status_tx.send(format!("[TRACE] WHOIS query data: {}", query_str.trim()));

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
                debug!("WHOIS calling LLM for query from {}", peer_addr);
                let _ = status_tx.send(format!(
                    "[DEBUG] WHOIS calling LLM for query from {}",
                    peer_addr
                ));

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
                            info!("{}", message);
                            let _ = status_tx.send(format!("[INFO] {}", message));
                        }

                        // DEBUG: Log protocol results count
                        debug!(
                            "WHOIS got {} protocol results",
                            execution_result.protocol_results.len()
                        );
                        let _ = status_tx.send(format!(
                            "[DEBUG] WHOIS got {} protocol results",
                            execution_result.protocol_results.len()
                        ));

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
                                    debug!("WHOIS sent {} bytes to {}", output_data.len(), peer_addr);
                                    let _ = status_tx.send(format!(
                                        "[DEBUG] WHOIS sent {} bytes to {}",
                                        output_data.len(),
                                        peer_addr
                                    ));

                                    // TRACE: Log full payload
                                    trace!(
                                        "WHOIS response: {}",
                                        String::from_utf8_lossy(&output_data)
                                    );
                                    let _ = status_tx.send(format!(
                                        "[TRACE] WHOIS response: {}",
                                        String::from_utf8_lossy(&output_data)
                                    ));

                                    // INFO: User-facing message
                                    let _ = status_tx.send(format!(
                                        "→ WHOIS response to {} ({} bytes)",
                                        peer_addr,
                                        output_data.len()
                                    ));
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
                        error!("WHOIS LLM call failed: {}", e);
                        let _ = status_tx.send(format!("✗ WHOIS LLM error: {}", e));
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
    let _ = status_tx.send("__UPDATE_UI__".to_string());
}
