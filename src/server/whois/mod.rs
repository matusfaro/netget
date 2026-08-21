//! WHOIS server implementation
pub mod actions;

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::logging::emit::Log;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use actions::WHOIS_QUERY_EVENT;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};

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

        Log::new(Some(&status_tx)).info(format!("WHOIS server listening on {}", local_addr));

        let protocol = Arc::new(actions::WhoisProtocol::new());

        let task_registrar = app_state.clone();
        let accept_handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, peer_addr)) => {
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);

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
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        Log::new(Some(&status_tx))
                            .info(format!("WHOIS client connected from {}", peer_addr));

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
                        Log::new(Some(&status_tx)).error(format!("WHOIS accept error: {}", e));
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

/// Write one reply to the peer and count it. The guard is dropped before the
/// stats update so nothing awaits while holding the write half.
async fn write_counted<W>(
    write_half: &Arc<Mutex<W>>,
    data: &[u8],
    app_state: &AppState,
    server_id: crate::state::ServerId,
    connection_id: ConnectionId,
) -> std::io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    {
        let mut writer = write_half.lock().await;
        writer.write_all(data).await?;
        writer.flush().await?;
    }
    app_state
        .update_connection_stats(
            server_id,
            connection_id,
            None,
            Some(data.len() as u64),
            None,
            Some(1),
        )
        .await;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_whois_connection(
    socket: tokio::net::TcpStream,
    peer_addr: SocketAddr,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    server_id: crate::state::ServerId,
    protocol: Arc<actions::WhoisProtocol>,
    connection_id: ConnectionId,
) {
    let (reader, write_half) = tokio::io::split(socket);
    let write_half = Arc::new(Mutex::new(write_half));

    // Peer messaging: the dashboard's "message this peer" / "disconnect this peer" inject
    // actions into THIS connection through the same executor the LLM path uses. Registered
    // before the first read, because a WHOIS server says nothing until the client speaks
    // and a manual `*` rule can then park the query for minutes - the operator must be
    // able to reach the connection while it waits.
    let peer_rx = crate::server::peer_support::register_peer_channel(
        &app_state,
        server_id,
        connection_id.as_u32(),
    )
    .await;
    crate::server::peer_support::spawn_peer_command_task(
        peer_rx,
        protocol.clone(),
        app_state.clone(),
        server_id,
        connection_id.as_u32(),
        write_half.clone(),
        status_tx.clone(),
    );

    run_whois_session(
        reader,
        &write_half,
        peer_addr,
        &llm_client,
        &app_state,
        &status_tx,
        server_id,
        &protocol,
        connection_id,
    )
    .await;

    // Every exit path - EOF, read error, write error, close_connection, LLM failure - lands
    // here. Dropping the handle also ends the peer command task, which releases its clone of
    // the write half; the explicit shutdown makes the FIN immediate rather than waiting on it.
    app_state
        .remove_peer_handle(server_id, connection_id.as_u32())
        .await;
    let _ = write_half.lock().await.shutdown().await;

    use crate::state::server::ConnectionStatus;
    app_state
        .update_connection_status(server_id, connection_id, ConnectionStatus::Closed)
        .await;
    let _ = status_tx.send("__UPDATE_UI__".to_string());
}

#[allow(clippy::too_many_arguments)]
async fn run_whois_session<R, W>(
    mut reader: R,
    write_half: &Arc<Mutex<W>>,
    peer_addr: SocketAddr,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    server_id: crate::state::ServerId,
    protocol: &Arc<actions::WhoisProtocol>,
    connection_id: ConnectionId,
) where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut buffer = vec![0u8; 4096];
    let log = Log::new(Some(status_tx));

    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => {
                log.info(format!("WHOIS client {} disconnected", peer_addr));
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

                // Summary + payload are FileOnly: the whois_query event template
                // surfaces the query to the TUI.
                log.debug(format!("WHOIS received {} bytes from {}", n, peer_addr));
                log.trace(format!("WHOIS query data: {}", query_str.trim()));

                // Parse query (trim whitespace and newlines)
                let query = query_str.trim().to_string();

                // Create event
                let event = Event::new(
                    &WHOIS_QUERY_EVENT,
                    serde_json::json!({
                        "query": query,
                    }),
                );

                log.debug(format!("WHOIS calling LLM for query from {}", peer_addr));

                // Call LLM
                match call_llm(
                    llm_client,
                    app_state,
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
                            log.info(message);
                        }

                        log.debug(format!(
                            "WHOIS got {} protocol results",
                            execution_result.protocol_results.len()
                        ));

                        // Send all outputs to client and check for close.
                        //
                        // WHOIS is one query, one response, then close (RFC 3912),
                        // so a connection that closes without writing anything is
                        // indistinguishable to the client from a server that is
                        // broken. Track whether anything reached the wire and
                        // answer below if nothing did.
                        let mut should_close = false;
                        let mut wrote_output = false;
                        for protocol_result in execution_result.protocol_results {
                            match protocol_result {
                                crate::llm::actions::protocol_trait::ActionResult::Output(output_data) => {
                                    if let Err(e) = write_counted(
                                        write_half,
                                        &output_data,
                                        app_state,
                                        server_id,
                                        connection_id,
                                    )
                                    .await
                                    {
                                        log.error(format!("WHOIS write error: {}", e));
                                        return;
                                    }
                                    wrote_output = true;

                                    // Summary + payload FileOnly; access line below on TUI.
                                    log.debug(format!(
                                        "WHOIS sent {} bytes to {}",
                                        output_data.len(),
                                        peer_addr
                                    ));
                                    log.trace(format!(
                                        "WHOIS response: {}",
                                        String::from_utf8_lossy(&output_data)
                                    ));
                                    log.info(format!(
                                        "WHOIS response to {} ({} bytes)",
                                        peer_addr,
                                        output_data.len()
                                    ));
                                }
                                crate::llm::actions::protocol_trait::ActionResult::CloseConnection => {
                                    should_close = true;
                                    log.debug("WHOIS closing connection per LLM request");
                                }
                                _ => {} // Ignore other action results
                            }
                        }

                        // Nothing reached the wire: the model answered with only a
                        // close, or with actions that all failed. Say so in a WHOIS
                        // comment line rather than hanging up silently — '%' is the
                        // conventional comment marker, so a client reads it as a
                        // remark and never as a record.
                        if !wrote_output {
                            log.warn(format!(
                                "WHOIS produced no response for {} ({} failed action(s)); \
                                 answering with a comment instead of closing silently",
                                peer_addr,
                                execution_result.failures.len()
                            ));
                            let notice = b"% netget: no data was produced for this query\r\n";
                            if write_counted(
                                write_half,
                                notice,
                                app_state,
                                server_id,
                                connection_id,
                            )
                            .await
                            .is_err()
                            {
                                return;
                            }
                        }

                        // Break loop if LLM requested connection close
                        if should_close {
                            break;
                        }
                    }
                    Err(e) => {
                        // Same reasoning as above: the client is waiting for a
                        // response it will otherwise never get.
                        log.warn(format!("WHOIS LLM call failed: {}", e));
                        let notice = b"% netget: the query could not be answered\r\n";
                        let _ =
                            write_counted(write_half, notice, app_state, server_id, connection_id)
                                .await;
                        break;
                    }
                }
            }
            Err(e) => {
                log.error(format!("WHOIS read error from {}: {}", peer_addr, e));
                break;
            }
        }
    }
}
