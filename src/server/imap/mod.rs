//! IMAP server implementation
//!
//! This module implements an IMAP (Internet Message Access Protocol) server
//! that allows LLM control over email retrieval and mailbox management.
//!
//! Key points:
//! - IMAP4rev1 commands parsed by hand into (tag, command, args). There is no `imap-codec`
//!   and no grammar: anything the split does not model (literals, continuations, quoting) is
//!   passed to the model as raw text.
//! - Session state management (NotAuthenticated -> Authenticated -> Selected -> Logout)
//! - Plain TCP only. An `ImapServer::spawn_with_tls` used to sit here advertising IMAPS on
//!   993; nothing called it and it could not have worked - it fed a concatenated PEM string
//!   to `Identity::from_pkcs12`, which only accepts DER PKCS#12 - so it was removed rather
//!   than left as an implemented-looking feature.
//! - No mailbox storage: the model answers LIST/FETCH/SEARCH from its instruction and memory.

pub mod actions;

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

#[cfg(feature = "imap")]
use crate::llm::action_helper::call_llm;
#[cfg(feature = "imap")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "imap")]
use crate::llm::ActionResult;
#[cfg(feature = "imap")]
use crate::protocol::Event;
#[cfg(feature = "imap")]
use crate::server::connection::ConnectionId;
#[cfg(feature = "imap")]
use crate::server::ImapProtocol;
#[cfg(feature = "imap")]
use crate::state::app_state::AppState;
#[cfg(feature = "imap")]
use crate::state::server::{
    ConnectionStatus, ImapSessionState, ProtocolConnectionInfo, ProtocolState, ServerId,
};
#[cfg(feature = "imap")]
use actions::{IMAP_AUTH_EVENT, IMAP_COMMAND_EVENT, IMAP_CONNECTION_EVENT};
#[cfg(feature = "imap")]
use serde_json::json;

/// IMAP server that handles mail retrieval with LLM
pub struct ImapServer;

#[cfg(feature = "imap")]
impl ImapServer {
    /// Spawn IMAP server with integrated LLM actions (plain TCP on port 143)
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: ServerId,
    ) -> Result<SocketAddr> {
        let listener =
            crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("IMAP server (action-based) listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] IMAP server listening on {}", local_addr));

        let protocol = Arc::new(ImapProtocol::new());

        let task_registrar = app_state.clone();
        let accept_handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);
                        debug!("IMAP connection {} from {}", connection_id, remote_addr);
                        let _ = status_tx.send(format!(
                            "→ IMAP connection {} from {}",
                            connection_id, remote_addr
                        ));

                        // Track connection in server state
                        let local_addr = stream.local_addr().unwrap_or(listen_addr);
                        let (read_half, write_half) = tokio::io::split(stream);
                        let write_half_arc = Arc::new(tokio::sync::Mutex::new(write_half));

                        // Add connection to app_state
                        app_state
                            .add_connection_to_server(
                                server_id,
                                crate::state::ConnectionState {
                                    id: connection_id,
                                    remote_addr,
                                    local_addr,
                                    bytes_sent: 0,
                                    bytes_received: 0,
                                    packets_sent: 0,
                                    packets_received: 0,
                                    last_activity: std::time::Instant::now(),
                                    status: ConnectionStatus::Active,
                                    status_changed_at: std::time::Instant::now(),
                                    protocol_info: ProtocolConnectionInfo::empty(),
                                },
                            )
                            .await;

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();
                        let write_half_for_session = write_half_arc.clone();

                        tokio::spawn(async move {
                            let mut session = ImapSession {
                                reader: BufReader::new(read_half),
                                writer: write_half_for_session,
                                connection_id,
                                server_id,
                                remote_addr,
                                llm_client: llm_clone,
                                app_state: state_clone.clone(),
                                status_tx: status_clone.clone(),
                                protocol: protocol_clone,
                            };

                            // Handle IMAP session
                            if let Err(e) = session.handle().await {
                                error!("IMAP session error for {}: {}", connection_id, e);
                                let _ = status_clone.send(format!(
                                    "[ERROR] IMAP session {} error: {}",
                                    connection_id, e
                                ));
                            }

                            // Mark connection as closed
                            state_clone
                                .update_connection_status(
                                    server_id,
                                    connection_id,
                                    ConnectionStatus::Closed,
                                )
                                .await;

                            info!("IMAP connection {} closed", connection_id);
                            let _ = status_clone
                                .send(format!("✗ IMAP connection {} closed", connection_id));
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept IMAP connection: {}", e);
                        let _ = status_tx
                            .send(format!("[ERROR] Failed to accept IMAP connection: {}", e));
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

#[cfg(feature = "imap")]
struct ImapSession<R, W> {
    reader: BufReader<R>,
    writer: Arc<tokio::sync::Mutex<W>>,
    connection_id: ConnectionId,
    server_id: ServerId,
    #[allow(dead_code)]
    remote_addr: SocketAddr,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<ImapProtocol>,
}

#[cfg(feature = "imap")]
impl<R: tokio::io::AsyncRead + Unpin, W: tokio::io::AsyncWrite + Unpin> ImapSession<R, W> {
    async fn handle(&mut self) -> Result<()> {
        // Send greeting via LLM. A failure here used to propagate and drop the socket without
        // a single byte written, so the client sat waiting for a banner that never arrived.
        // RFC 3501 lets a server refuse a connection with an untagged BYE; RFC 5530 gives it
        // a machine-readable reason.
        if let Err(e) = self.send_greeting().await {
            let (code, detail) = imap_failure_code(&e);
            error!(
                "IMAP greeting handler failed on connection {}: {}",
                self.connection_id, e
            );
            let _ = self.status_tx.send(format!(
                "[ERROR] IMAP connection {} refused with BYE [{}]: {}",
                self.connection_id, code, e
            ));
            let bye = format!("* BYE [{}] {}\r\n", code, detail);
            let _ = self.send_response(bye.as_bytes()).await;
            // `send_response` flushes; returning drops the session and closes the socket.
            return Ok(());
        }

        // Main command loop
        loop {
            let mut line = String::new();
            match self.reader.read_line(&mut line).await {
                Ok(0) => {
                    // EOF - client disconnected
                    debug!("IMAP client {} disconnected", self.connection_id);
                    break;
                }
                Ok(n) => {
                    trace!(
                        "IMAP received {} bytes from {}: {}",
                        n,
                        self.connection_id,
                        line.trim()
                    );
                    let _ = self
                        .status_tx
                        .send(format!("[TRACE] IMAP command: {}", line.trim()));

                    // Update bytes received
                    self.app_state
                        .update_connection_stats(
                            self.server_id,
                            self.connection_id,
                            Some(n as u64),
                            None,
                            Some(1),
                            None,
                        )
                        .await;

                    // Parse and handle IMAP command
                    if let Err(e) = self.handle_command(&line).await {
                        // NO, not BAD. BAD means "I did not understand the command", which
                        // invites the client to give up on the command permanently; a backend
                        // failure is a refusal to execute a command we understood fine. The
                        // tag is echoed so the client can correlate, and the RFC 5530 code
                        // tells it whether retrying is worth anything.
                        let (code, detail) = imap_failure_code(&e);
                        error!(
                            "Error handling IMAP command on connection {}: {}",
                            self.connection_id, e
                        );
                        let _ = self.status_tx.send(format!(
                            "[ERROR] IMAP connection {} refusing command with NO [{}]: {}",
                            self.connection_id, code, e
                        ));

                        let (tag, _, _) = parse_imap_command(&line);
                        let error_response = format!("{} NO [{}] {}\r\n", tag, code, detail);
                        let _ = self.send_response(error_response.as_bytes()).await;
                    }

                    // Check if session should logout
                    if let Some((session_state, _, _)) = self
                        .app_state
                        .get_imap_connection_state(self.server_id, self.connection_id)
                        .await
                    {
                        if session_state == ImapSessionState::Logout {
                            break;
                        }
                    }
                }
                Err(e) => {
                    error!("Error reading IMAP command: {}", e);
                    break;
                }
            }
        }

        Ok(())
    }

    async fn send_greeting(&mut self) -> Result<()> {
        let event = Event::new(&IMAP_CONNECTION_EVENT, json!({}));

        let result = call_llm(
            &self.llm_client,
            &self.app_state,
            self.server_id,
            Some(self.connection_id),
            &event,
            self.protocol.as_ref(),
        )
        .await?;

        for action_result in result.protocol_results {
            match action_result {
                ActionResult::Output(data) => {
                    self.send_response(&data).await?;
                }
                ActionResult::CloseConnection => {
                    // Don't close on greeting
                }
                _ => {}
            }
        }

        Ok(())
    }

    async fn handle_command(&mut self, line: &str) -> Result<()> {
        let (tag, command, args) = parse_imap_command(line);

        debug!(
            "IMAP command from {}: tag={}, command={}, args={}",
            self.connection_id, tag, command, args
        );

        // Get current session state
        let (session_state, authenticated_user, selected_mailbox) = self
            .app_state
            .get_imap_connection_state(self.server_id, self.connection_id)
            .await
            .unwrap_or((ImapSessionState::NotAuthenticated, None, None));

        // Handle LOGIN specially for authentication event
        if command.to_uppercase() == "LOGIN" {
            return self.handle_login(&tag, &args).await;
        }

        // Create event for LLM
        let event = Event::new(
            &IMAP_COMMAND_EVENT,
            json!({
                "tag": tag,
                "command": command,
                "args": args,
                "session_state": format!("{:?}", session_state),
                "authenticated_user": authenticated_user,
                "selected_mailbox": selected_mailbox,
            }),
        );

        let result = call_llm(
            &self.llm_client,
            &self.app_state,
            self.server_id,
            Some(self.connection_id),
            &event,
            self.protocol.as_ref(),
        )
        .await?;

        // Execute actions returned by LLM
        for action_result in result.protocol_results {
            match action_result {
                ActionResult::Output(data) => {
                    self.send_response(&data).await?;
                }
                ActionResult::CloseConnection => {
                    // Update session state to Logout
                    self.app_state
                        .update_imap_session_state(
                            self.server_id,
                            self.connection_id,
                            ImapSessionState::Logout,
                        )
                        .await;
                }
                ActionResult::WaitForMore => {
                    // Mark as accumulating (for multi-line commands like APPEND)
                    self.app_state
                        .update_imap_protocol_state(
                            self.server_id,
                            self.connection_id,
                            ProtocolState::Accumulating,
                        )
                        .await;
                }
                _ => {}
            }
        }

        // Handle state transitions based on command
        self.update_session_state(&command, &args).await?;

        Ok(())
    }

    async fn handle_login(&mut self, tag: &str, args: &str) -> Result<()> {
        // Parse LOGIN username password
        let parts: Vec<&str> = args.split_whitespace().collect();
        if parts.len() < 2 {
            let response = format!("{} BAD LOGIN requires username and password\r\n", tag);
            return self.send_response(response.as_bytes()).await;
        }

        let username = parts[0].trim_matches('"');
        let password = parts[1].trim_matches('"');

        debug!("IMAP LOGIN attempt: username={}", username);

        let event = Event::new(
            &IMAP_AUTH_EVENT,
            json!({
                "tag": tag,
                "username": username,
                "password": password,
            }),
        );

        let result = call_llm(
            &self.llm_client,
            &self.app_state,
            self.server_id,
            Some(self.connection_id),
            &event,
            self.protocol.as_ref(),
        )
        .await?;

        // Check if authentication was successful by looking for OK response
        let mut auth_success = false;
        for action_result in &result.protocol_results {
            if let ActionResult::Output(data) = action_result {
                let response = String::from_utf8_lossy(&data);
                if response.contains(&format!("{} OK", tag)) {
                    auth_success = true;
                }
                self.send_response(&data).await?;
            } else if let ActionResult::CloseConnection = action_result {
                // Authentication failed, close connection
                self.app_state
                    .update_imap_session_state(
                        self.server_id,
                        self.connection_id,
                        ImapSessionState::Logout,
                    )
                    .await;
            }
        }

        // If authentication successful, update session state
        if auth_success {
            self.app_state
                .update_imap_connection_state(
                    self.server_id,
                    self.connection_id,
                    Some(ImapSessionState::Authenticated),
                    Some(Some(username.to_string())),
                    None,
                    None,
                )
                .await;
            debug!("IMAP user {} authenticated", username);
        }

        Ok(())
    }

    async fn update_session_state(&mut self, command: &str, args: &str) -> Result<()> {
        let cmd_upper = command.to_uppercase();

        match cmd_upper.as_str() {
            "SELECT" => {
                // Extract mailbox name
                let mailbox = args.split_whitespace().next().unwrap_or("INBOX");
                self.app_state
                    .update_imap_connection_state(
                        self.server_id,
                        self.connection_id,
                        Some(ImapSessionState::Selected),
                        None,
                        Some(Some(mailbox.trim_matches('"').to_string())),
                        Some(false),
                    )
                    .await;
                debug!("IMAP mailbox selected: {}", mailbox);
            }
            "EXAMINE" => {
                // Like SELECT but read-only
                let mailbox = args.split_whitespace().next().unwrap_or("INBOX");
                self.app_state
                    .update_imap_connection_state(
                        self.server_id,
                        self.connection_id,
                        Some(ImapSessionState::Selected),
                        None,
                        Some(Some(mailbox.trim_matches('"').to_string())),
                        Some(true),
                    )
                    .await;
                debug!("IMAP mailbox examined (read-only): {}", mailbox);
            }
            "CLOSE" => {
                // Close selected mailbox, back to Authenticated
                self.app_state
                    .update_imap_connection_state(
                        self.server_id,
                        self.connection_id,
                        Some(ImapSessionState::Authenticated),
                        None,
                        Some(None),
                        Some(false),
                    )
                    .await;
                debug!("IMAP mailbox closed");
            }
            "LOGOUT" => {
                self.app_state
                    .update_imap_session_state(
                        self.server_id,
                        self.connection_id,
                        ImapSessionState::Logout,
                    )
                    .await;
                debug!("IMAP session logout");
            }
            _ => {}
        }

        Ok(())
    }

    async fn send_response(&mut self, data: &[u8]) -> Result<()> {
        let mut writer = self.writer.lock().await;
        writer.write_all(data).await?;
        writer.flush().await?;
        drop(writer);

        // Update stats
        self.app_state
            .update_connection_stats(
                self.server_id,
                self.connection_id,
                None,
                Some(data.len() as u64),
                None,
                Some(1),
            )
            .await;

        trace!("IMAP sent {} bytes to {}", data.len(), self.connection_id);
        let _ = self
            .status_tx
            .send(format!("[TRACE] IMAP sent {} bytes", data.len()));

        Ok(())
    }
}

/// The RFC 5530 response code and human text to use when the LLM backend fails us.
///
/// `UNAVAILABLE` is defined as "temporary failure because a subsystem is down" and is the
/// signal a client should read as "retry later"; it is what capacity exhaustion deserves.
/// Anything else - the backend refusing, timing out, or answering with something we cannot
/// parse - is reported as `SERVERBUG`, which does not invite an immediate retry loop.
///
/// Both are refusals. Neither can be confused with success, which is the point: an `OK` here
/// would tell the client the command was carried out.
#[cfg(feature = "imap")]
fn imap_failure_code(err: &anyhow::Error) -> (&'static str, String) {
    // Response text is a single line; a newline in the error would forge a second response.
    let sanitized =
        crate::utils::truncate::truncate_for_log(&err.to_string(), 200).replace(['\r', '\n'], " ");
    if crate::llm::is_overload_error(err) {
        (
            "UNAVAILABLE",
            format!("netget: backend at capacity, retry later ({})", sanitized),
        )
    } else {
        ("SERVERBUG", format!("netget: {}", sanitized))
    }
}

/// Parse IMAP command line into (tag, command, args)
#[cfg(feature = "imap")]
fn parse_imap_command(line: &str) -> (String, String, String) {
    let trimmed = line.trim();
    let parts: Vec<&str> = trimmed.splitn(3, ' ').collect();

    match parts.len() {
        0 => ("*".to_string(), "".to_string(), "".to_string()),
        1 => (parts[0].to_string(), "".to_string(), "".to_string()),
        2 => (parts[0].to_string(), parts[1].to_string(), "".to_string()),
        _ => (
            parts[0].to_string(),
            parts[1].to_string(),
            parts[2].to_string(),
        ),
    }
}
