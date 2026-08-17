//! E2E for the client command channel (`AppState::send_to_client`): a NetGet
//! client connected to a NetGet server of the same protocol, with an action
//! injected from outside the client's loop — the dashboard's [send] path.
//!
//! Zero LLM involvement: the server answers via static handlers and the
//! client's LLM points at an unreachable URL (errors are tolerated by the
//! loops; nothing here depends on a model).
//!
//! Run with:
//!   ./cargo-isolated.sh test --no-default-features --features tcp,telnet --test client_handle_test -- --test-threads=100

#![cfg(feature = "tcp")]

use std::time::Duration;

use netget::cli::management::{ClientForm, ServerForm};
use netget::state::app_state::AppState;
use netget::state::client_handles::ClientSendOutcome;
use netget::state::{AccessLogOwner, ClientId, ServerId};
use tokio::sync::mpsc;

async fn new_state() -> AppState {
    let state = AppState::new_with_options(false, false, "http://127.0.0.1:1".to_string());
    state
        .set_llm_client(netget::llm::OllamaClient::new(
            "http://127.0.0.1:1".to_string(),
        ))
        .await;
    state
}

async fn wait_for_port(state: &AppState, id: ServerId) -> u16 {
    for _ in 0..100 {
        if let Some(s) = state.get_server(id).await {
            if let Some(addr) = s.local_addr {
                return addr.port();
            }
            if s.port != 0 {
                return s.port;
            }
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    panic!("server #{} never bound a port", id.as_u32());
}

async fn wait_for_client_handle(state: &AppState, id: ClientId) {
    for _ in 0..100 {
        if state.has_client_handle(id).await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    panic!("client #{} never registered a command handle", id.as_u32());
}

/// Poll the owner-scoped access log until an entry's serialized form contains
/// `needle`, or panic after ~3s.
async fn wait_for_log_containing(
    state: &AppState,
    owner: AccessLogOwner,
    needle: &str,
) -> netget::state::app_state::AccessLogEntry {
    for _ in 0..100 {
        for entry in state.list_access_logs_for(Some(owner), None).await {
            let text = serde_json::to_string(&entry).unwrap_or_default();
            if text.contains(needle) {
                return entry;
            }
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    panic!("no access-log entry for {owner:?} containing {needle:?}");
}

#[tokio::test]
async fn injected_tcp_action_reaches_our_own_server() {
    let state = new_state().await;
    let (tx, _rx) = mpsc::unbounded_channel();

    // TCP server that statically answers every data event with "pong".
    let server_form = ServerForm {
        protocol: "tcp".to_string(),
        port: Some(0),
        event_handlers: Some(vec![serde_json::json!({
            "event_pattern": "tcp_data_received",
            "handler": {
                "type": "static",
                "actions": [ { "type": "send_tcp_data", "data": "pong" } ]
            }
        })]),
        ..Default::default()
    };
    let server_id = server_form.create(&state, tx.clone()).await.expect("create tcp server");
    let port = wait_for_port(&state, server_id).await;

    // TCP client (dual protocol!) connected to our own server.
    let client_form = ClientForm {
        protocol: "tcp".to_string(),
        remote_addr: Some(format!("127.0.0.1:{port}")),
        instruction: Some("test client".to_string()),
        ..Default::default()
    };
    let client_id = client_form
        .create(
            &state,
            netget::llm::OllamaClient::new("http://127.0.0.1:1".to_string()),
            tx.clone(),
        )
        .await
        .expect("create tcp client");

    wait_for_client_handle(&state, client_id).await;

    // Inject a send through the running client — the dashboard's [send] path.
    let outcome = state
        .send_to_client(
            client_id,
            serde_json::json!({"type": "send_tcp_data", "data": "ping-from-ui"}),
            Duration::from_secs(5),
        )
        .await
        .expect("send_to_client");
    match outcome {
        ClientSendOutcome::Sent { bytes_sent } => assert_eq!(bytes_sent, "ping-from-ui".len()),
        other => panic!("expected Sent, got {other:?}"),
    }

    // The server saw the bytes: its static handler ran and the access log
    // recorded the event (printable payloads appear as text on the tcp
    // server side; only non-printable data is hex-encoded).
    let server_entry = wait_for_log_containing(
        &state,
        AccessLogOwner::Server(server_id.as_u32()),
        "ping-from-ui",
    )
    .await;
    assert_eq!(server_entry.server_id, Some(server_id.as_u32()));

    // The injection itself is in the client's request log.
    let client_entry = wait_for_log_containing(
        &state,
        AccessLogOwner::Client(client_id.as_u32()),
        "injected_action",
    )
    .await;
    assert_eq!(client_entry.client_id, Some(client_id.as_u32()));
    assert_eq!(client_entry.event_type, "injected_action");

    // A malformed action is rejected by the protocol, not swallowed.
    let outcome = state
        .send_to_client(
            client_id,
            serde_json::json!({"type": "no_such_action"}),
            Duration::from_secs(5),
        )
        .await
        .expect("send_to_client (bad action)");
    assert!(
        matches!(outcome, ClientSendOutcome::Rejected { .. }),
        "expected Rejected, got {outcome:?}"
    );

    // Disconnect via injected action: loop breaks, handle is dropped, and a
    // later send fails fast with a clear error instead of hanging.
    let outcome = state
        .send_to_client(
            client_id,
            serde_json::json!({"type": "disconnect"}),
            Duration::from_secs(5),
        )
        .await
        .expect("send_to_client (disconnect)");
    assert!(matches!(outcome, ClientSendOutcome::Disconnected));

    for _ in 0..100 {
        if !state.has_client_handle(client_id).await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let err = state
        .send_to_client(
            client_id,
            serde_json::json!({"type": "send_tcp_data", "data": "late"}),
            Duration::from_secs(1),
        )
        .await
        .expect_err("send after disconnect must error");
    assert!(
        err.to_string().contains("does not accept injected commands")
            || err.to_string().contains("not running"),
        "unexpected error: {err}"
    );
}

/// A client id that never existed (or a protocol that has not adopted the
/// channel) fails cleanly and immediately.
#[tokio::test]
async fn send_to_unknown_client_errors_cleanly() {
    let state = new_state().await;
    let err = state
        .send_to_client(
            ClientId::new(4242),
            serde_json::json!({"type": "send_tcp_data", "data": "x"}),
            Duration::from_secs(1),
        )
        .await
        .expect_err("must error");
    assert!(err.to_string().contains("does not accept injected commands"));
}

#[cfg(feature = "telnet")]
#[tokio::test]
async fn injected_telnet_command_reaches_our_own_server() {
    let state = new_state().await;
    let (tx, _rx) = mpsc::unbounded_channel();

    // Telnet server with a wildcard static handler that answers nothing —
    // enough to exercise dispatch and access logging without a model.
    let server_form = ServerForm {
        protocol: "telnet".to_string(),
        port: Some(0),
        event_handlers: Some(vec![serde_json::json!({
            "event_pattern": "*",
            "handler": { "type": "static", "actions": [] }
        })]),
        ..Default::default()
    };
    let server_id = server_form
        .create(&state, tx.clone())
        .await
        .expect("create telnet server");
    let port = wait_for_port(&state, server_id).await;

    let client_form = ClientForm {
        protocol: "telnet".to_string(),
        remote_addr: Some(format!("127.0.0.1:{port}")),
        instruction: Some("test client".to_string()),
        ..Default::default()
    };
    let client_id = client_form
        .create(
            &state,
            netget::llm::OllamaClient::new("http://127.0.0.1:1".to_string()),
            tx.clone(),
        )
        .await
        .expect("create telnet client");

    wait_for_client_handle(&state, client_id).await;

    let outcome = state
        .send_to_client(
            client_id,
            serde_json::json!({"type": "send_command", "command": "hello-dashboard"}),
            Duration::from_secs(5),
        )
        .await
        .expect("send_to_client");
    assert!(
        matches!(outcome, ClientSendOutcome::Sent { .. }),
        "expected Sent, got {outcome:?}"
    );

    // The server-side event carries the command text.
    wait_for_log_containing(
        &state,
        AccessLogOwner::Server(server_id.as_u32()),
        "hello-dashboard",
    )
    .await;
}
