//! Unit tests for the AppState access-log ring buffer.
//!
//! No Ollama required — exercises record/list/get directly.
//!
//! Run with:
//!   ./cargo-isolated.sh test --no-default-features --features tcp --test access_log_test -- --test-threads=100

use netget::state::app_state::AppState;
use serde_json::json;

#[tokio::test]
async fn records_and_retrieves_entries_newest_first() {
    let state = AppState::new();

    state
        .record_access_log(
            1,
            "http",
            Some(7),
            "http_request",
            json!({"method": "GET", "path": "/"}),
            vec![json!({"type": "send_http_response", "status_code": 200, "body": "hi"})],
        )
        .await;
    state
        .record_access_log(
            1,
            "http",
            Some(7),
            "http_request",
            json!({"method": "GET", "path": "/recipe/pancake"}),
            vec![json!({"type": "send_http_response", "status_code": 404})],
        )
        .await;

    // Newest first
    let recent = state.list_access_logs(Some(10)).await;
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].id, 2);
    assert_eq!(recent[1].id, 1);
    assert_eq!(recent[0].request["path"], "/recipe/pancake");

    // Look up a specific entry and confirm request + response round-trip
    let entry = state.get_access_log(1).await.expect("entry #1 exists");
    assert_eq!(entry.protocol, "http");
    assert_eq!(entry.connection_id, Some(7));
    assert_eq!(entry.request["method"], "GET");
    assert_eq!(entry.response[0]["status_code"], 200);

    // Missing id yields None
    assert!(state.get_access_log(999).await.is_none());
}

#[tokio::test]
async fn ring_buffer_caps_and_drops_oldest() {
    let state = AppState::new();

    // Record more than the retained capacity (200)
    for i in 0..250u64 {
        state
            .record_access_log(1, "tcp", None, "tcp_data", json!({"n": i}), vec![])
            .await;
    }

    let all = state.list_access_logs(None).await;
    assert_eq!(all.len(), 200, "buffer should cap at 200 entries");

    // Oldest 50 ids (1..=50) should have been dropped; newest id is 250
    assert_eq!(all[0].id, 250);
    assert!(state.get_access_log(50).await.is_none(), "id 50 aged out");
    assert!(state.get_access_log(51).await.is_some(), "id 51 retained");
}
