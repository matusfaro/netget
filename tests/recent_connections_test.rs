//! Recent-connection history: closed server connections leave a
//! `ClosedConnectionSummary` behind, and client status transitions build a
//! `connection_history` — the data behind the dashboard's "recently
//! connected" panes.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Instant;

use netget::server::connection::ConnectionId;
use netget::state::client::{ClientId, ClientInstance, ClientStatus};
use netget::state::server::{
    ConnectionState, ConnectionStatus, ProtocolConnectionInfo, ServerId, ServerInstance,
    RECENT_CONNECTION_CAPACITY,
};
use netget::state::AppState;

fn test_connection(id: u32) -> ConnectionState {
    let addr: SocketAddr = format!("127.0.0.1:{}", 40000 + id).parse().unwrap();
    ConnectionState {
        id: ConnectionId::new(id),
        remote_addr: addr,
        local_addr: "127.0.0.1:9999".parse().unwrap(),
        bytes_sent: 10,
        bytes_received: 20,
        packets_sent: 1,
        packets_received: 2,
        last_activity: Instant::now(),
        status: ConnectionStatus::Active,
        status_changed_at: Instant::now(),
        protocol_info: ProtocolConnectionInfo::new(serde_json::json!({"kind": "test"})),
    }
}

#[test]
fn removed_connection_is_summarized() {
    let mut server = ServerInstance::new(ServerId::new(1), 9999, "TCP".into(), "test".into());
    server.add_connection(test_connection(7));
    assert!(server.recent_connections.is_empty());

    let removed = server.remove_connection(ConnectionId::new(7));
    assert!(removed.is_some());

    assert_eq!(server.recent_connections.len(), 1);
    let summary = &server.recent_connections[0];
    assert_eq!(summary.id, 7);
    assert_eq!(summary.remote_addr, "127.0.0.1:40007");
    assert_eq!(summary.bytes_sent, 10);
    assert_eq!(summary.bytes_received, 20);
    // Opened through add_connection, so the open time must be known and sane.
    let opened = summary.opened_unix_ms.expect("opened_unix_ms recorded");
    assert!(
        opened <= summary.closed_unix_ms + 1000,
        "open must not postdate close"
    );
    // A wall-clock timestamp from this decade, not 0 or a duration-since-start.
    assert!(summary.closed_unix_ms > 1_500_000_000_000);
}

#[test]
fn history_is_newest_first_and_capped() {
    let mut server = ServerInstance::new(ServerId::new(1), 9999, "TCP".into(), "test".into());
    for id in 0..(RECENT_CONNECTION_CAPACITY as u32 + 10) {
        server.add_connection(test_connection(id));
        server.remove_connection(ConnectionId::new(id));
    }
    assert_eq!(server.recent_connections.len(), RECENT_CONNECTION_CAPACITY);
    // Newest first: the last-closed connection is at the front.
    assert_eq!(
        server.recent_connections[0].id,
        RECENT_CONNECTION_CAPACITY as u32 + 9
    );
    // The oldest entries fell off the back.
    assert!(server.recent_connections.iter().all(|s| s.id >= 10));
}

#[test]
fn stale_connectionless_entries_are_summarized_too() {
    let mut server = ServerInstance::new(ServerId::new(1), 9999, "UDP".into(), "test".into());
    server.add_connection(test_connection(3));
    // max_age 0 ⇒ everything is stale.
    server.cleanup_old_connections(0);
    assert!(server.connections.is_empty());
    assert_eq!(server.recent_connections.len(), 1);
    assert_eq!(server.recent_connections[0].id, 3);
}

#[tokio::test]
async fn app_state_reaper_records_and_accessor_returns() {
    let state = AppState::new();
    let server = ServerInstance::new(ServerId::new(0), 9999, "TCP".into(), "test".into());
    let server_id = state.add_server(server).await;

    let mut conn = test_connection(1);
    conn.status = ConnectionStatus::Closed;
    conn.status_changed_at = Instant::now();
    state.add_connection_to_server(server_id, conn).await;

    // Reap closed connections immediately (max_age 0).
    state.cleanup_closed_connections(0).await;

    let recent = state.get_recent_connections(server_id).await;
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].id, 1);

    // Unknown server: empty, not a panic.
    assert!(state
        .get_recent_connections(ServerId::new(999))
        .await
        .is_empty());
}

#[test]
fn client_status_transitions_build_history() {
    let mut client = ClientInstance::new(
        ClientId::new(1),
        "127.0.0.1:2323".into(),
        "Telnet".into(),
        "test".into(),
    );
    assert!(client.connection_history.is_empty());

    client.record_status_transition(&ClientStatus::Connecting);
    assert_eq!(client.connection_history.len(), 1);
    assert_eq!(client.connection_history[0].outcome, "connecting");
    assert!(client.connection_history[0].ended_unix_ms.is_none());

    client.record_status_transition(&ClientStatus::Connected);
    assert_eq!(client.connection_history.len(), 1);
    assert_eq!(client.connection_history[0].outcome, "connected");
    assert!(client.connection_history[0].ended_unix_ms.is_none());

    client.record_status_transition(&ClientStatus::Disconnected);
    assert_eq!(client.connection_history[0].outcome, "disconnected");
    assert!(client.connection_history[0].ended_unix_ms.is_some());

    // A reconnect opens a second attempt; an error closes it.
    client.record_status_transition(&ClientStatus::Connecting);
    client.record_status_transition(&ClientStatus::Error("refused".into()));
    assert_eq!(client.connection_history.len(), 2);
    assert_eq!(client.connection_history[1].outcome, "error: refused");
    assert!(client.connection_history[1].ended_unix_ms.is_some());
}

#[tokio::test]
async fn update_client_status_records_history() {
    let state = AppState::new();
    let client = ClientInstance::new(
        ClientId::new(0),
        "127.0.0.1:2323".into(),
        "Telnet".into(),
        "test".into(),
    );
    let client_id = state.add_client(client).await;

    state
        .update_client_status(client_id, ClientStatus::Connecting)
        .await;
    state
        .update_client_status(client_id, ClientStatus::Connected)
        .await;
    state
        .update_client_status(client_id, ClientStatus::Disconnected)
        .await;

    let clients: HashMap<_, _> = state
        .get_all_clients()
        .await
        .into_iter()
        .map(|c| (c.id, c))
        .collect();
    let history = &clients[&client_id].connection_history;
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].outcome, "disconnected");
    assert!(history[0].ended_unix_ms.is_some());
}
