//! Generic adoption helpers for messaging ONE connection of a server.
//!
//! The server-side mirror of `client/command_support.rs`: the dashboard's
//! "message this peer" (and any programmatic caller) injects an action into a
//! specific live connection via [`AppState::send_to_peer`]. A protocol opts in
//! per connection:
//!
//! ```ignore
//! // In the accept loop, once the connection is registered:
//! let peer_rx = peer_support::register_peer_channel(&app_state, server_id, conn.as_u32()).await;
//! peer_support::spawn_peer_command_task(
//!     peer_rx, protocol.clone(), app_state.clone(), server_id, connection_id,
//!     write_half_arc.clone(), status_tx.clone(),
//! );
//! // In the connection's close paths:
//! app_state.remove_peer_handle(server_id, connection_id.as_u32()).await;
//! ```
//!
//! The command task executes the action through the same central executor the
//! LLM path uses (`execute_actions` + the protocol's own `execute_action`), so
//! an injected `send_tcp_data` is encoded by exactly the code that encodes the
//! model's. `ActionResult::Output` bytes are written to the connection's write
//! half; everything else is reported as executed.
//!
//! Only protocols that adopt this offer the affordance — the UI shows
//! "message this peer" exactly where a handle exists, which is the honest
//! rendering of "only if the protocol permits it".

use std::sync::Arc;

use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, warn};

use crate::llm::actions::protocol_trait::{ActionResult, Server};
use crate::state::client_handles::{
    ClientCommand, ClientHandle, ClientSendOutcome, CLIENT_COMMAND_CAPACITY,
};
use crate::state::{AccessLogOwner, AppState, ServerId};

/// Create and register the command channel for one live connection.
pub async fn register_peer_channel(
    state: &AppState,
    server_id: ServerId,
    connection_id: u32,
) -> mpsc::Receiver<ClientCommand> {
    let (command_tx, command_rx) = mpsc::channel(CLIENT_COMMAND_CAPACITY);
    state
        .register_peer_handle(server_id, connection_id, ClientHandle { command_tx })
        .await;
    command_rx
}

/// Drive a connection's command channel until it closes (the handle is dropped
/// by the connection's close path or by server teardown).
///
/// Runs as its own task: unlike the client loops, a server connection task
/// blocks in `read()` without a select, and injected sends must not wait for
/// the peer to talk first. Writes are safe against the reader's own responses
/// because both go through the same `Arc<Mutex<WriteHalf>>`.
pub fn spawn_peer_command_task<W>(
    mut command_rx: mpsc::Receiver<ClientCommand>,
    protocol: Arc<dyn Server>,
    state: Arc<AppState>,
    server_id: ServerId,
    connection_id: u32,
    write_half: Arc<Mutex<W>>,
    status_tx: mpsc::UnboundedSender<String>,
) where
    W: AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        while let Some(command) = command_rx.recv().await {
            handle_peer_command(
                protocol.as_ref(),
                &state,
                server_id,
                connection_id,
                &write_half,
                command,
                &status_tx,
            )
            .await;
        }
        debug!(
            "peer command task for server #{} connection #{} ended",
            server_id.as_u32(),
            connection_id
        );
    });
}

/// Execute one injected action against a connection and reply with the outcome.
async fn handle_peer_command<W>(
    protocol: &dyn Server,
    state: &AppState,
    server_id: ServerId,
    connection_id: u32,
    write_half: &Arc<Mutex<W>>,
    command: ClientCommand,
    status_tx: &mpsc::UnboundedSender<String>,
) where
    W: AsyncWrite + Unpin,
{
    let action = command.action.clone();
    let outcome = execute_peer_action(protocol, state, server_id, write_half, &action).await;

    // Record it in the access log under this server + connection, so the send
    // appears in the peer's request list exactly like LLM-produced traffic.
    let outcome_json = match &outcome {
        Ok(outcome) => serde_json::to_value(outcome).unwrap_or(serde_json::Value::Null),
        Err(e) => serde_json::json!({"error": e.to_string()}),
    };
    state
        .record_access_log(
            AccessLogOwner::Server(server_id.as_u32()),
            protocol.protocol_name(),
            Some(connection_id),
            "injected_action",
            action,
            vec![outcome_json],
        )
        .await;

    if let Err(e) = &outcome {
        warn!(
            "injected action on server #{} connection #{} failed: {}",
            server_id.as_u32(),
            connection_id,
            e
        );
    }
    let _ = status_tx.send("__UPDATE_UI__".to_string());
    let _ = command.reply_tx.send(outcome);
}

/// Run the action through the central executor and write any `Output` bytes.
async fn execute_peer_action<W>(
    protocol: &dyn Server,
    state: &AppState,
    server_id: ServerId,
    write_half: &Arc<Mutex<W>>,
    action: &serde_json::Value,
) -> anyhow::Result<ClientSendOutcome>
where
    W: AsyncWrite + Unpin,
{
    let result = crate::llm::actions::executor::execute_actions(
        vec![action.clone()],
        state,
        Some(protocol),
        Some(server_id),
        None,
    )
    .await?;

    let mut bytes_sent = 0usize;
    let mut details: Vec<String> = Vec::new();
    let mut stack: Vec<ActionResult> = result.protocol_results;
    stack.reverse();
    while let Some(item) = stack.pop() {
        match item {
            ActionResult::Output(bytes) => {
                let mut write = write_half.lock().await;
                write.write_all(&bytes).await?;
                write.flush().await?;
                bytes_sent += bytes.len();
            }
            ActionResult::Multiple(items) => {
                for inner in items.into_iter().rev() {
                    stack.push(inner);
                }
            }
            ActionResult::CloseConnection => details.push("close requested".to_string()),
            ActionResult::WaitForMore | ActionResult::NoAction => {}
            other => details.push(format!("{other:?}")),
        }
    }

    if bytes_sent > 0 {
        Ok(ClientSendOutcome::Sent { bytes_sent })
    } else if details.is_empty() {
        Ok(ClientSendOutcome::Executed {
            detail: "executed (nothing to write)".to_string(),
        })
    } else {
        Ok(ClientSendOutcome::Executed {
            detail: crate::utils::truncate_for_log(&details.join("; "), 160),
        })
    }
}
