//! SMTP server implementation
pub mod actions;

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

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
                    // Answer 451 rather than writing nothing. SMTP has a whole 4xx class
                    // meaning "temporary, retry later" (RFC 5321 §4.2.1), which is exactly
                    // what an unavailable backend is, and a client that gets one requeues the
                    // message instead of blocking until its own timeout and then bouncing it.
                    //
                    // It also fails closed: a 4xx is never mistaken for acceptance, so an
                    // outage cannot silently look like a delivered message.
                    error!(
                        "SMTP connection {} got no response for {:?}: {:#}",
                        connection_id, command, e
                    );
                    let _ = status_tx.send(format!("[ERROR] SMTP LLM error: {:#}", e));

                    // 4.3.2 is "system not accepting network messages" (RFC 3463), which is
                    // the truthful enhanced code for capacity exhaustion; 4.3.0 covers the
                    // rest.
                    let reply: &[u8] = if crate::llm::is_overload_error(&e) {
                        warn!(
                            "SMTP 451 on connection {}: LLM capacity exhausted",
                            connection_id
                        );
                        b"451 4.3.2 Backend at capacity, try again later\r\n"
                    } else {
                        b"451 4.3.0 Temporary local error, try again later\r\n"
                    };
                    write_half.write_all(reply).await?;
                    write_half.flush().await?;
                    console_debug!(
                        status_tx,
                        "SMTP sent: {}",
                        String::from_utf8_lossy(reply).trim()
                    );
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
                // No banner means no session. RFC 5321 §3.1 gives a server exactly this way
                // to decline one: a 421 greeting, after which the connection closes. Writing
                // nothing (the previous behaviour) left the peer waiting for a 220 until its
                // own timeout, with no way to tell an overloaded server from a black hole.
                error!(
                    "SMTP greeting for connection {} failed: {:#}",
                    connection_id, e
                );
                let _ = _status_tx.send(format!("[ERROR] SMTP greeting failed: {:#}", e));

                let reply: &[u8] = if crate::llm::is_overload_error(&e) {
                    warn!(
                        "SMTP 421 on connection {}: LLM capacity exhausted",
                        connection_id
                    );
                    b"421 4.3.2 Service not available, backend at capacity\r\n"
                } else {
                    b"421 4.3.0 Service not available, closing transmission channel\r\n"
                };
                stream.write_all(reply).await?;
                stream.flush().await?;
                let _ = _status_tx.send(format!(
                    "→ SMTP {} to connection {}",
                    String::from_utf8_lossy(reply).trim(),
                    connection_id
                ));

                // Propagate so handle_session stops before the command loop: after a 421 the
                // only thing the server may do is close.
                anyhow::bail!("SMTP greeting unavailable: {e:#}");
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
