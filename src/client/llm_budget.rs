//! Per-client LLM call budget.
//!
//! Every client protocol drives its own event loop: an event arrives, the loop
//! calls the model, executes the returned actions, and those actions may produce
//! another event. Nothing in that shape guarantees convergence — a handler that
//! always answers "issue another request" loops until the process dies. That is
//! not hypothetical: the DNS client made 211 model calls and then overflowed the
//! stack (`IMPROVEMENTS.md` item 49).
//!
//! Servers are protected by the per-connection Idle/Processing/Accumulating state
//! machine, which prevents re-entrancy. Clients have no equivalent, so this module
//! provides the backstop instead: a hard ceiling on LLM calls per client session,
//! enforced in one place.
//!
//! ## How it is applied
//!
//! [`call_llm_for_client`] here is a drop-in replacement for
//! [`crate::llm::action_helper::call_llm_for_client`] with an identical signature.
//! Client protocols import this one:
//!
//! ```ignore
//! use crate::client::llm_budget::call_llm_for_client;
//! ```
//!
//! Once the budget is exhausted the call returns `Err` with an explanatory message
//! instead of contacting the model, which every existing call site already logs.
//! A protocol that wants to fail louder can match on the error.
//!
//! The cap is `AppState::DEFAULT_CLIENT_LLM_CALL_LIMIT` (100), overridable at
//! runtime via `NETGET_CLIENT_LLM_CALL_LIMIT` (`0` = unlimited) or
//! `AppState::set_client_llm_call_limit()`.

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{error, warn};

use crate::llm::actions::client_trait::Client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::ClientId;

/// Fraction of the budget after which each call emits a warning.
const WARN_AT_PERCENT: u32 = 80;

/// Budget-checked wrapper around [`crate::llm::action_helper::call_llm_for_client`].
///
/// Consumes one unit of the calling client's LLM call budget and fails without
/// contacting the model once that budget is exhausted. See the module docs.
#[allow(clippy::too_many_arguments)]
pub async fn call_llm_for_client(
    llm_client: &OllamaClient,
    state: &AppState,
    client_id: String,
    instruction: &str,
    memory: &str,
    event: Option<&Event>,
    protocol: &dyn Client,
    status_tx: &mpsc::UnboundedSender<String>,
) -> Result<ClientLlmResult> {
    if let Some(cid) = ClientId::from_string(&client_id) {
        match state.try_consume_client_llm_call(cid).await {
            Ok(used) => {
                let limit = state.get_client_llm_call_limit().await;
                if limit > 0 && used * 100 >= limit * WARN_AT_PERCENT {
                    warn!(
                        "Client {} has used {}/{} of its LLM call budget",
                        cid, used, limit
                    );
                    let _ = status_tx.send(format!(
                        "[CLIENT] ⚠ client {} has used {}/{} LLM calls",
                        cid, used, limit
                    ));
                }
            }
            Err(msg) => {
                error!("{}", msg);
                let _ = status_tx.send(format!("[CLIENT] ✖ {}", msg));
                return Err(anyhow::anyhow!(msg));
            }
        }
    }

    crate::llm::action_helper::call_llm_for_client(
        llm_client,
        state,
        client_id,
        instruction,
        memory,
        event,
        protocol,
        status_tx,
    )
    .await
}
