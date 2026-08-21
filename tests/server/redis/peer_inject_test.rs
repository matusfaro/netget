//! The dashboard's "message this peer" / "disconnect this peer" path on a Redis server
//! connection: `AppState::send_to_peer` injects an action into one live connection. Zero LLM
//! calls - the server answers through a `*` static handler.
//!
//! Two things are pinned here:
//! - `close_connection` (what "disconnect this peer" injects) half-closes the socket from
//!   outside the connection task, the handle is removed, and the connection's counters were
//!   live.
//! - Redis's reply verbs (`redis_simple_string` etc.) return `ActionResult::Custom`, which the
//!   generic peer task reports as `Executed` WITHOUT writing anything. That gap is deliberate
//!   (see `src/server/redis/CLAUDE.md`); if it is ever closed, flip the assertion to `Sent`.
//!
//! Run with:
//!   ./cargo-isolated.sh test --no-default-features --features redis --test server -- redis::peer_inject --test-threads=100

#![cfg(feature = "redis")]

use std::time::Duration;

use netget::cli::management::ServerForm;
use netget::state::app_state::AppState;
use netget::state::client_handles::ClientSendOutcome;
use netget::state::ServerId;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
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
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    panic!("Redis server #{} never bound a port", id.as_u32());
}

/// The first connection that has a peer handle registered.
async fn wait_for_peer_handle(state: &AppState, id: ServerId) -> u32 {
    for _ in 0..100 {
        if let Some(s) = state.get_server(id).await {
            for conn in s.connections.values() {
                if state.has_peer_handle(id, conn.id.as_u32()).await {
                    return conn.id.as_u32();
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    panic!(
        "Redis server #{} never registered a peer handle",
        id.as_u32()
    );
}

#[tokio::test]
async fn injected_redis_close_sends_eof_and_reply_verbs_report_the_custom_gap() {
    let state = new_state().await;
    let (tx, _rx) = mpsc::unbounded_channel();

    let server_id = ServerForm {
        protocol: "redis".to_string(),
        port: Some(0),
        event_handlers: Some(vec![serde_json::json!({
            "event_pattern": "*",
            "handler": {
                "type": "static",
                "actions": [ { "type": "redis_simple_string", "value": "PONG" } ]
            }
        })]),
        ..Default::default()
    }
    .create(&state, tx.clone())
    .await
    .expect("create redis server");
    let port = wait_for_port(&state, server_id).await;

    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .expect("connect");

    // The handle exists before the peer has said anything.
    let conn = wait_for_peer_handle(&state, server_id).await;

    // A real command round-trips through the static handler, so the counters move.
    stream
        .write_all(b"*1\r\n$4\r\nPING\r\n")
        .await
        .expect("write PING");
    let mut buf = [0u8; 64];
    let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf))
        .await
        .expect("PONG within 5s")
        .expect("read PONG");
    assert_eq!(&buf[..n], b"+PONG\r\n");

    let server = state.get_server(server_id).await.expect("server");
    let conn_state = server
        .connections
        .values()
        .find(|c| c.id.as_u32() == conn)
        .expect("connection tracked");
    assert_eq!(conn_state.bytes_received, 14, "PING frame counted as read");
    assert_eq!(conn_state.bytes_sent, 7, "PONG reply counted as write");
    assert_eq!(conn_state.packets_received, 1);
    assert_eq!(conn_state.packets_sent, 1);

    // The Custom-result gap: the reply verb is accepted and executed, but the generic peer
    // task has no bytes to write, so nothing reaches the socket.
    let outcome = state
        .send_to_peer(
            server_id,
            conn,
            serde_json::json!({"type": "redis_simple_string", "value": "injected"}),
            Duration::from_secs(5),
        )
        .await
        .expect("send_to_peer reply verb");
    assert!(
        matches!(outcome, ClientSendOutcome::Executed { .. }),
        "redis reply verbs return ActionResult::Custom; expected Executed, got {outcome:?}"
    );

    // "disconnect this peer" injects the generic `close_connection` (not the model-facing
    // `close_this_connection`): half-close from outside, the socket reads EOF.
    let outcome = state
        .send_to_peer(
            server_id,
            conn,
            serde_json::json!({"type": "close_connection"}),
            Duration::from_secs(5),
        )
        .await
        .expect("send_to_peer close");
    assert!(
        matches!(outcome, ClientSendOutcome::Disconnected),
        "expected Disconnected, got {outcome:?}"
    );

    let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf))
        .await
        .expect("EOF within 5s")
        .expect("read after close");
    assert_eq!(n, 0, "expected EOF after close_connection");

    // The handle goes away with the connection.
    for _ in 0..100 {
        if !state.has_peer_handle(server_id, conn).await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    panic!("peer handle still registered after the connection closed");
}
