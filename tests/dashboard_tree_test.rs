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
        can_message: false,
    }
}

/// A default tree state with the collapsed-by-default groups opened, for
/// tests that assert on config/handler contents.
fn all_open(key: UiKey) -> TreeState {
    let mut state = TreeState::default();
    state.expand(&NodeId::Config(key));
    state.expand(&NodeId::Routing(key));
    state
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
        intercepts: Vec::new(),
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

    // The three groups sit one level in. Config and handlers start COLLAPSED —
    // they are settings; peers (the traffic) start open.
    let groups = labels_at_depth(&rows, 1);
    assert!(groups.iter().any(|l| l.starts_with("config")), "{groups:?}");
    assert!(
        groups.iter().any(|l| l.starts_with("handlers")),
        "{groups:?}"
    );
    assert!(groups.iter().any(|l| l.starts_with("peers")), "{groups:?}");
    assert!(
        subtree(&rows, "config").is_empty() && subtree(&rows, "handlers").is_empty(),
        "config and handlers default to collapsed"
    );
    assert!(
        !subtree(&rows, "peers").is_empty(),
        "peers default to expanded"
    );

    // Every group row is expandable. Action rows sit at this depth too and are
    // leaves by design — `[ stop server ]` has nothing to expand.
    for row in rows
        .iter()
        .filter(|r| r.depth == 1 && !matches!(r.node, NodeId::Action(..)))
    {
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
        .filter(|r| r.depth == 2 && !matches!(r.node, NodeId::Action(..)))
        .map(|r| r.label.clone())
        .collect();
    assert_eq!(peers.len(), 2, "two connections: {peers:?}");
    assert!(peers[0].contains("· 2 req"), "peer 1 has two: {peers:?}");
    assert!(peers[1].contains("· 1 req"), "peer 2 has one: {peers:?}");

    // The request rows under the first peer are its own, not peer 2's. (The
    // peer's own verb rows — message / disconnect, or the note that the
    // protocol offers neither — sit there too and are not requests.)
    let under_first: Vec<&str> = subtree(under_peers, "127.0.0.1:40001")
        .iter()
        .filter(|r| !matches!(r.node, NodeId::Action(..)))
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
    let shown = rows
        .iter()
        .filter(|r| r.depth == 3 && !matches!(r.node, NodeId::Action(..)))
        .count();
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
    assert_eq!(
        rows.iter()
            .filter(|r| r.depth == 3 && !matches!(r.node, NodeId::Action(..)))
            .count(),
        total
    );
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

fn client_row(send_state: SendState) -> ClientRow {
    ClientRow {
        id: ClientId::new(2),
        protocol: "TCP".into(),
        remote_addr: "127.0.0.1:8080".into(),
        status: if send_state == SendState::NotConnected {
            ClientStatus::Disconnected
        } else {
            ClientStatus::Connected
        },
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
        send_state,
        send_actions: vec!["send_command".into(), "send_text".into()],
        intercepts: Vec::new(),
    }
}

/// The client's peer group: one entry per connection (live or past), with the
/// requests that flowed during it beneath — attempts are not a separate list.
#[test]
fn a_client_tree_groups_its_connections_with_their_traffic() {
    let mut row = client_row(SendState::Ready);
    // Two connections: an old one that closed, and the live one. The request
    // stream spans both; time attributes each to its connection.
    row.history = vec![
        netget::state::client::ClientConnectionAttempt {
            remote_addr: "127.0.0.1:8080".into(),
            started_unix_ms: 1_700_000_000_000,
            ended_unix_ms: Some(1_700_000_000_900),
            outcome: "connected".into(),
        },
        netget::state::client::ClientConnectionAttempt {
            remote_addr: "127.0.0.1:8080".into(),
            started_unix_ms: 1_700_000_001_000,
            ended_unix_ms: None,
            outcome: "connected".into(),
        },
    ];
    row.connection = Some(ConnRow {
        id: 9,
        remote_addr: "127.0.0.1:8080".into(),
        bytes_received: 5,
        bytes_sent: 7,
        active: true,
        can_message: false,
    });
    // entry ids double as unix_ms offsets (see `entry`): 5 lands in the first
    // window, 2000 in the second.
    row.requests = vec![entry(5, None), entry(2000, None)];

    let rows = tree::client_rows(&row, &TreeState::default());
    let key = UiKey::Client(row.id);

    let groups = labels_at_depth(&rows, 1);
    assert!(groups.iter().any(|l| l.starts_with("config")), "{groups:?}");
    assert!(
        groups.iter().any(|l| l.starts_with("handlers")),
        "{groups:?}"
    );
    assert!(
        groups.iter().any(|l| l.starts_with("peer (2 connections")),
        "{groups:?}"
    );
    assert!(
        !groups.iter().any(|l| l.starts_with("attempts")),
        "attempts merged into the peer group: {groups:?}"
    );

    // Order under the root: lifecycle first at a fixed place (it used to sit
    // at the bottom, where every new peer and request moved it), then the
    // client's own verbs — one row per action, not a generic send button.
    let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(
        &labels[1..5],
        &[
            "[ disconnect ]",
            "[ remove client ]",
            "[ send_command ]",
            "[ send_text ]"
        ],
        "{labels:?}"
    );
    assert_eq!(
        rows[3].node,
        NodeId::Action(key, tree::RowAction::SendAction(0))
    );
    assert_eq!(
        rows[4].node,
        NodeId::Action(key, tree::RowAction::SendAction(1))
    );

    // Newest connection first; each carries its own requests.
    let under_peer = subtree(&rows, "peer (");
    let entries: Vec<&tree::TreeRow> = under_peer
        .iter()
        .filter(|r| matches!(r.node, NodeId::Attempt(_, _)))
        .collect();
    assert_eq!(entries.len(), 2, "one entry per connection");
    assert!(
        entries[0].label.contains("↓5 ↑7") && entries[0].label.contains("· 1 req"),
        "the live connection shows its counters and its own traffic: {}",
        entries[0].label
    );
    assert!(
        entries[1].label.contains("(closed)") && entries[1].label.contains("· 1 req"),
        "the closed connection keeps its own traffic: {}",
        entries[1].label
    );

    // The requests really sit beneath their connection.
    let live_node = NodeId::Attempt(key, 1_700_000_001_000);
    let live_index = under_peer
        .iter()
        .position(|r| r.node == live_node)
        .expect("live entry");
    assert!(
        matches!(under_peer[live_index + 1].node, NodeId::Request(_, 2000)),
        "the live connection's request is the one from its own window"
    );
}

/// Every verb is a row, under the noun it acts on.
///
/// They used to be right-aligned buttons on the group rows, which put them
/// outside the up/down order entirely: the only way to reach one was to know
/// its shortcut, and nothing on screen said what the shortcuts were.
#[test]
fn actions_are_rows_beneath_the_thing_they_act_on() {
    use tree::RowAction;
    let key = UiKey::Server(ServerId::new(1));
    let row = server(vec![conn(1)], vec![]);
    let rows = tree::server_rows(&row, &all_open(key));

    let action_at = |group: &str, action: RowAction| {
        subtree(&rows, group)
            .iter()
            .any(|r| r.node == NodeId::Action(key, action))
    };

    assert!(
        action_at("config", RowAction::EditConfig),
        "editing config belongs under `config`"
    );
    assert!(
        action_at("handlers", RowAction::AddRoute),
        "adding a handler belongs under `handlers`"
    );
    assert!(
        action_at("peers", RowAction::AddClient),
        "connecting a client belongs under `peers` — it produces another peer"
    );
    assert!(
        rows.iter()
            .any(|r| r.node == NodeId::Action(key, RowAction::Stop)),
        "stopping the instance should be reachable as a row"
    );
}

/// The `[ edit config ]` row must survive the child cap.
///
/// The cap exists so one busy group cannot bury what is below it. Charging the
/// verb against it would mean a server with six startup parameters loses the
/// only row that can change them — the exact opposite of what the cap is for.
#[test]
fn the_edit_row_is_not_charged_against_the_child_cap() {
    let key = UiKey::Server(ServerId::new(1));
    let mut params = serde_json::Map::new();
    for i in 0..(CHILD_LIMIT * 3) {
        params.insert(format!("param_{i}"), serde_json::json!(i));
    }
    let mut row = server(vec![], vec![]);
    row.startup_params = Some(serde_json::Value::Object(params));

    let rows = tree::server_rows(&row, &all_open(key));
    let under_config = subtree(&rows, "config");
    assert!(
        under_config.iter().any(|r| r.label.starts_with("… ")),
        "the cap should have hidden something: {:?}",
        under_config.iter().map(|r| &r.label).collect::<Vec<_>>()
    );
    assert!(
        under_config
            .iter()
            .any(|r| r.node == NodeId::Action(key, tree::RowAction::EditConfig)),
        "[ edit config ] must survive the cap: {:?}",
        under_config.iter().map(|r| &r.label).collect::<Vec<_>>()
    );
}

/// A parked manual-handler request is the loudest thing in the tree: a row of
/// its own right under the instance, plus a chip on the root so a collapsed
/// instance still shows it.
#[test]
fn a_pending_intercept_is_visible_and_activatable() {
    use netget::state::intercepts::{InterceptOwner, InterceptView};

    let mut row = server(vec![conn(1)], vec![]);
    row.intercepts = vec![InterceptView {
        id: 7,
        owner: InterceptOwner::Server(ServerId::new(1)),
        connection_id: Some(1),
        event_type: "tcp_data_received".into(),
        description: "Data received".into(),
        event_data: Some(serde_json::json!({"data": "hello"})),
        created_unix_ms: 1_700_000_000_000,
    }];

    let key = UiKey::Server(ServerId::new(1));
    let rows = tree::server_rows(&row, &TreeState::default());
    assert!(
        rows[0].label.contains("⚠ 1 waiting"),
        "the root should carry the waiting chip: {}",
        rows[0].label
    );
    assert_eq!(
        rows[1].node,
        NodeId::Intercept(key, 7),
        "the intercept row comes first under the root: {:?}",
        rows.iter().map(|r| &r.label).collect::<Vec<_>>()
    );
    assert!(rows[1].label.contains("tcp_data_received"));

    // Collapsed, the chip is still there even though the row is not.
    let mut state = TreeState::default();
    state.collapse(&NodeId::Instance(key));
    let rows = tree::server_rows(&row, &state);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].label.contains("waiting"));
}

/// The LLM fallback is not a handler, and must not be reachable as one.
///
/// It was `NodeId::Route(key, usize::MAX)` — indistinguishable from a real
/// handler to everything downstream, which is how it ended up looking editable
/// and deletable when it is neither.
#[test]
fn the_llm_fallback_is_stated_but_not_a_route() {
    let key = UiKey::Server(ServerId::new(1));
    let row = server(vec![], vec![]);
    let rows = tree::server_rows(&row, &all_open(key));
    let under_routing = subtree(&rows, "handlers");

    assert!(
        under_routing
            .iter()
            .any(|r| matches!(r.node, NodeId::RoutingFallback(_))),
        "the fallback should still be visible: {:?}",
        under_routing.iter().map(|r| &r.label).collect::<Vec<_>>()
    );
    assert!(
        !under_routing
            .iter()
            .any(|r| matches!(r.node, NodeId::Route(..))),
        "with no handlers configured there are no routes, only the fallback note"
    );
    assert!(
        rows.iter().any(|r| r.label.starts_with("handlers (0)")),
        "the fallback must not be counted as a configured handler: {:?}",
        rows.iter().map(|r| &r.label).collect::<Vec<_>>()
    );
}

/// With a `*` rule nothing can fall through, so the "otherwise → LLM" note
/// would describe a path that cannot happen — it disappears.
#[test]
fn a_wildcard_handler_hides_the_llm_fallback_note() {
    use netget::scripting::{EventHandler, EventHandlerConfig, EventHandlerType, EventPattern};

    let key = UiKey::Server(ServerId::new(1));
    let mut config = EventHandlerConfig::new();
    config.add_handler(EventHandler::new(
        EventPattern::wildcard(),
        EventHandlerType::manual(300),
    ));
    let mut row = server(vec![], vec![]);
    row.routing = Some(config);

    let rows = tree::server_rows(&row, &all_open(key));
    let under = subtree(&rows, "handlers");
    assert!(
        under.iter().any(|r| r.label.contains("MANUAL")),
        "{:?}",
        under.iter().map(|r| &r.label).collect::<Vec<_>>()
    );
    assert!(
        !under
            .iter()
            .any(|r| matches!(r.node, NodeId::RoutingFallback(_))),
        "a wildcard rule catches everything; the LLM note must go: {:?}",
        under.iter().map(|r| &r.label).collect::<Vec<_>>()
    );
}

/// The verbs a client offers: disconnect while connected (keeping the row),
/// connect while disconnected, and remove always. A message-capable server
/// connection additionally offers [ message this peer ].
#[test]
fn client_verbs_and_peer_messaging_follow_state() {
    use tree::RowAction;

    // Connected client: disconnect + remove, no connect.
    let row = client_row(SendState::Ready);
    let rows = tree::client_rows(&row, &TreeState::default());
    let key = UiKey::Client(row.id);
    let has = |action: RowAction, rows: &[tree::TreeRow]| {
        rows.iter().any(|r| r.node == NodeId::Action(key, action))
    };
    // Every declared action is reachable as its own row.
    assert!(has(RowAction::SendAction(0), &rows));
    assert!(has(RowAction::SendAction(1), &rows));
    assert!(has(RowAction::Disconnect, &rows));
    assert!(!has(RowAction::Connect, &rows));
    assert!(has(RowAction::Stop, &rows));
    assert!(
        rows.iter().any(|r| r.label.contains("remove client")),
        "the client's stop row says remove"
    );

    // Disconnected: connect + remove, no disconnect — and no action rows,
    // replaced by the row that says why sending is unavailable.
    let row = client_row(SendState::NotConnected);
    let rows = tree::client_rows(&row, &TreeState::default());
    assert!(has(RowAction::Connect, &rows));
    assert!(!has(RowAction::Disconnect, &rows));
    assert!(!has(RowAction::SendAction(0), &rows));
    assert!(
        rows.iter().any(|r| r.label.contains("not connected")),
        "{:?}",
        rows.iter().map(|r| &r.label).collect::<Vec<_>>()
    );

    // A protocol that declares nothing to send says so, rather than showing
    // an empty gap where the verbs would be.
    let mut row = client_row(SendState::Ready);
    row.send_actions.clear();
    let rows = tree::client_rows(&row, &TreeState::default());
    assert!(
        rows.iter()
            .any(|r| r.label.contains("declares no client actions")),
        "{:?}",
        rows.iter().map(|r| &r.label).collect::<Vec<_>>()
    );

    // A server connection that registered a peer handle offers messaging.
    let server_key = UiKey::Server(ServerId::new(1));
    let mut c = conn(1);
    c.can_message = true;
    let srow = server(vec![c], vec![]);
    let rows = tree::server_rows(&srow, &TreeState::default());
    assert!(
        rows.iter()
            .any(|r| r.node == NodeId::Action(server_key, RowAction::MessagePeer(1))),
        "{:?}",
        rows.iter().map(|r| &r.label).collect::<Vec<_>>()
    );

    // A handle also brings the server-side hang-up.
    assert!(rows
        .iter()
        .any(|r| r.node == NodeId::Action(server_key, RowAction::DisconnectPeer(1))));

    // One that did not register a handle gets neither verb — only a dim row
    // saying why, so the missing control does not read as a missing feature.
    let srow = server(vec![conn(1)], vec![]);
    let rows = tree::server_rows(&srow, &TreeState::default());
    assert!(!rows.iter().any(|r| matches!(
        r.node,
        NodeId::Action(_, RowAction::MessagePeer(_) | RowAction::DisconnectPeer(_))
    ) && r.style == tree::RowStyle::Button));
    assert!(rows
        .iter()
        .any(|r| r.style == tree::RowStyle::Dim && r.label.contains("cannot message")));
}
