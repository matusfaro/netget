//! SMTP server implementation
pub mod actions;

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::console_debug;
#[cfg(feature = "smtp")]
use crate::llm::action_helper::call_llm;
#[cfg(feature = "smtp")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "smtp")]
use crate::llm::ActionResult;
#[cfg(feature = "smtp")]
use crate::protocol::Event;
#[cfg(feature = "smtp")]
use crate::server::SmtpProtocol;
#[cfg(feature = "smtp")]
use crate::state::app_state::AppState;
#[cfg(feature = "smtp")]
use actions::SMTP_COMMAND_EVENT;
#[cfg(feature = "smtp")]
use tokio_rustls::TlsAcceptor;

/// SMTP server that forwards mail to LLM
pub struct SmtpServer;

#[cfg(feature = "smtp")]
impl SmtpServer {
    /// Spawn SMTP server with integrated LLM actions
    ///
    /// If tls_config is Some, the server will use implicit TLS (SMTPS)
    /// If tls_config is None, the server will use plain text (SMTP)
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
                "SMTPS server (TLS, action-based) listening on {}",
                local_addr
            );
            let _ = status_tx.send(format!("[INFO] SMTPS server listening on {}", local_addr));
        } else {
            info!(
                "SMTP server (plain, action-based) listening on {}",
                local_addr
            );
            let _ = status_tx.send(format!("[INFO] SMTP server listening on {}", local_addr));
        }

        let protocol = Arc::new(SmtpProtocol::new());
        let tls_acceptor = tls_config.map(TlsAcceptor::from);

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
                            "SMTP connection {} from {}",
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
                                        if let Err(e) = SmtpSession::handle_session(
                                            tls_stream,
                                            connection_id,
                                            server_id,
                                            llm_clone,
                                            state_clone,
                                            status_clone.clone(),
                                            protocol_clone,
                                        )
                                        .await
                                        {
                                            error!("SMTP session error: {}", e);
                                            let _ = status_clone
                                                .send(format!("[ERROR] SMTP session error: {}", e));
                                        }
                                    }
                                    Err(e) => {
                                        error!(
                                            "TLS handshake failed for connection {}: {}",
                                            connection_id, e
                                        );
                                        let _ = status_clone
                                            .send(format!("[ERROR] TLS handshake failed: {}", e));
                                    }
                                }
                            } else {
                                if let Err(e) = SmtpSession::handle_session(
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
                                    error!("SMTP session error: {}", e);
                                    let _ = status_clone
                                        .send(format!("[ERROR] SMTP session error: {}", e));
                                }
                            };
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept SMTP connection: {}", e);
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

#[cfg(feature = "smtp")]
struct SmtpSession;

#[cfg(feature = "smtp")]
impl SmtpSession {
    /// Handle one SMTP session, plain or SMTPS.
    ///
    /// Generic over the transport: the plain and TLS paths were previously two verbatim
    /// copies of the same greeting-then-command-loop code.
    async fn handle_session<S>(
        stream: S,
        connection_id: crate::server::connection::ConnectionId,
        server_id: crate::state::ServerId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<SmtpProtocol>,
    ) -> Result<()>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
    {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let (read_half, mut write_half) = tokio::io::split(stream);
        let mut reader = BufReader::new(read_half);

        // Send initial greeting
        Self::send_greeting(
            &mut write_half,
            connection_id,
            server_id,
            &llm_client,
            &app_state,
            &status_tx,
            &protocol,
        )
        .await?;

        let mut line = String::new();

        loop {
            line.clear();
            let n = reader.read_line(&mut line).await?;
            if n == 0 {
                break;
            }

            let command = line.trim();
            console_debug!(status_tx, "SMTP received: {}", command);

            let event = Event::new(
                &SMTP_COMMAND_EVENT,
                serde_json::json!({
                    "command": command
                }),
            );

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
                    let mut should_close = false;

                    for protocol_result in execution_result.protocol_results {
                        match protocol_result {
                            ActionResult::Output(data) => {
                                write_half.write_all(&data).await?;
                                write_half.flush().await?;

                                let response = String::from_utf8_lossy(&data);
                                console_debug!(status_tx, "SMTP sent: {}", response.trim());
                            }
                            // Do not return here: the QUIT reply is normally a 221 followed by
                            // close_connection in the same batch, and returning early would
                            // drop the 221 when the ordering came back reversed.
                            ActionResult::CloseConnection => should_close = true,
                            _ => {}
                        }
                    }

                    if should_close {
                        return Ok(());
                    }
                }
                Err(e) => {
                    // Without this the peer just waits: nothing is written and the loop
                    // silently goes back to reading.
                    error!(
                        "SMTP connection {} got no response for {:?}: {:#}",
                        connection_id, command, e
                    );
                    let _ = status_tx.send(format!("[ERROR] SMTP LLM error: {:#}", e));
                }
            }
        }

        Ok(())
    }

    /// Send greeting for plain connection
    async fn send_greeting<S>(
        stream: &mut S,
        connection_id: crate::server::connection::ConnectionId,
        server_id: crate::state::ServerId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        _status_tx: &mpsc::UnboundedSender<String>,
        protocol: &Arc<SmtpProtocol>,
    ) -> Result<()>
    where
        S: tokio::io::AsyncWrite + Unpin,
    {
        use tokio::io::AsyncWriteExt;

        let greeting_event = Event::new(
            &SMTP_COMMAND_EVENT,
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
                // Previously swallowed by `if let Ok(..)`. The peer gets no 220 banner and sits
                // there until its own timeout, so at least say why in the log.
                error!(
                    "SMTP greeting for connection {} failed, no banner sent: {:#}",
                    connection_id, e
                );
                let _ = _status_tx.send(format!("[ERROR] SMTP greeting failed: {:#}", e));
            }
        }

        Ok(())
    }
}

#[cfg(not(feature = "smtp"))]
impl SmtpServer {
    pub async fn spawn_with_llm_actions(
        _listen_addr: SocketAddr,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        _status_tx: mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
        _tls_config: Option<Arc<rustls::ServerConfig>>,
    ) -> Result<SocketAddr> {
        anyhow::bail!("SMTP feature not enabled")
    }
}
