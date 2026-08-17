//! The rail's tree model: what nests under what, how long lists are capped,
//! and what expanding a request reveals.

#![cfg(feature = "tcp")]

use netget::state::app_state::AccessLogEntry;
use netget::state::client::ClientStatus;
use netget::state::server::ServerStatus;
use netget::state::{ClientId, ServerId};
use netget::tui::app::UiKey;
use netget::tui::projection::{ClientRow, ConnRow, SendState, ServerRow};
use netget::tui::tree::{self, NodeId, TreeState, CHILD_LIMIT};

fn entry(id: u64, conn: Option<u32>) -> AccessLogEntry {
    AccessLogEntry {
        id,
        unix_ms: 1_700_000_000_000 + id,
        server_id: Some(1),
        client_id: None,
        protocol: "TCP".into(),
        connection_id: conn,
        event_type: "tcp_data_received".into(),
        request: serde_json::json!({"data": format!("payload-{id}")}),
        response: vec![serde_json::json!({"type": "send_tcp_data", "data": "pong"})],
    }
}

fn conn(id: u32) -> ConnRow {
    ConnRow {
        id,
        remote_addr: format!("127.0.0.1:{}", 40000 + id),
        bytes_received: 10,
        bytes_sent: 20,
        active: true,
    }
}

fn server(conns: Vec<ConnRow>, requests: Vec<AccessLogEntry>) -> ServerRow {
    ServerRow {
        id: ServerId::new(1),
        protocol: "TCP".into(),
        port: 8080,
        local_addr: Some("127.0.0.1:8080".into()),
        status: ServerStatus::Running,
        instruction: "be a server".into(),
        memory_len: 0,
        startup_params: None,
        routing: None,
        conns,
        recent: Vec::new(),
        requests,
        task_count: 0,
        client_counterpart: Some("TCP".into()),
    }
}

/// Rows carrying a given depth, for asserting structure without pinning exact
/// label text.
fn labels_at_depth(rows: &[tree::TreeRow], depth: u16) -> Vec<String> {
    rows.iter()
        .filter(|r| r.depth == depth)
        .map(|r| r.label.clone())
        .collect()
}

/// The rows belonging to one group: everything after it that is deeper, up to
/// the next row at the same or shallower depth. Depth alone is ambiguous —
/// config items, routes and peers are all depth 2 — so subtree scoping is what
/// makes these assertions mean what they say.
fn subtree<'a>(rows: &'a [tree::TreeRow], group_label_prefix: &str) -> &'a [tree::TreeRow] {
    let start = rows
        .iter()
        .position(|r| r.label.starts_with(group_label_prefix))
        .unwrap_or_else(|| panic!("no group starting with {group_label_prefix:?}"));
    let depth = rows[start].depth;
    let end = rows[start + 1..]
        .iter()
        .position(|r| r.depth <= depth)
        .map(|i| start + 1 + i)
        .unwrap_or(rows.len());
    &rows[start + 1..end]
}

#[test]
fn an_instance_is_the_root_with_config_routing_and_peers_beneath() {
    let row = server(vec![conn(1)], vec![]);
    let rows = tree::server_rows(&row, &TreeState::default());

    // Exactly one root, and it is the instance.
    let roots = labels_at_depth(&rows, 0);
    assert_eq!(roots.len(), 1);
    assert!(roots[0].contains("#1 TCP"), "root was {:?}", roots[0]);

    // The three groups sit one level in.
    let groups = labels_at_depth(&rows, 1);
    assert!(groups.iter().any(|l| l.starts_with("config")), "{groups:?}");
    assert!(groups.iter().any(|l| l.starts_with("routing")), "{groups:?}");
    assert!(groups.iter().any(|l| l.starts_with("peers")), "{groups:?}");

    // Every group row is expandable.
    for row in rows.iter().filter(|r| r.depth == 1) {
        assert!(
            row.expanded.is_some(),
            "group {:?} should be expandable",
            row.label
        );
    }
}

#[test]
fn requests_nest_under_the_peer_that_carried_them() {
    let requests = vec![entry(1, Some(1)), entry(2, Some(2)), entry(3, Some(1))];
    let row = server(vec![conn(1), conn(2)], requests);
    let rows = tree::server_rows(&row, &TreeState::default());

    // Peers live under the peers group; their requests one level deeper.
    let under_peers = subtree(&rows, "peers");
    let peers: Vec<String> = under_peers
        .iter()
        .filter(|r| r.depth == 2)
        .map(|r| r.label.clone())
        .collect();
    assert_eq!(peers.len(), 2, "two connections: {peers:?}");
    assert!(peers[0].contains("· 2 req"), "peer 1 has two: {peers:?}");
    assert!(peers[1].contains("· 1 req"), "peer 2 has one: {peers:?}");

    // The request rows under the first peer are its own, not peer 2's.
    let under_first: Vec<&str> = subtree(under_peers, "127.0.0.1:40001")
        .iter()
        .map(|r| r.label.as_str())
        .collect();
    assert_eq!(under_first.len(), 2, "{under_first:?}");
    assert!(under_first.iter().all(|l| l.starts_with('#')));
}

#[test]
fn a_connectionless_request_gets_its_own_bucket() {
    // UDP/DNS-style events carry no connection id and would otherwise vanish.
    let row = server(vec![], vec![entry(7, None)]);
    let rows = tree::server_rows(&row, &TreeState::default());
    assert!(
        rows.iter().any(|r| r.label.contains("(connectionless)")),
        "expected a connectionless bucket: {:?}",
        rows.iter().map(|r| &r.label).collect::<Vec<_>>()
    );
}

#[test]
fn a_long_list_is_capped_and_the_cap_can_be_lifted() {
    let requests: Vec<AccessLogEntry> = (1..=(CHILD_LIMIT as u64 + 4))
        .map(|id| entry(id, Some(1)))
        .collect();
    let total = requests.len();
    let row = server(vec![conn(1)], requests);

    let mut state = TreeState::default();
    let rows = tree::server_rows(&row, &state);
    let shown = rows.iter().filter(|r| r.depth == 3).count();
    assert_eq!(shown, CHILD_LIMIT + 1, "capped list plus the '… more' row");
    assert!(
        rows.iter()
            .filter(|r| matches!(r.node, NodeId::Request(_, _)))
            .count()
            == CHILD_LIMIT,
        "the cap counts requests"
    );
    assert!(
        rows.iter().any(|r| r.label.starts_with("… ")),
        "a capped group must say how much is hidden"
    );

    // Lifting the cap on that peer reveals the rest.
    let peer = NodeId::Peer(UiKey::Server(ServerId::new(1)), Some(1));
    state.show_all(&peer);
    let rows = tree::server_rows(&row, &state);
    assert_eq!(rows.iter().filter(|r| r.depth == 3).count(), total);
    assert!(!rows.iter().any(|r| r.label.starts_with("… ")));
}

#[test]
fn collapsing_a_group_hides_its_children() {
    let row = server(vec![conn(1)], vec![entry(1, Some(1))]);
    let key = UiKey::Server(ServerId::new(1));

    let mut state = TreeState::default();
    let expanded = tree::server_rows(&row, &state).len();

    state.collapse(&NodeId::Peers(key));
    let collapsed = tree::server_rows(&row, &state);
    assert!(
        collapsed.len() < expanded,
        "collapsing peers should hide rows ({} vs {})",
        collapsed.len(),
        expanded
    );
    assert!(
        subtree(&collapsed, "peers").is_empty(),
        "nothing below the collapsed peers group should render"
    );

    // Collapsing the instance itself leaves only the root.
    state.collapse(&NodeId::Instance(key));
    assert_eq!(tree::server_rows(&row, &state).len(), 1);
}

#[test]
fn a_request_expands_in_place_to_show_its_detail() {
    let row = server(vec![conn(1)], vec![entry(42, Some(1))]);
    let key = UiKey::Server(ServerId::new(1));
    let mut state = TreeState::default();

    // Requests start closed: a busy peer should read as a list.
    let closed = tree::server_rows(&row, &state);
    let request_row = closed
        .iter()
        .find(|r| matches!(r.node, NodeId::Request(_, 42)))
        .expect("the request is listed");
    assert_eq!(request_row.expanded, Some(false));
    assert!(
        !closed.iter().any(|r| r.depth > 3),
        "nothing below the request until it is opened"
    );

    // Opening it adds its request/response detail one level deeper.
    state.toggle(&NodeId::Request(key, 42));
    let open = tree::server_rows(&row, &state);
    let detail: Vec<&str> = open
        .iter()
        .filter(|r| r.depth == 4)
        .map(|r| r.label.as_str())
        .collect();
    assert!(!detail.is_empty(), "expanding a request must reveal detail");
    assert!(
        detail.iter().any(|l| l.contains("payload-42")),
        "the detail should include the request payload: {detail:?}"
    );
    assert!(
        detail.iter().any(|l| l.contains("send_tcp_data")),
        "and the response actions: {detail:?}"
    );
}

/// Opening a request must not consume the peer's request budget: its detail is
/// nested inside it, not a sibling of the other requests.
#[test]
fn expanding_a_request_does_not_truncate_the_request_list() {
    let requests: Vec<AccessLogEntry> = (1..=CHILD_LIMIT as u64)
        .map(|id| entry(id, Some(1)))
        .collect();
    let row = server(vec![conn(1)], requests);
    let key = UiKey::Server(ServerId::new(1));

    let mut state = TreeState::default();
    let before = tree::server_rows(&row, &state)
        .iter()
        .filter(|r| matches!(r.node, NodeId::Request(_, _)))
        .count();
    assert_eq!(before, CHILD_LIMIT);

    state.toggle(&NodeId::Request(key, 1));
    let after = tree::server_rows(&row, &state)
        .iter()
        .filter(|r| matches!(r.node, NodeId::Request(_, _)))
        .count();
    assert_eq!(
        after, CHILD_LIMIT,
        "every request should still be listed once one of them is expanded"
    );
}

#[test]
fn a_client_tree_groups_its_peer_requests_and_attempts() {
    let row = ClientRow {
        id: ClientId::new(2),
        protocol: "TCP".into(),
        remote_addr: "127.0.0.1:8080".into(),
        status: ClientStatus::Connected,
        instruction: "be a client".into(),
        memory_len: 0,
        startup_params: None,
        routing: None,
        connection: None,
        history: vec![netget::state::client::ClientConnectionAttempt {
            remote_addr: "127.0.0.1:8080".into(),
            started_unix_ms: 1_700_000_000_000,
            ended_unix_ms: None,
            outcome: "connected".into(),
        }],
        requests: vec![entry(5, None)],
        task_count: 0,
        send_state: SendState::Ready,
    };
    let rows = tree::client_rows(&row, &TreeState::default());

    let groups = labels_at_depth(&rows, 1);
    assert!(groups.iter().any(|l| l.starts_with("config")), "{groups:?}");
    assert!(groups.iter().any(|l| l.starts_with("routing")), "{groups:?}");
    assert!(groups.iter().any(|l| l.starts_with("peer")), "{groups:?}");
    // Attempts are a sibling of peer, not one of its children.
    assert!(
        groups.iter().any(|l| l.starts_with("attempts")),
        "attempts should be their own group: {groups:?}"
    );

    // The root carries the send affordance.
    assert!(
        rows[0]
            .button
            .as_ref()
            .is_some_and(|(label, _)| label.contains("send")),
        "the client root should offer [ send ]"
    );
}
