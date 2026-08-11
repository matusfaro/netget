//! FTP server implementation
//!
//! File Transfer Protocol (RFC 959) server with LLM-controlled responses.
//! Supports basic FTP commands: USER, PASS, SYST, PWD, CWD, LIST, RETR, STOR, QUIT, etc.

pub mod actions;

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

#[cfg(feature = "ftp")]
use crate::console_debug;
#[cfg(feature = "ftp")]
use crate::llm::action_helper::call_llm;
#[cfg(feature = "ftp")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "ftp")]
use crate::llm::ActionResult;
#[cfg(feature = "ftp")]
use crate::protocol::Event;
#[cfg(feature = "ftp")]
use crate::server::ftp::actions::FtpProtocol;
#[cfg(feature = "ftp")]
use crate::state::app_state::AppState;
#[cfg(feature = "ftp")]
use actions::FTP_COMMAND_EVENT;

/// FTP server that provides LLM-controlled file transfer operations
pub struct FtpServer;

#[cfg(feature = "ftp")]
impl FtpServer {
    /// Spawn FTP server with integrated LLM actions
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

        info!("FTP server (action-based) listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] FTP server listening on {}", local_addr));

        let protocol = Arc::new(FtpProtocol::new());

        let task_registrar = app_state.clone();
        let accept_handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = crate::server::connection::ConnectionId::new(
                            app_state.get_next_unified_id().await,
                        );
                        console_debug!(
                            status_tx,
                            "FTP connection {} from {}",
                            connection_id,
                            remote_addr
                        );

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);

                        tokio::spawn(async move {
                            // Register the connection so it shows up in the TUI and in
                            // list_connections, and so stop_server accounts for it.
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

                            if let Err(e) = FtpSession::handle_session(
                                stream,
                                connection_id,
                                server_id,
                                llm_clone,
                                state_clone.clone(),
                                status_clone.clone(),
                                protocol_clone,
                            )
                            .await
                            {
                                error!("FTP session error: {}", e);
                                let _ =
                                    status_clone.send(format!("[ERROR] FTP session error: {}", e));
                            }

                            state_clone
                                .close_connection_on_server(server_id, connection_id)
                                .await;
                            let _ = status_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept FTP connection: {}", e);
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

#[cfg(feature = "ftp")]
struct FtpSession;

#[cfg(feature = "ftp")]
impl FtpSession {
    /// Handle an FTP session
    async fn handle_session(
        mut stream: tokio::net::TcpStream,
        connection_id: crate::server::connection::ConnectionId,
        server_id: crate::state::ServerId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<FtpProtocol>,
    ) -> Result<()> {
        // Send initial greeting
        Self::send_greeting(
            &mut stream,
            connection_id,
            server_id,
            &llm_client,
            &app_state,
            &status_tx,
            &protocol,
        )
        .await?;

        // Handle session commands
        Self::handle_session_commands(
            stream,
            connection_id,
            server_id,
            llm_client,
            app_state,
            status_tx,
            protocol,
        )
        .await
    }

    /// Send FTP greeting (220 response)
    async fn send_greeting<S>(
        stream: &mut S,
        connection_id: crate::server::connection::ConnectionId,
        server_id: crate::state::ServerId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: &Arc<FtpProtocol>,
    ) -> Result<()>
    where
        S: tokio::io::AsyncWrite + Unpin,
    {
        use tokio::io::AsyncWriteExt;

        // The client will not speak until it has seen a 2xx greeting, so this event is the
        // handler's only chance to produce one. `CONNECTION_ESTABLISHED` is a sentinel, not a
        // command the client sent - see FTP_COMMAND_EVENT.
        let greeting_event = Event::new(
            &FTP_COMMAND_EVENT,
            serde_json::json!({
                "command": "CONNECTION_ESTABLISHED"
            }),
        );

        match call_llm(
            llm_client,
            app_state,
            server_id,
            Some(connection_id),
            &greeting_event,
            protocol.as_ref(),
        )
        .await
        {
            Ok(execution_result) => {
                for protocol_result in execution_result.protocol_results {
                    if let ActionResult::Output(data) = protocol_result {
                        stream.write_all(&data).await?;
                        stream.flush().await?;
                    }
                }
            }
            Err(e) => {
                // Logging it was an improvement on silence for *us*; the client still sat
                // waiting for a greeting that never came, because an FTP client may not send
                // a command until it has read one. RFC 959 defines 421 as the greeting a
                // server sends when it is declining the session, and it closes afterwards -
                // which is the same shape SMTP uses, for the same reason.
                error!("FTP greeting handler failed on connection {connection_id}: {e}");
                let reason = crate::utils::truncate_for_log(&e.to_string(), 200)
                    .replace(['\r', '\n'], " ");
                let _ = status_tx.send(format!(
                    "[ERROR] FTP connection {connection_id} refused with 421: {reason}"
                ));
                let reply = format!(
                    "421 Service not available, closing control connection (netget: {reason})\r\n"
                );
                let _ = stream.write_all(reply.as_bytes()).await;
                let _ = stream.flush().await;
                // 421 means the control connection is closing, so the session must not
                // continue into the command loop. The caller propagates this with `?`, which
                // ends the connection task and drops the socket.
                return Err(anyhow::anyhow!(
                    "FTP greeting refused with 421: {reason}"
                ));
            }
        }

        Ok(())
    }

    /// Handle FTP session commands
    async fn handle_session_commands(
        mut stream: tokio::net::TcpStream,
        connection_id: crate::server::connection::ConnectionId,
        server_id: crate::state::ServerId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<FtpProtocol>,
    ) -> Result<()> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let (read_half, mut write_half) = tokio::io::split(&mut stream);
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        loop {
            line.clear();
            let n = reader.read_line(&mut line).await?;
            if n == 0 {
                break;
            }

            let command = line.trim();
            debug!("FTP received: {}", command);
            console_debug!(status_tx, "FTP received: {}", command);

            // Create FTP command event
            let event = Event::new(
                &FTP_COMMAND_EVENT,
                serde_json::json!({
                    "command": command
                }),
            );

            // Get handler/LLM response
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
                    for protocol_result in execution_result.protocol_results {
                        match protocol_result {
                            ActionResult::Output(data) => {
                                write_half.write_all(&data).await?;
                                write_half.flush().await?;

                                let response = String::from_utf8_lossy(&data);
                                debug!("FTP sent: {}", response.trim());
                                console_debug!(status_tx, "FTP sent: {}", response.trim());
                            }
                            ActionResult::CloseConnection => {
                                return Ok(());
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    // Do not leave the client hanging with no diagnostic: RFC 959 421 tells it
                    // the service is unavailable and the control connection is closing.
                    error!("FTP handler failed for command {command:?}: {e}");
                    let _ = status_tx.send(format!("[ERROR] FTP handler failed: {e}"));
                    let _ = write_half
                        .write_all(b"421 Service not available, closing control connection\r\n")
                        .await;
                    let _ = write_half.flush().await;
                    return Ok(());
                }
            }
        }

        Ok(())
    }
}

#[cfg(not(feature = "ftp"))]
impl FtpServer {
    pub async fn spawn_with_llm_actions(
        _listen_addr: SocketAddr,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        _status_tx: mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        anyhow::bail!("FTP feature not enabled")
    }
}

// Stub types needed for non-feature compilation
#[cfg(not(feature = "ftp"))]
use crate::llm::ollama_client::OllamaClient;
#[cfg(not(feature = "ftp"))]
use crate::state::app_state::AppState;
