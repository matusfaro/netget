//! Regression tests for the per-client LLM call budget.
//!
//! A client protocol drives its own event loop, so nothing structurally prevents a
//! response handler from re-issuing a request forever. The DNS client did exactly
//! that: 211 model calls and then a stack overflow (IMPROVEMENTS.md item 49).
//! Servers are protected by the per-connection Idle/Processing/Accumulating state
//! machine; clients get this hard ceiling instead.
//!
//! No Ollama required — these drive `AppState` directly.
//!
//! Run with:
//!   ./cargo-isolated.sh test --no-default-features --features tcp \
//!       --test client_llm_budget_test -- --test-threads=100

use netget::state::app_state::AppState;
use netget::state::client::ClientInstance;
use netget::state::ClientId;
use std::sync::Arc;

/// Register a client instance so the test has a client to operate on.
async fn add_placeholder_client(state: &Arc<AppState>, proto: &str, remote: &str) -> ClientId {
    let client = ClientInstance::new(
        ClientId::new(0),
        remote.to_string(),
        proto.to_string(),
        "test".to_string(),
    );
    state.add_client(client).await
}

/// The per-client LLM budget must stop a non-converging client dead.
#[tokio::test]
async fn client_llm_call_budget_is_enforced() {
    let state = Arc::new(AppState::new());
    let client_id = add_placeholder_client(&state, "tcp", "127.0.0.1:1").await;

    state.set_client_llm_call_limit(3).await;

    for expected in 1..=3u32 {
        let used = state
            .try_consume_client_llm_call(client_id)
            .await
            .unwrap_or_else(|e| panic!("call {} should be within budget, got: {}", expected, e));
        assert_eq!(used, expected, "budget should count calls monotonically");
    }

    let err = state
        .try_consume_client_llm_call(client_id)
        .await
        .expect_err("the 4th call must be refused once the budget is exhausted");
    assert!(
        err.contains("budget"),
        "the refusal must explain itself, got: {}",
        err
    );

    // Refusal must be sticky, not a one-off.
    assert!(state.try_consume_client_llm_call(client_id).await.is_err());
    assert_eq!(state.get_client_llm_calls(client_id).await, 3);
}

/// A limit of 0 disables the cap (escape hatch for deliberately long sessions).
#[tokio::test]
async fn client_llm_call_budget_can_be_disabled() {
    let state = Arc::new(AppState::new());
    let client_id = add_placeholder_client(&state, "tcp", "127.0.0.1:1").await;

    state.set_client_llm_call_limit(0).await;

    for _ in 0..500 {
        assert!(state.try_consume_client_llm_call(client_id).await.is_ok());
    }
}

/// Budgets are per client, not global — one runaway client must not starve others.
#[tokio::test]
async fn client_llm_call_budget_is_per_client() {
    let state = Arc::new(AppState::new());
    let noisy = add_placeholder_client(&state, "tcp", "127.0.0.1:1").await;
    let quiet = add_placeholder_client(&state, "tcp", "127.0.0.1:2").await;

    state.set_client_llm_call_limit(2).await;

    assert!(state.try_consume_client_llm_call(noisy).await.is_ok());
    assert!(state.try_consume_client_llm_call(noisy).await.is_ok());
    assert!(state.try_consume_client_llm_call(noisy).await.is_err());

    assert!(
        state.try_consume_client_llm_call(quiet).await.is_ok(),
        "a second client must have its own budget"
    );
}
