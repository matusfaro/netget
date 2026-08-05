//! Tests for the live server-instance registry.
//!
//! Protocol actions are dispatched on the *stateless* protocol struct from the
//! registry, which owns no channel, socket or peer table. Sync actions are fine —
//! they are answers carried back to the connection task that raised the event — but
//! async actions (`list_peers`, `get_server_info`, …) have nothing to read from, so
//! the only honest thing they could return was `NoAction`. Several protocols
//! advertised such actions to the model and quietly did nothing.
//!
//! `AppState::register_server_handle` / `server_handle` are the missing half: a
//! running server publishes a handle to its live state, and the action path looks it
//! up by `ServerId`. These tests pin the registry's contract, including the two
//! failure modes that must not panic (wrong type, dead server) and the lifetime rule
//! (a handle never outlives its server).
//!
//! Run with:
//!   ./cargo-isolated.sh test --no-default-features --test server_handle_registry_test -- --test-threads=100

use netget::state::app_state::AppState;
use netget::state::server::{ServerInstance, ServerStatus};
use netget::state::ServerId;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Stand-in for a protocol's live handle: the kind of thing a real one holds is an
/// `mpsc::Sender` plus a shared view of peer state.
struct FakePeerTable {
    peers: Vec<String>,
    queries: AtomicUsize,
}

impl FakePeerTable {
    fn list_peers(&self) -> Vec<String> {
        self.queries.fetch_add(1, Ordering::SeqCst);
        self.peers.clone()
    }
}

/// A different protocol's handle type, to prove downcasting is checked.
struct OtherHandle;

async fn add_server(state: &AppState) -> ServerId {
    let mut server = ServerInstance::new(
        ServerId::new(0),
        0,
        "test".to_string(),
        "Do nothing".to_string(),
    );
    server.status = ServerStatus::Running;
    state.add_server(server).await
}

#[tokio::test]
async fn a_registered_handle_is_reachable_by_server_id() {
    let state = AppState::new();
    let server_id = add_server(&state).await;

    let handle = Arc::new(FakePeerTable {
        peers: vec!["alice".into(), "bob".into()],
        queries: AtomicUsize::new(0),
    });
    state
        .register_server_handle(server_id, handle.clone())
        .await;

    assert!(state.has_server_handle(server_id).await);

    // This is the action path: nothing but the AppState and the ServerId, which is
    // exactly what `executor::execute_actions` already holds.
    let live = state
        .server_handle::<FakePeerTable>(server_id)
        .await
        .expect("the running instance must be reachable from the action path");
    assert_eq!(live.list_peers(), vec!["alice".to_string(), "bob".into()]);

    // The same instance, not a copy: the async action observes real live state.
    assert_eq!(handle.queries.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn asking_for_the_wrong_handle_type_is_a_miss_not_a_panic() {
    let state = AppState::new();
    let server_id = add_server(&state).await;

    state
        .register_server_handle(server_id, Arc::new(OtherHandle))
        .await;

    assert!(state.has_server_handle(server_id).await);
    assert!(
        state
            .server_handle::<FakePeerTable>(server_id)
            .await
            .is_none(),
        "a protocol asking for another protocol's handle must get None"
    );
}

#[tokio::test]
async fn a_handle_never_outlives_its_server() {
    let state = AppState::new();
    let server_id = add_server(&state).await;

    state
        .register_server_handle(
            server_id,
            Arc::new(FakePeerTable {
                peers: Vec::new(),
                queries: AtomicUsize::new(0),
            }),
        )
        .await;

    assert!(state.remove_server(server_id).await.is_some());

    assert!(!state.has_server_handle(server_id).await);
    assert!(state
        .server_handle::<FakePeerTable>(server_id)
        .await
        .is_none());
}

#[tokio::test]
async fn the_reaper_drops_handles_too() {
    let state = AppState::new();
    let server_id = add_server(&state).await;
    state
        .register_server_handle(
            server_id,
            Arc::new(FakePeerTable {
                peers: Vec::new(),
                queries: AtomicUsize::new(0),
            }),
        )
        .await;

    state
        .update_server_status(server_id, ServerStatus::Stopped)
        .await;
    state.cleanup_old_servers(0).await;

    assert!(!state.has_server_handle(server_id).await);
}

#[tokio::test]
async fn registering_against_an_unknown_server_is_a_no_op() {
    let state = AppState::new();
    let ghost = ServerId::new(4242);

    state
        .register_server_handle(
            ghost,
            Arc::new(FakePeerTable {
                peers: Vec::new(),
                queries: AtomicUsize::new(0),
            }),
        )
        .await;

    assert!(
        !state.has_server_handle(ghost).await,
        "a spawn racing a stop must not resurrect an entry for a dead server"
    );
}

#[tokio::test]
async fn handles_are_per_server() {
    let state = AppState::new();
    let a = add_server(&state).await;
    let b = add_server(&state).await;

    state
        .register_server_handle(
            a,
            Arc::new(FakePeerTable {
                peers: vec!["only-a".into()],
                queries: AtomicUsize::new(0),
            }),
        )
        .await;

    assert!(state.server_handle::<FakePeerTable>(a).await.is_some());
    assert!(state.server_handle::<FakePeerTable>(b).await.is_none());
}
