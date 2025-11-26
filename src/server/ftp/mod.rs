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

        tokio::spawn(async move {
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

                        tokio::spawn(async move {
                            if let Err(e) = FtpSession::handle_session(
                                stream,
                                connection_id,
                                server_id,
                                llm_clone,
                                state_clone,
                                status_clone.clone(),
                                protocol_clone,
                            )
                            .await
                            {
                                error!("FTP session error: {}", e);
                                let _ =
                                    status_clone.send(format!("[ERROR] FTP session error: {}", e));
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept FTP connection: {}", e);
                        break;
                    }
                }
            }
        });

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
        _status_tx: &mpsc::UnboundedSender<String>,
        protocol: &Arc<FtpProtocol>,
    ) -> Result<()>
    where
        S: tokio::io::AsyncWrite + Unpin,
    {
        use tokio::io::AsyncWriteExt;

        let greeting_event = Event::new(
            &FTP_COMMAND_EVENT,
            serde_json::json!({
                "command": "CONNECTION_ESTABLISHED"
            }),
        );

        if let Ok(execution_result) = call_llm(
            llm_client,
            app_state,
            server_id,
            Some(connection_id),
            &greeting_event,
            protocol.as_ref(),
        )
        .await
        {
            for protocol_result in execution_result.protocol_results {
                if let ActionResult::Output(data) = protocol_result {
                    stream.write_all(&data).await?;
                    stream.flush().await?;
                }
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

            // Get LLM response
            if let Ok(execution_result) = call_llm(
                &llm_client,
                &app_state,
                server_id,
                Some(connection_id),
                &event,
                protocol.as_ref(),
            )
            .await
            {
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
