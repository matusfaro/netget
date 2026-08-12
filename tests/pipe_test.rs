//! Tests for pipes: connecting one instance's events into another instance's input.
//!
//! * Unit tests (no protocol features): cycle refusal, bad-target error, and the
//!   structured field mapping / payload encoding.
//! * End-to-end (needs `http` + `tcp`): an HTTP server whose access events drive a
//!   write into a second NetGet TCP server. We start both, make a request to the
//!   HTTP server, and assert the TCP server received the mapped log line at the
//!   protocol level (its read loop reports the bytes on its status stream).
//!
//! Run:
//!   ./cargo-isolated.sh test --no-default-features --features http,mysql,tcp \
//!       --test pipe_test -- --test-threads=100

use netget::pipe::{render_payload, would_create_cycle, PipeSpec};
use netget::state::app_state::AppState;
use netget::state::server::ServerInstance;
use netget::state::ServerId;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Register a placeholder server so `add_pipe`'s existence checks pass. Returns
/// the assigned id (the ctor id is ignored — `add_server` allocates).
async fn add_placeholder(state: &Arc<AppState>, proto: &str) -> ServerId {
    let server = ServerInstance::new(ServerId::new(0), 0, proto.to_string(), String::new());
    state.add_server(server).await
}

fn map_of(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

// ---------------------------------------------------------------------------
// Pure mapping / payload tests
// ---------------------------------------------------------------------------

#[test]
fn render_payload_substitutes_event_fields() {
    let spec = PipeSpec {
        id: 1,
        from: 1,
        on: "http_request".to_string(),
        to: 2,
        as_action: "send_tcp_data".to_string(),
        map: map_of(&[("data", "{method} {path} host={headers.host}\n")]),
    };
    let event = serde_json::json!({
        "method": "GET",
        "path": "/hello",
        "headers": { "host": "example.com" }
    });
    let bytes = render_payload(&spec, &event).expect("render");
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        "GET /hello host=example.com\n"
    );
}

#[test]
fn render_payload_missing_field_renders_empty() {
    let spec = PipeSpec {
        id: 1,
        from: 1,
        on: "e".to_string(),
        to: 2,
        as_action: "send_tcp_data".to_string(),
        map: map_of(&[("data", "[{nope}]")]),
    };
    let bytes = render_payload(&spec, &serde_json::json!({})).expect("render");
    assert_eq!(String::from_utf8(bytes).unwrap(), "[]");
}

#[test]
fn render_payload_honours_hex_encoding() {
    let spec = PipeSpec {
        id: 1,
        from: 1,
        on: "e".to_string(),
        to: 2,
        as_action: "send_tcp_data".to_string(),
        map: map_of(&[("data", "{blob}"), ("encoding", "hex")]),
    };
    let event = serde_json::json!({ "blob": "48656c6c6f" });
    let bytes = render_payload(&spec, &event).expect("render");
    assert_eq!(bytes, b"Hello");
}

#[test]
fn render_payload_requires_data_field() {
    let spec = PipeSpec {
        id: 7,
        from: 1,
        on: "e".to_string(),
        to: 2,
        as_action: "send_tcp_data".to_string(),
        map: map_of(&[("not_data", "x")]),
    };
    assert!(render_payload(&spec, &serde_json::json!({})).is_err());
}

// ---------------------------------------------------------------------------
// Cycle detection (pure) + through the real add_pipe path
// ---------------------------------------------------------------------------

#[test]
fn would_create_cycle_detects_self_and_back_edges() {
    // No pipes yet: self-loop is a cycle, distinct edge is not.
    assert!(would_create_cycle(&[], 1, 1));
    assert!(!would_create_cycle(&[], 1, 2));

    let existing = vec![PipeSpec {
        id: 1,
        from: 1,
        on: "e".to_string(),
        to: 2,
        as_action: "a".to_string(),
        map: BTreeMap::new(),
    }];
    // 2 -> 1 closes 1 -> 2 -> 1.
    assert!(would_create_cycle(&existing, 2, 1));
    // 2 -> 3 is fine.
    assert!(!would_create_cycle(&existing, 2, 3));
}

#[tokio::test]
async fn add_pipe_refuses_cycles() {
    let state = Arc::new(AppState::new());
    let a = add_placeholder(&state, "http").await;
    let b = add_placeholder(&state, "tcp").await;

    // A -> B is accepted.
    state
        .add_pipe(
            a,
            "http_request".into(),
            b,
            "send_tcp_data".into(),
            map_of(&[("data", "x")]),
        )
        .await
        .expect("A->B should be allowed");

    // B -> A would close the loop — refused.
    let back = state
        .add_pipe(
            b,
            "tcp_data_received".into(),
            a,
            "send_tcp_data".into(),
            map_of(&[("data", "x")]),
        )
        .await;
    assert!(back.is_err(), "B->A should be refused as a cycle");

    // Self-loop A -> A — refused.
    let self_loop = state
        .add_pipe(
            a,
            "http_request".into(),
            a,
            "send_tcp_data".into(),
            map_of(&[("data", "x")]),
        )
        .await;
    assert!(self_loop.is_err(), "A->A self-loop should be refused");
}

#[tokio::test]
async fn add_pipe_to_missing_target_errors() {
    let state = Arc::new(AppState::new());
    let a = add_placeholder(&state, "http").await;
    let missing = ServerId::new(9999);

    let err = state
        .add_pipe(
            a,
            "http_request".into(),
            missing,
            "send_tcp_data".into(),
            map_of(&[("data", "x")]),
        )
        .await;
    assert!(
        err.is_err(),
        "pipe to a nonexistent target must error cleanly"
    );
}

#[tokio::test]
async fn pipe_removed_when_endpoint_server_closes() {
    let state = Arc::new(AppState::new());
    let a = add_placeholder(&state, "http").await;
    let b = add_placeholder(&state, "tcp").await;
    state
        .add_pipe(
            a,
            "http_request".into(),
            b,
            "send_tcp_data".into(),
            map_of(&[("data", "x")]),
        )
        .await
        .expect("pipe");
    assert_eq!(state.list_pipes().await.len(), 1);

    // Closing the sink tears the pipe down (mirrors scheduled-task scoping).
    state.remove_server(b).await;
    assert_eq!(
        state.list_pipes().await.len(),
        0,
        "pipe should be removed when its sink server closes"
    );
}

#[tokio::test]
async fn create_and_remove_pipe_via_action() {
    // The LLM/operator entry point: a `create_pipe` action creates the wiring,
    // `remove_pipe` tears it down. Exercises the same path the action executor uses.
    let state = Arc::new(AppState::new());
    let a = add_placeholder(&state, "http").await;
    let b = add_placeholder(&state, "tcp").await;

    let create = serde_json::json!({
        "type": "create_pipe",
        "on": "http_request",
        "to": b.as_u32(),
        "as": "send_tcp_data",
        "map": { "data": "{method} {path}\n" }
    });
    // `from` omitted -> defaults to the acting server (A).
    netget::pipe::execute_pipe_action("create_pipe", &create, &state, Some(a))
        .await
        .expect("create_pipe should succeed");

    let pipes = state.list_pipes().await;
    assert_eq!(pipes.len(), 1);
    assert_eq!(pipes[0].from, a.as_u32());
    assert_eq!(pipes[0].to, b.as_u32());

    let remove = serde_json::json!({ "type": "remove_pipe", "pipe_id": pipes[0].id });
    netget::pipe::execute_pipe_action("remove_pipe", &remove, &state, None)
        .await
        .expect("remove_pipe should succeed");
    assert!(state.list_pipes().await.is_empty());
}

// ---------------------------------------------------------------------------
// End-to-end: HTTP access event -> TCP sink
// ---------------------------------------------------------------------------

#[cfg(all(feature = "http", feature = "tcp"))]
#[tokio::test]
async fn http_access_event_pipes_into_tcp_server() {
    use netget::llm::ollama_client::OllamaClient;
    use netget::server::{HttpServer, TcpServer};
    use netget::state::server::ServerStatus;
    use std::time::Duration;
    use tokio::sync::mpsc;

    let state = Arc::new(AppState::new());

    // --- Sink: a real NetGet TCP server (B). Its LLM is unreachable on purpose;
    //     the read loop reports received bytes on its status stream *before* the
    //     LLM call, which is what we assert on. ---
    let tcp_id = add_placeholder(&state, "tcp").await;
    let (tcp_status_tx, mut tcp_status_rx) = mpsc::unbounded_channel::<String>();
    let tcp_addr = TcpServer::spawn_with_llm_actions(
        "127.0.0.1:0".parse().unwrap(),
        OllamaClient::new("http://127.0.0.1:1"),
        state.clone(),
        tcp_status_tx,
        false, // send_first
        tcp_id,
    )
    .await
    .expect("tcp sink should start");
    // The pipe only delivers to a Running, bound sink.
    state.update_server_local_addr(tcp_id, tcp_addr).await;
    state
        .update_server_status(tcp_id, ServerStatus::Running)
        .await;

    // --- Source: a real NetGet HTTP server (A). Dead LLM again — the pipe fires
    //     before/independent of A's own response. ---
    let http_id = add_placeholder(&state, "http").await;
    let (http_status_tx, _http_status_rx) = mpsc::unbounded_channel::<String>();
    let http_addr = HttpServer::spawn_with_llm_actions(
        "127.0.0.1:0".parse().unwrap(),
        OllamaClient::new("http://127.0.0.1:1"),
        state.clone(),
        http_status_tx,
        http_id,
        None, // no TLS
    )
    .await
    .expect("http source should start");
    state.update_server_local_addr(http_id, http_addr).await;
    state
        .update_server_status(http_id, ServerStatus::Running)
        .await;

    // --- The wiring: A's http_request -> B, as a TCP send of a formatted log line. ---
    state
        .add_pipe(
            http_id,
            "http_request".into(),
            tcp_id,
            "send_tcp_data".into(),
            map_of(&[("data", "{method} {path}\n")]),
        )
        .await
        .expect("pipe A->B should be created");

    // --- Trigger: one HTTP request to A. ---
    let url = format!("http://{}/pipe-test", http_addr);
    // A's LLM is dead, so it answers 500 — we don't care about the status, only
    // that the request reached A and raised http_request.
    let _ = reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await;

    // --- Assert: B received the mapped log line over a real TCP socket. ---
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut received = false;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(250), tcp_status_rx.recv()).await {
            Ok(Some(line)) => {
                if line.contains("TCP received") && line.contains("GET /pipe-test") {
                    received = true;
                    break;
                }
            }
            Ok(None) => break, // channel closed
            Err(_) => {}       // idle tick; keep waiting until the deadline
        }
    }

    assert!(
        received,
        "TCP sink #{} never reported receiving the piped 'GET /pipe-test' line",
        tcp_id.as_u32()
    );

    state.remove_server(http_id).await;
    state.remove_server(tcp_id).await;
}
