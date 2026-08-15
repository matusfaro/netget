//! Standard I/O (stdin/stdout/stderr) server implementation.
//!
//! Platform: Unix/Linux/macOS only. NetGet itself becomes the child process behind a pipe
//! (`someprogram | netget ... | otherprogram`): it reads from its own stdin, hands each chunk to
//! the LLM as a `stdio_input_received` event, and the model decides what to emit on stdout/stderr.
//!
//! ## Coexistence (the subtlety)
//!
//! This protocol takes over the process's real stdin/stdout, so it is fundamentally incompatible
//! with two run modes and refuses to start under either:
//!
//! - **the interactive TUI** — ratatui owns the terminal; detected via `stdin` being a TTY.
//! - **`--mcp` stdio** — JSON-RPC owns stdin/stdout; detected via the process argv.
//!
//! It is intended for headless / one-shot piped use (`prog | netget --non-interactive '...' | prog`)
//! or under `--mcp-http`, where stdin is a pipe and tracing logs go to stderr. Only one stdio
//! server may run per process (an `AtomicBool` claim). See `CLAUDE.md` for the caveat that the
//! non-interactive status stream currently shares stdout.
#![cfg(unix)]

pub mod actions;

use anyhow::Result;
use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ActionResult;
use crate::logging::emit::Log;
use crate::protocol::Event;
use crate::server::stdio::actions::{
    StdioProtocol, STDIO_INPUT_CLOSED_EVENT, STDIO_INPUT_RECEIVED_EVENT, STDIO_STARTED_EVENT,
};
use crate::state::app_state::AppState;

/// At most one stdio server may own the process's standard streams at a time.
static STDIO_CLAIMED: AtomicBool = AtomicBool::new(false);

/// Releases the stdio claim when the read task ends (including on abort).
struct StdioClaimGuard;

impl Drop for StdioClaimGuard {
    fn drop(&mut self) {
        STDIO_CLAIMED.store(false, Ordering::SeqCst);
    }
}

/// True when this process was launched as an MCP stdio server (`--mcp` / `--mcp-stdio`), in which
/// case JSON-RPC owns stdin/stdout and a stdio protocol server must not contend for them.
/// `--mcp-http` is a different flag and does not match.
fn running_as_mcp_stdio() -> bool {
    std::env::args().any(|a| a == "--mcp" || a == "--mcp-stdio")
}

/// Standard I/O server.
pub struct StdioServer;

impl StdioServer {
    /// Claim the process stdio and spawn the read/dispatch loop.
    ///
    /// Returns `Err` — so `server_startup` records `ServerStatus::Error` — when the environment
    /// cannot support a stdio takeover (a TTY/TUI, `--mcp` stdio, or another stdio server already
    /// running). No half-started state is left behind on refusal.
    pub async fn spawn_with_llm_actions(
        send_first: bool,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<()> {
        if std::io::stdin().is_terminal() {
            anyhow::bail!(
                "stdio server refuses to run under an interactive terminal: the TUI owns the \
                 terminal. Use it as a pipe filter (prog | netget --non-interactive '...' | prog) \
                 or under --mcp-http."
            );
        }
        if running_as_mcp_stdio() {
            anyhow::bail!(
                "stdio server cannot run under --mcp (stdio): MCP JSON-RPC already owns this \
                 process's stdin/stdout. Use --mcp-http, or run NetGet headless as a pipe filter."
            );
        }
        if STDIO_CLAIMED
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            anyhow::bail!(
                "another stdio server already owns this process's stdin/stdout; only one is \
                 possible per process."
            );
        }
        // From here on we own the claim; the guard releases it on task end.
        let claim = StdioClaimGuard;

        Log::new(Some(&status_tx)).info("stdio server claimed process stdin/stdout");

        let protocol = Arc::new(StdioProtocol::new());
        let task_registrar = app_state.clone();

        let handle = tokio::spawn(async move {
            let _claim = claim; // released on return/abort

            if send_first {
                let event = Event::new(&STDIO_STARTED_EVENT, serde_json::json!({}));
                let _ = Self::dispatch(
                    &event,
                    &llm_client,
                    &app_state,
                    server_id,
                    protocol.as_ref(),
                    &status_tx,
                )
                .await;
            }

            let mut stdin = tokio::io::stdin();
            let mut buffer = vec![0u8; 8192];

            loop {
                match stdin.read(&mut buffer).await {
                    Ok(0) => {
                        // EOF: upstream closed the pipe. Let the model emit any final output.
                        Log::new(Some(&status_tx)).debug("stdio stdin reached EOF");
                        let event = Event::new(&STDIO_INPUT_CLOSED_EVENT, serde_json::json!({}));
                        let _ = Self::dispatch(
                            &event,
                            &llm_client,
                            &app_state,
                            server_id,
                            protocol.as_ref(),
                            &status_tx,
                        )
                        .await;
                        break;
                    }
                    Ok(n) => {
                        let data = &buffer[..n];
                        let (data_str, encoding) = if data
                            .iter()
                            .all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace())
                        {
                            (String::from_utf8_lossy(data).to_string(), "utf8")
                        } else {
                            (hex::encode(data), "hex")
                        };

                        Log::new(Some(&status_tx)).debug(format!(
                            "stdio read {} bytes from stdin ({})",
                            n, encoding
                        ));

                        let event = Event::new(
                            &STDIO_INPUT_RECEIVED_EVENT,
                            serde_json::json!({ "data": data_str, "encoding": encoding }),
                        );
                        let should_close = Self::dispatch(
                            &event,
                            &llm_client,
                            &app_state,
                            server_id,
                            protocol.as_ref(),
                            &status_tx,
                        )
                        .await;
                        if should_close {
                            info!("stdio session closed by model (close_stdio)");
                            break;
                        }
                    }
                    Err(e) => {
                        Log::new(Some(&status_tx)).error(format!("stdio stdin read error: {}", e));
                        break;
                    }
                }
            }
        });

        task_registrar.register_server_task(server_id, handle).await;
        Ok(())
    }

    /// Run one LLM round-trip for `event` and act on the results. Returns true if the model asked
    /// to close the stdio session.
    async fn dispatch(
        event: &Event,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        server_id: crate::state::ServerId,
        protocol: &StdioProtocol,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> bool {
        match call_llm(llm_client, app_state, server_id, None, event, protocol).await {
            Ok(result) => {
                for msg in result.messages {
                    let _ = status_tx.send(msg);
                }
                let mut should_close = false;
                for pr in result.protocol_results {
                    match pr {
                        ActionResult::Output(bytes) => {
                            Self::write_stream(false, &bytes, status_tx).await;
                        }
                        ActionResult::Custom { name, data } if name == "stdio_stderr" => {
                            if let Some(hex_str) = data.get("hex").and_then(|v| v.as_str()) {
                                if let Ok(bytes) = hex::decode(hex_str) {
                                    Self::write_stream(true, &bytes, status_tx).await;
                                }
                            }
                        }
                        ActionResult::CloseConnection => should_close = true,
                        _ => {}
                    }
                }
                should_close
            }
            Err(e) => {
                // Fail closed: emit nothing, report on both channels.
                Log::new(Some(status_tx)).error(format!("LLM error for stdio event: {}", e));
                false
            }
        }
    }

    /// Write `bytes` to stdout (`stderr = false`) or stderr (`stderr = true`), flushing.
    async fn write_stream(stderr: bool, bytes: &[u8], status_tx: &mpsc::UnboundedSender<String>) {
        let res = if stderr {
            let mut out = tokio::io::stderr();
            out.write_all(bytes).await.and(out.flush().await)
        } else {
            let mut out = tokio::io::stdout();
            out.write_all(bytes).await.and(out.flush().await)
        };
        match res {
            Ok(()) => {
                let dest = if stderr { "stderr" } else { "stdout" };
                debug!("stdio wrote {} bytes to {}", bytes.len(), dest);
            }
            Err(e) => {
                Log::new(Some(status_tx)).error(format!("stdio write error: {}", e));
            }
        }
    }
}
