//! POP3 server implementation
pub mod actions;

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

#[cfg(feature = "pop3")]
use crate::console_debug;
#[cfg(feature = "pop3")]
use crate::llm::action_helper::call_llm;
#[cfg(feature = "pop3")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "pop3")]
use crate::llm::ActionResult;
#[cfg(feature = "pop3")]
use crate::protocol::Event;
#[cfg(feature = "pop3")]
use crate::server::Pop3Protocol;
#[cfg(feature = "pop3")]
use crate::state::app_state::AppState;
#[cfg(feature = "pop3")]
use actions::POP3_COMMAND_EVENT;
#[cfg(feature = "pop3")]
use tokio_rustls::TlsAcceptor;

/// POP3 server that forwards mail retrieval to LLM
pub struct Pop3Server;

#[cfg(feature = "pop3")]
impl Pop3Server {
    /// Spawn POP3 server with integrated LLM actions
    ///
    /// If tls_config is Some, the server will use implicit TLS (POP3S)
    /// If tls_config is None, the server will use plain text (POP3)
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        tls_config: Option<Arc<rustls::ServerConfig>>,
    ) -> Result<SocketAddr> {
        let listener =
            crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;

        if tls_config.is_some() {
            info!(
                "POP3S server (TLS, action-based) listening on {}",
                local_addr
            );
        } else {
            info!(
                "POP3 server (plain, action-based) listening on {}",
                local_addr
            );
        }

        let protocol = Arc::new(Pop3Protocol::new());
        let tls_acceptor = tls_config.map(TlsAcceptor::from);

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = crate::server::connection::ConnectionId::new(
                            app_state.get_next_unified_id().await,
                        );
                        console_debug!(
                            status_tx,
                            "POP3 connection {} from {}",
                            connection_id,
                            remote_addr
                        );

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();
                        let tls_acceptor_clone = tls_acceptor.clone();

                        tokio::spawn(async move {
                            // Optionally perform TLS handshake
                            if let Some(ref acceptor) = tls_acceptor_clone {
                                match acceptor.accept(stream).await {
                                    Ok(tls_stream) => {
                                        debug!(
                                            "TLS handshake completed for connection {}",
                                            connection_id
                                        );
                                        let _ = status_clone.send(format!(
                                            "[DEBUG] TLS handshake completed for connection {}",
                                            connection_id
                                        ));
                                        if let Err(e) = Pop3Session::handle_tls_session(
                                            tls_stream,
                                            connection_id,
                                            server_id,
                                            llm_clone,
                                            state_clone,
                                            status_clone,
                                            protocol_clone,
                                        )
                                        .await
                                        {
                                            error!(
                                                "POP3S session error for connection {}: {}",
                                                connection_id, e
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        error!(
                                            "TLS handshake failed for connection {}: {}",
                                            connection_id, e
                                        );
                                    }
                                }
                            } else {
                                // Plain text POP3
                                if let Err(e) = Pop3Session::handle_session(
                                    stream,
                                    connection_id,
                                    server_id,
                                    llm_clone,
                                    state_clone,
                                    status_clone,
                                    protocol_clone,
                                )
                                .await
                                {
                                    error!(
                                        "POP3 session error for connection {}: {}",
                                        connection_id, e
                                    );
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept POP3 connection: {}", e);
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

#[cfg(feature = "pop3")]
struct Pop3Session;

#[cfg(feature = "pop3")]
impl Pop3Session {
    async fn handle_tls_session(
        stream: tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
        connection_id: crate::server::connection::ConnectionId,
        server_id: crate::state::ServerId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<Pop3Protocol>,
    ) -> Result<()> {
        use tokio::io::{AsyncBufReadExt, BufReader};

        let (read_half, write_half) = tokio::io::split(stream);
        let mut reader = BufReader::new(read_half);
        let write_half = Arc::new(tokio::sync::Mutex::new(write_half));

        // Send initial greeting
        let greeting_event = Event::new(
            &POP3_COMMAND_EVENT,
            serde_json::json!({
                "command": "CONNECTION_ESTABLISHED",
                "connection_id": connection_id.to_string(),
            }),
        );

        if let Err(e) = Self::process_command(
            &greeting_event,
            &llm_client,
            &app_state,
            &status_tx,
            &protocol,
            server_id,
            connection_id,
            &write_half,
        )
        .await
        {
            error!("Failed to send POP3 greeting: {}", e);
            return Err(e);
        }

        // Main command loop
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => {
                    debug!("POP3 connection {} closed by client", connection_id);
                    break;
                }
                Ok(_) => {
                    let command = line.trim().to_string();
                    if command.is_empty() {
                        continue;
                    }

                    debug!("POP3 connection {} received: {}", connection_id, command);

                    let event = Event::new(
                        &POP3_COMMAND_EVENT,
                        serde_json::json!({
                            "command": command,
                            "connection_id": connection_id.to_string(),
                        }),
                    );

                    if let Err(e) = Self::process_command(
                        &event,
                        &llm_client,
                        &app_state,
                        &status_tx,
                        &protocol,
                        server_id,
                        connection_id,
                        &write_half,
                    )
                    .await
                    {
                        error!("Failed to process POP3 command: {}", e);
                        break;
                    }
                }
                Err(e) => {
                    error!("POP3 connection {} read error: {}", connection_id, e);
                    break;
                }
            }
        }

        Ok(())
    }

    async fn handle_session(
        stream: tokio::net::TcpStream,
        connection_id: crate::server::connection::ConnectionId,
        server_id: crate::state::ServerId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<Pop3Protocol>,
    ) -> Result<()> {
        use tokio::io::{AsyncBufReadExt, BufReader};

        let (read_half, write_half) = tokio::io::split(stream);
        let mut reader = BufReader::new(read_half);
        let write_half = Arc::new(tokio::sync::Mutex::new(write_half));

        // Send initial greeting
        let greeting_event = Event::new(
            &POP3_COMMAND_EVENT,
            serde_json::json!({
                "command": "CONNECTION_ESTABLISHED",
                "connection_id": connection_id.to_string(),
            }),
        );

        if let Err(e) = Self::process_command(
            &greeting_event,
            &llm_client,
            &app_state,
            &status_tx,
            &protocol,
            server_id,
            connection_id,
            &write_half,
        )
        .await
        {
            error!("Failed to send POP3 greeting: {}", e);
            return Err(e);
        }

        // Main command loop
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => {
                    debug!("POP3 connection {} closed by client", connection_id);
                    break;
                }
                Ok(_) => {
                    let command = line.trim().to_string();
                    if command.is_empty() {
                        continue;
                    }

                    debug!("POP3 connection {} received: {}", connection_id, command);

                    let event = Event::new(
                        &POP3_COMMAND_EVENT,
                        serde_json::json!({
                            "command": command,
                            "connection_id": connection_id.to_string(),
                        }),
                    );

                    if let Err(e) = Self::process_command(
                        &event,
                        &llm_client,
                        &app_state,
                        &status_tx,
                        &protocol,
                        server_id,
                        connection_id,
                        &write_half,
                    )
                    .await
                    {
                        error!("Failed to process POP3 command: {}", e);
                        break;
                    }
                }
                Err(e) => {
                    error!("POP3 connection {} read error: {}", connection_id, e);
                    break;
                }
            }
        }

        Ok(())
    }

    async fn process_command<W>(
        event: &Event,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        _status_tx: &mpsc::UnboundedSender<String>,
        protocol: &Arc<Pop3Protocol>,
        server_id: crate::state::ServerId,
        connection_id: crate::server::connection::ConnectionId,
        write_half: &Arc<tokio::sync::Mutex<W>>,
    ) -> Result<()>
    where
        W: tokio::io::AsyncWrite + Unpin,
    {
        use tokio::io::AsyncWriteExt;

        // Call LLM for action
        let llm_result = call_llm(
            llm_client,
            app_state,
            server_id,
            Some(connection_id),
            event,
            protocol.as_ref(),
        )
        .await?;

        // Execute actions
        for action in llm_result.protocol_results {
            match action {
                ActionResult::Output(data) => {
                    let mut writer = write_half.lock().await;
                    writer.write_all(&data).await?;
                    writer.flush().await?;
                    debug!(
                        "POP3 connection {} sent {} bytes",
                        connection_id,
                        data.len()
                    );
                }
                ActionResult::CloseConnection => {
                    debug!("POP3 connection {} closing", connection_id);
                    return Ok(());
                }
                ActionResult::WaitForMore => {
                    // Do nothing, wait for next command
                }
                _ => {
                    // Unknown action, ignore
                }
            }
        }

        Ok(())
    }
}
