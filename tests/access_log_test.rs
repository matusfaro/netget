//! Unit tests for the AppState access-log ring buffer.
//!
//! No Ollama required — exercises record/list/get and the owner
//! (server/client) discriminator directly.
//!
//! Run with:
//!   ./cargo-isolated.sh test --no-default-features --features tcp --test access_log_test -- --test-threads=100

use netget::state::app_state::AppState;
use netget::state::AccessLogOwner;
use serde_json::json;

#[tokio::test]
async fn records_and_retrieves_entries_newest_first() {
    let state = AppState::new();

    state
        .record_access_log(
            AccessLogOwner::Server(1),
            "http",
            Some(7),
            "http_request",
            json!({"method": "GET", "path": "/"}),
            vec![json!({"type": "send_http_response", "status_code": 200, "body": "hi"})],
        )
        .await;
    state
        .record_access_log(
            AccessLogOwner::Server(1),
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

    // Record more than the retained capacity (1000)
    for i in 0..1050u64 {
        state
            .record_access_log(
                AccessLogOwner::Server(1),
                "tcp",
                None,
                "tcp_data",
                json!({"n": i}),
                vec![],
            )
            .await;
    }

    let all = state.list_access_logs(None).await;
    assert_eq!(all.len(), 1000, "buffer should cap at 1000 entries");

    // Oldest 50 ids (1..=50) should have been dropped; newest id is 1050
    assert_eq!(all[0].id, 1050);
    assert!(state.get_access_log(50).await.is_none(), "id 50 aged out");
    assert!(state.get_access_log(51).await.is_some(), "id 51 retained");
}

#[tokio::test]
async fn server_and_client_entries_coexist_and_filter() {
    let state = AppState::new();

    state
        .record_access_log(
            AccessLogOwner::Server(1),
            "tcp",
            Some(3),
            "tcp_data_received",
            json!({"a": 1}),
            vec![],
        )
        .await;
    state
        .record_access_log(
            AccessLogOwner::Client(2),
            "tcp",
            None,
            "tcp_data_received",
            json!({"b": 2}),
            vec![],
        )
        .await;
    state
        .record_access_log(
            AccessLogOwner::Server(1),
            "tcp",
            Some(3),
            "tcp_data_received",
            json!({"c": 3}),
            vec![],
        )
        .await;
    state
        .record_access_log(
            AccessLogOwner::Server(9),
            "http",
            None,
            "http_request",
            json!({"d": 4}),
            vec![],
        )
        .await;

    let all = state.list_access_logs(None).await;
    assert_eq!(all.len(), 4);

    let server1 = state
        .list_access_logs_for(Some(AccessLogOwner::Server(1)), None)
        .await;
    assert_eq!(server1.len(), 2);
    assert!(server1
        .iter()
        .all(|e| e.server_id == Some(1) && e.client_id.is_none()));

    let client2 = state
        .list_access_logs_for(Some(AccessLogOwner::Client(2)), None)
        .await;
    assert_eq!(client2.len(), 1);
    assert_eq!(client2[0].client_id, Some(2));
    assert!(client2[0].server_id.is_none());

    // owner: None == everything, same as list_access_logs.
    let unfiltered = state.list_access_logs_for(None, Some(2)).await;
    assert_eq!(unfiltered.len(), 2);
    assert_eq!(unfiltered[0].id, 4);
}

/// MCP-compat guarantee: a server entry's JSON keeps the pre-existing shape —
/// `server_id` present as a bare number, no `client_id` key at all.
#[tokio::test]
async fn server_entry_json_shape_is_backward_compatible() {
    let state = AppState::new();
    state
        .record_access_log(
            AccessLogOwner::Server(5),
            "http",
            Some(1),
            "http_request",
            json!({}),
            vec![],
        )
        .await;
    state
        .record_access_log(
            AccessLogOwner::Client(6),
            "tcp",
            None,
            "tcp_data_received",
            json!({}),
            vec![],
        )
        .await;

    let server_entry = state.get_access_log(1).await.unwrap();
    let json = serde_json::to_value(&server_entry).unwrap();
    assert_eq!(json["server_id"], 5);
    assert!(
        json.get("client_id").is_none(),
        "client_id key must be absent on server entries"
    );

    let client_entry = state.get_access_log(2).await.unwrap();
    let json = serde_json::to_value(&client_entry).unwrap();
    assert_eq!(json["client_id"], 6);
    assert!(
        json.get("server_id").is_none(),
        "server_id key must be absent on client entries"
    );
}
