//! Reverse-shell listener server.
//!
//! A raw-TCP listener that **emulates** the operator side of a reverse shell for authorized
//! security testing, CTF and lab use. An operator connects back to this listener with a plain
//! TCP client (`nc`, `socat`, `ncat`), and the LLM role-plays the shell on the far end: it
//! decides the banner, each command's output and the prompt.
//!
//! Safety: NetGet does **not** execute the operator's commands on this host. Every byte the
//! operator sees is fictional output supplied by the model — the same premise as every other
//! NetGet protocol (impersonating a service without being one). Real command execution is only
//! reachable through the separate, opt-in, unsandboxed scripting layer documented in the
//! top-level `CLAUDE.md`; this protocol never touches it. See `src/server/reverse_shell/CLAUDE.md`.
//!
//! Wire model: there is no framing. The listener reads raw bytes, buffers them into
//! newline-terminated lines, and raises one `reverse_shell_command` event per line. One
//! connection is handled strictly sequentially — the model call for one line finishes before the
//! next line is read — so there is never a concurrent LLM call on a single connection, and input
//! that arrives mid-call sits in the socket buffer until the call returns.

pub mod actions;

use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::state::app_state::AppState;
use crate::state::server::{ConnectionState, ConnectionStatus, ProtocolConnectionInfo};
use crate::{console_debug, console_info, console_warn};
use actions::{
    ReverseShellProtocol, REVERSE_SHELL_COMMAND_EVENT, REVERSE_SHELL_SESSION_OPENED_EVENT,
};
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};

/// Largest single line accepted from the operator, in bytes.
///
/// The line accumulator grows until a newline arrives; without a cap a peer that never sends one
/// could grow it without bound. A command line far longer than this is not a real shell command.
const MAX_LINE_LEN: usize = 64 * 1024;

/// Reverse-shell listener.
pub struct ReverseShellServer;

impl ReverseShellServer {
    /// Bind the listener and spawn the accept loop.
    ///
    /// Awaits the bind so a failure (address in use, permission) is returned as `Err` and the
    /// server is marked `Error` rather than lying about being `Running`. The accept-loop
    /// `JoinHandle` is registered so `stop_server` releases the socket.
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

        // Keep the address last on the line: the E2E harness parses the port from after "on ".
        console_info!(
            status_tx,
            "Reverse-shell listener (emulation) listening on {}",
            local_addr
        );

        let task_registrar = app_state.clone();
        let accept_handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id =
                            ConnectionId::new(app_state.get_next_unified_id().await);
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);

                        info!("Reverse-shell operator connected from {}", remote_addr);
                        let _ = status_tx.send(format!(
                            "[INFO] Reverse-shell operator connected from {}",
                            remote_addr
                        ));

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_connection(
                                stream,
                                connection_id,
                                remote_addr,
                                local_addr_conn,
                                server_id,
                                state_clone,
                                status_clone,
                                llm_clone,
                            )
                            .await
                            {
                                error!("Reverse-shell connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        // Break rather than spin: a persistent accept error (EMFILE, socket
                        // closed under us) would otherwise loop at full CPU.
                        error!("Reverse-shell accept failed: {}", e);
                        let _ = status_tx
                            .send(format!("✗ Reverse-shell accept failed, stopping: {}", e));
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

    /// Handle one operator connection: session-open event, then a line loop.
    #[allow(clippy::too_many_arguments)]
    async fn handle_connection(
        stream: TcpStream,
        connection_id: ConnectionId,
        remote_addr: SocketAddr,
        local_addr: SocketAddr,
        server_id: crate::state::ServerId,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        llm_client: OllamaClient,
    ) -> Result<()> {
        let (mut read_half, write_half) = tokio::io::split(stream);
        let write_half = Arc::new(Mutex::new(write_half));

        let now = std::time::Instant::now();
        let conn_state = ConnectionState {
            id: connection_id,
            remote_addr,
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

        let mut conn = ShellConnection {
            connection_id,
            server_id,
            app_state: app_state.clone(),
            llm_client,
            protocol: ReverseShellProtocol::new(),
            status_tx: status_tx.clone(),
            write_half,
        };

        let result = conn.run(&mut read_half).await;

        app_state
            .remove_connection_from_server(server_id, connection_id)
            .await;
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        result
    }
}

/// Per-connection state and behaviour for one operator session.
struct ShellConnection {
    connection_id: ConnectionId,
    server_id: crate::state::ServerId,
    app_state: Arc<AppState>,
    llm_client: OllamaClient,
    protocol: ReverseShellProtocol,
    status_tx: mpsc::UnboundedSender<String>,
    write_half: Arc<Mutex<tokio::io::WriteHalf<TcpStream>>>,
}

/// What the model decided in answer to one event.
#[derive(Default)]
struct Outcome {
    /// Bytes to write to the operator, in order.
    output: Vec<u8>,
    /// The session should close after writing `output`.
    close: bool,
    /// No usable action came back (an LLM error, or an empty/failed action list).
    no_answer: bool,
}

impl ShellConnection {
    /// The session: greet on open, then one event per operator command line.
    async fn run(&mut self, read_half: &mut tokio::io::ReadHalf<TcpStream>) -> Result<()> {
        // Session-opened event.
        let open_event = Event::new(&REVERSE_SHELL_SESSION_OPENED_EVENT, serde_json::json!({}));
        let outcome = self.consult(&open_event).await;
        if self.apply(outcome).await? {
            return Ok(());
        }

        let mut buffer = vec![0u8; 8192];
        let mut line: Vec<u8> = Vec::new();
        let mut first_command = true;

        loop {
            let n = match read_half.read(&mut buffer).await {
                Ok(0) => {
                    debug!("Reverse-shell operator {} disconnected", self.connection_id);
                    break;
                }
                Ok(n) => n,
                Err(e) => {
                    error!("Reverse-shell read error on {}: {}", self.connection_id, e);
                    break;
                }
            };

            for &byte in &buffer[..n] {
                if byte == b'\n' {
                    // Strip a trailing CR so CRLF and LF clients look identical to the model.
                    if line.last() == Some(&b'\r') {
                        line.pop();
                    }
                    let command = String::from_utf8_lossy(&line).to_string();
                    let empty = command.trim().is_empty();
                    line.clear();

                    console_debug!(
                        self.status_tx,
                        "Reverse-shell command from {}: {:?}",
                        self.connection_id,
                        command
                    );

                    let event = Event::new(
                        &REVERSE_SHELL_COMMAND_EVENT,
                        serde_json::json!({
                            "command": command,
                            "first_command": first_command,
                            "empty": empty,
                        }),
                    );
                    first_command = false;

                    let outcome = self.consult(&event).await;
                    if self.apply(outcome).await? {
                        return Ok(());
                    }
                } else {
                    line.push(byte);
                    if line.len() > MAX_LINE_LEN {
                        console_warn!(
                            self.status_tx,
                            "Reverse-shell line from {} exceeded {} bytes, closing",
                            self.connection_id,
                            MAX_LINE_LEN
                        );
                        self.shutdown().await;
                        return Ok(());
                    }
                }
            }
        }

        Ok(())
    }

    /// Ask the event handlers (script → static → LLM) what to do.
    ///
    /// Never returns an error: a failure has to fail *closed* on the caller side, so the reason
    /// is carried back in `Outcome::no_answer` and the caller closes the socket. Silence is never
    /// treated as approval — there is nothing to approve here, but the same discipline keeps the
    /// "no answer" path distinct from a deliberate "no output" (`no_shell_output`).
    async fn consult(&self, event: &Event) -> Outcome {
        let mut outcome = Outcome::default();

        let execution = match call_llm(
            &self.llm_client,
            &self.app_state,
            self.server_id,
            Some(self.connection_id),
            event,
            &self.protocol,
        )
        .await
        {
            Ok(execution) => execution,
            Err(e) => {
                warn!(
                    "Reverse-shell event {} not answered: {}",
                    event.event_type.id, e
                );
                let _ = self.status_tx.send(format!(
                    "[WARN] Reverse-shell {} not answered: {}",
                    event.event_type.id, e
                ));
                outcome.no_answer = true;
                return outcome;
            }
        };

        for message in execution.messages {
            let _ = self.status_tx.send(message);
        }

        let mut saw_result = false;
        for result in execution.protocol_results {
            match result {
                ActionResult::Output(data) => {
                    saw_result = true;
                    outcome.output.extend_from_slice(&data);
                }
                ActionResult::CloseConnection => {
                    saw_result = true;
                    outcome.close = true;
                }
                // no_shell_output: a deliberate decision to print nothing. Structurally distinct
                // from a missing answer — the session stays open and no placeholder is invented.
                ActionResult::WaitForMore | ActionResult::NoAction => {
                    saw_result = true;
                }
                other => {
                    warn!(
                        "Reverse-shell ignoring unexpected action result: {:?}",
                        other
                    );
                }
            }
        }

        if !saw_result {
            outcome.no_answer = true;
        }

        outcome
    }

    /// Write the decided output, then close if asked or if no usable answer came back.
    ///
    /// Returns true when the connection has been closed and the caller must stop.
    ///
    /// Fail-closed: a missing answer (LLM error, empty/failed action list) shuts the socket down
    /// with a FIN rather than falling through to any permissive default — there is no fabricated
    /// output and no fake prompt, so an LLM outage is visible to the operator as a dropped
    /// session, not as a silently-working shell.
    async fn apply(&mut self, outcome: Outcome) -> Result<bool> {
        if !outcome.output.is_empty() {
            let mut writer = self.write_half.lock().await;
            writer.write_all(&outcome.output).await?;
            writer.flush().await?;
            drop(writer);
            console_debug!(
                self.status_tx,
                "Reverse-shell sent {} bytes to {}",
                outcome.output.len(),
                self.connection_id
            );
        }

        if outcome.no_answer {
            let _ = self.status_tx.send(format!(
                "✗ Reverse-shell closing {} (no usable answer from model)",
                self.connection_id
            ));
            self.shutdown().await;
            return Ok(true);
        }

        if outcome.close {
            let _ = self.status_tx.send(format!(
                "✗ Reverse-shell session {} ended",
                self.connection_id
            ));
            self.shutdown().await;
            return Ok(true);
        }

        Ok(false)
    }

    /// Half-close the write direction so the operator's next read returns EOF.
    async fn shutdown(&self) {
        let mut writer = self.write_half.lock().await;
        let _ = writer.shutdown().await;
    }
}
