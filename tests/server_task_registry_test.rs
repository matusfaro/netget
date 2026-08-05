//! Regression test: a server may own more than one background task, and stopping
//! it must abort every one of them.
//!
//! `register_server_task()` used to store a single `JoinHandle` per server, so a
//! protocol with two long-lived loops (WireGuard's UDP listener plus its TUN
//! reader) silently leaked the first — `remove_server` then aborted only the last.
//! OpenVPN worked around it by fusing both loops into one task with `select!`.
//! Handles now accumulate in a `Vec`, as they already did for clients.
//!
//! No protocol feature, no network peer and no Ollama are needed: the tasks under
//! test are plain listeners, and a held port is the observable proof that a task
//! is still alive.
//!
//! Run with:
//!   ./cargo-isolated.sh test --no-default-features --test server_task_registry_test -- --test-threads=100

use netget::state::app_state::AppState;
use netget::state::server::ServerInstance;
use netget::state::ServerId;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

/// True if a plain TCP bind on `addr` succeeds — i.e. the port is free.
fn port_is_free(addr: SocketAddr) -> bool {
    std::net::TcpListener::bind(addr).is_ok()
}

/// Poll (up to ~1s) for the port to become free after an async abort.
async fn wait_until_free(addr: SocketAddr) -> bool {
    for _ in 0..50 {
        if port_is_free(addr) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

async fn add_placeholder(state: &AppState) -> ServerId {
    let server = ServerInstance::new(ServerId::new(0), 0, "test".to_string(), String::new());
    state.add_server(server).await
}

/// Spawn a task that binds an ephemeral port and holds it until aborted.
/// Returns the bound address and the task handle.
async fn spawn_listener() -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        loop {
            // Holding `listener` across the await is the point: the port stays
            // bound for as long as this task lives.
            let _ = listener.accept().await;
        }
    });
    (addr, handle)
}

#[tokio::test]
async fn every_registered_server_task_is_aborted_on_stop() {
    let state = Arc::new(AppState::new());
    let server_id = add_placeholder(&state).await;

    let (addr_a, handle_a) = spawn_listener().await;
    let (addr_b, handle_b) = spawn_listener().await;

    state.register_server_task(server_id, handle_a).await;
    state.register_server_task(server_id, handle_b).await;

    assert_eq!(
        state.server_task_count(server_id).await,
        2,
        "registering a second task must not evict the first"
    );

    assert!(!port_is_free(addr_a), "task A should be holding its port");
    assert!(!port_is_free(addr_b), "task B should be holding its port");

    assert!(state.remove_server(server_id).await.is_some());

    assert!(
        wait_until_free(addr_a).await,
        "port {} was not released — the FIRST registered task leaked",
        addr_a.port()
    );
    assert!(
        wait_until_free(addr_b).await,
        "port {} was not released — the second registered task leaked",
        addr_b.port()
    );
    assert_eq!(state.server_task_count(server_id).await, 0);
}

#[tokio::test]
async fn finished_server_tasks_are_pruned_on_registration() {
    let state = Arc::new(AppState::new());
    let server_id = add_placeholder(&state).await;

    // A task that completes immediately, registered many times over, must not grow
    // the registry without bound — protocols that register a task per connection
    // would otherwise accumulate handles for the life of the server.
    for _ in 0..20 {
        let done = tokio::spawn(async {});
        // Let it finish so `is_finished()` is observable at the next registration.
        tokio::time::sleep(Duration::from_millis(5)).await;
        state.register_server_task(server_id, done).await;
    }

    assert!(
        state.server_task_count(server_id).await <= 2,
        "finished handles should be pruned, found {}",
        state.server_task_count(server_id).await
    );

    state.remove_server(server_id).await;
}

#[tokio::test]
async fn registering_against_a_stopped_server_aborts_immediately() {
    let state = Arc::new(AppState::new());
    let server_id = add_placeholder(&state).await;

    let (addr, handle) = spawn_listener().await;
    assert!(state.remove_server(server_id).await.is_some());

    // Races with stop: the spawn completed after the server was already gone.
    state.register_server_task(server_id, handle).await;

    assert!(
        wait_until_free(addr).await,
        "port {} leaked — a task registered after stop was never aborted",
        addr.port()
    );
    assert_eq!(state.server_task_count(server_id).await, 0);
}
