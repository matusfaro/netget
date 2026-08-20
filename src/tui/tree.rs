//! The rail's tree model.
//!
//! An instance is a root; under it sit `config`, `routing` and `peers`; under
//! a peer sit the requests that arrived on it. Fixed columns forced every
//! group to the same width and could not express that nesting — a request
//! belongs to the connection that carried it, not to a parallel column.
//!
//! Long lists are capped so one busy peer cannot bury everything below it;
//! the cap lifts per node ("show all"), and any group can be collapsed.

use std::collections::HashSet;

use crate::state::app_state::AccessLogEntry;
use crate::tui::app::UiKey;
use crate::tui::modal::request_detail::summary_line;
use crate::tui::projection::{ClientRow, SendState, ServerRow};

/// How many children a group shows before the "… N more" row.
pub const CHILD_LIMIT: usize = 5;

/// How many of a client's own actions are inlined under it before the rest
/// collapse behind "… N more". Higher than [`CHILD_LIMIT`] because these are
/// the client's whole point, and most protocols declare only a handful.
pub const INLINE_ACTION_LIMIT: usize = 8;

/// Something the user can do, sitting in the tree as a row of its own.
///
/// These used to be right-aligned buttons on the group rows — `[ edit ]` on the
/// instance, `[ + client ]` on `peers`. That put the verb somewhere the eye does
/// not travel and gave it no place in the up/down order, so reaching one meant
/// knowing a shortcut. As rows they are in the same list as everything else:
/// Enter runs them, a click runs them, and they sit under the thing they act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RowAction {
    /// Open the instance's config form.
    EditConfig,
    /// Open the routing editor on one handler.
    EditRoute(usize),
    /// Open the routing editor with a fresh handler.
    AddRoute,
    /// Connect a client of the counterpart protocol to this server.
    AddClient,
    /// Compose and send a request through this client, choosing the action in
    /// the composer. Kept for the `n` shortcut and for the rows that explain
    /// why sending is unavailable.
    Send,
    /// Send one specific action, by index into `ClientRow::send_actions`. The
    /// composer opens straight on that action's parameters.
    SendAction(usize),
    /// Compose and send an action to one live server connection (only where
    /// the protocol registered a peer handle).
    MessagePeer(u32),
    /// Hang up the client's connection but keep the row for a later connect.
    Disconnect,
    /// (Re)connect a disconnected client.
    Connect,
    /// Show the Wireshark / tshark command and filters that capture this
    /// instance's traffic (`tui::wireshark`).
    Wireshark,
    /// Stop this instance (servers) / remove it (clients).
    Stop,
}

/// Identity of a tree node, stable across re-polls so expansion state and the
/// selection survive polling.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeId {
    Instance(UiKey),
    Config(UiKey),
    ConfigItem(UiKey, String),
    Routing(UiKey),
    Route(UiKey, usize),
    /// The "anything else goes to the LLM" note. Not a handler — there is
    /// nothing to edit, reorder or delete — so it gets its own id and does
    /// nothing when activated.
    RoutingFallback(UiKey),
    /// A verb the user can run, placed under whatever it acts on.
    Action(UiKey, RowAction),
    /// A request a `manual` rule parked, waiting for the operator to answer.
    /// Activating it opens the answer modal.
    Intercept(UiKey, u64),
    /// Start a new instance. Owned by the rail rather than by any instance,
    /// so it carries no `UiKey`.
    NewInstance(crate::tui::app::Section),
    Peers(UiKey),
    /// A connection, by its id. `None` is the bucket for connectionless
    /// protocols, whose events carry no connection.
    Peer(UiKey, Option<u32>),
    Request(UiKey, u64),
    /// One client connection (live or past), by its start time.
    Attempt(UiKey, u64),
    /// One line of an expanded request's detail (index into its detail lines).
    RequestDetail(UiKey, u64, usize),
    /// The "… N more" row belonging to a group.
    More(Box<NodeId>),
}

/// How a row should be styled, resolved against the palette at render time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowStyle {
    Instance,
    Group,
    Normal,
    Dim,
    Good,
    Warn,
    Bad,
    Button,
}

#[derive(Debug, Clone)]
pub struct TreeRow {
    pub node: NodeId,
    pub depth: u16,
    pub label: String,
    pub style: RowStyle,
    /// Present on group rows; `None` on leaves.
    pub expanded: Option<bool>,
}

impl TreeRow {
    fn leaf(node: NodeId, depth: u16, label: impl Into<String>, style: RowStyle) -> Self {
        Self {
            node,
            depth,
            label: label.into(),
            style,
            expanded: None,
        }
    }

    fn group(node: NodeId, depth: u16, label: impl Into<String>, expanded: bool) -> Self {
        Self {
            node,
            depth,
            label: label.into(),
            style: RowStyle::Group,
            expanded: Some(expanded),
        }
    }
}

/// A runnable row. Styled as a button so it reads as a verb among the nouns.
fn action_row(key: UiKey, action: RowAction, depth: u16, label: impl Into<String>) -> TreeRow {
    TreeRow::leaf(NodeId::Action(key, action), depth, label, RowStyle::Button)
}

/// The two rows that start something new, at the foot of the rail.
///
/// They sit where the thing they create will appear, and they are the whole
/// content of an empty rail — so "how do I start a server" is answered by a row
/// you can walk onto, not by a button in a header or a shortcut you have to be
/// told about.
pub fn new_instance_rows() -> Vec<TreeRow> {
    use crate::tui::app::Section;
    vec![
        TreeRow::leaf(
            NodeId::NewInstance(Section::Servers),
            0,
            "[ + new server ]",
            RowStyle::Button,
        ),
        TreeRow::leaf(
            NodeId::NewInstance(Section::Clients),
            0,
            "[ + new client ]",
            RowStyle::Button,
        ),
    ]
}

/// A row for an action that cannot run right now, with the reason in place of
/// the verb. Dim rather than absent: a missing control reads as a missing
/// feature, and `[ send ]` on a client whose protocol has no command channel is
/// worth explaining once rather than hiding forever.
fn disabled_row(key: UiKey, action: RowAction, depth: u16, label: impl Into<String>) -> TreeRow {
    TreeRow::leaf(NodeId::Action(key, action), depth, label, RowStyle::Dim)
}

/// Per-band expansion state.
#[derive(Debug, Default, Clone)]
pub struct TreeState {
    /// Nodes the user collapsed. Groups are expanded by default, so storing
    /// the collapsed set means a new server arrives fully visible.
    pub collapsed: HashSet<NodeId>,
    /// Nodes whose child cap has been lifted.
    pub show_all: HashSet<NodeId>,
    /// Nodes that are collapsed by default and have been opened explicitly
    /// (requests: a peer with thirty of them should read as a list).
    pub opened: HashSet<NodeId>,
}

impl TreeState {
    pub fn is_expanded(&self, node: &NodeId) -> bool {
        !self.collapsed.contains(node)
    }

    /// Whether a node is collapsed unless explicitly opened.
    ///
    /// Config and handlers are settings — consulted sometimes, in the way
    /// always. Peers and their traffic are what the dashboard is FOR, so they
    /// stay open.
    pub fn defaults_closed(node: &NodeId) -> bool {
        matches!(
            node,
            NodeId::Request(_, _) | NodeId::Config(_) | NodeId::Routing(_)
        )
    }

    pub fn is_open(&self, node: &NodeId) -> bool {
        if Self::defaults_closed(node) {
            self.opened.contains(node)
        } else {
            self.is_expanded(node)
        }
    }

    pub fn toggle(&mut self, node: &NodeId) {
        if Self::defaults_closed(node) {
            if self.opened.contains(node) {
                self.opened.remove(node);
            } else {
                self.opened.insert(node.clone());
            }
            return;
        }
        if self.collapsed.contains(node) {
            self.collapsed.remove(node);
        } else {
            self.collapsed.insert(node.clone());
        }
    }

    pub fn expand(&mut self, node: &NodeId) {
        if Self::defaults_closed(node) {
            self.opened.insert(node.clone());
        }
        self.collapsed.remove(node);
    }

    pub fn collapse(&mut self, node: &NodeId) {
        if Self::defaults_closed(node) {
            self.opened.remove(node);
            return;
        }
        self.collapsed.insert(node.clone());
    }

    pub fn show_all(&mut self, node: &NodeId) {
        self.show_all.insert(node.clone());
    }

    fn limit_for(&self, node: &NodeId) -> usize {
        if self.show_all.contains(node) {
            usize::MAX
        } else {
            CHILD_LIMIT
        }
    }
}

/// Detail lines shown for an expanded request before "… N more".
///
/// Generous on purpose: the header alone is ~7 lines, so a tight cap cut off
/// before the response — the half you opened the request to read. Expanding is
/// opt-in and the band scrolls, so the cost of being generous is low; the cap
/// only exists to stop a megabyte payload from becoming a megabyte of rows.
pub const DETAIL_LIMIT: usize = 60;

/// Push children with an explicit cap.
fn push_capped_with_limit(
    rows: &mut Vec<TreeRow>,
    state: &TreeState,
    group: &NodeId,
    depth: u16,
    children: Vec<TreeRow>,
    limit: usize,
) {
    let limit = if state.show_all.contains(group) {
        usize::MAX
    } else {
        limit
    };
    let total = children.len();
    let shown = total.min(limit);
    rows.extend(children.into_iter().take(shown));
    if total > shown {
        rows.push(TreeRow::leaf(
            NodeId::More(Box::new(group.clone())),
            depth,
            format!("… {} more", total - shown),
            RowStyle::Dim,
        ));
    }
}

/// Push item-groups with the cap applied to the number of ITEMS, not to the
/// number of rows they flatten into.
///
/// This matters for requests: an expanded one contributes its whole detail, and
/// charging those rows against the peer's request cap silently truncated the
/// list the moment you opened anything.
fn push_capped_groups(
    rows: &mut Vec<TreeRow>,
    state: &TreeState,
    group: &NodeId,
    depth: u16,
    items: Vec<Vec<TreeRow>>,
) {
    let limit = state.limit_for(group);
    let total = items.len();
    let shown = total.min(limit);
    for item in items.into_iter().take(shown) {
        rows.extend(item);
    }
    if total > shown {
        rows.push(TreeRow::leaf(
            NodeId::More(Box::new(group.clone())),
            depth,
            format!("… {} more", total - shown),
            RowStyle::Dim,
        ));
    }
}

/// Push children with the group's cap applied, plus a "… N more" row when the
/// cap hides something.
fn push_capped(
    rows: &mut Vec<TreeRow>,
    state: &TreeState,
    group: &NodeId,
    depth: u16,
    children: Vec<TreeRow>,
) {
    let limit = state.limit_for(group);
    let total = children.len();
    let shown = total.min(limit);
    rows.extend(children.into_iter().take(shown));
    if total > shown {
        rows.push(TreeRow::leaf(
            NodeId::More(Box::new(group.clone())),
            depth,
            format!("… {} more", total - shown),
            RowStyle::Dim,
        ));
    }
}

/// `"  ⚠ N waiting"` on the instance row while intercepts are pending, so a
/// collapsed instance still shows that something is blocked on the human.
fn waiting_chip(count: usize) -> String {
    if count == 0 {
        String::new()
    } else {
        format!("  ⚠ {count} waiting for you")
    }
}

/// One high-visibility row per pending intercept, right under the instance.
fn push_intercepts(
    rows: &mut Vec<TreeRow>,
    key: UiKey,
    intercepts: &[crate::state::intercepts::InterceptView],
) {
    for view in intercepts {
        rows.push(TreeRow::leaf(
            NodeId::Intercept(key, view.id),
            1,
            format!(
                "⚠ {} waiting for YOUR answer — Enter to respond",
                view.event_type
            ),
            RowStyle::Bad,
        ));
    }
}

/// Flatten one server into display rows.
pub fn server_rows(row: &ServerRow, state: &TreeState) -> Vec<TreeRow> {
    let key = UiKey::Server(row.id);
    let mut rows = Vec::new();

    let instance = NodeId::Instance(key);
    let instance_expanded = state.is_expanded(&instance);
    let addr = row
        .local_addr
        .clone()
        .unwrap_or_else(|| format!(":{}", row.port));
    rows.push(TreeRow::group(
        instance.clone(),
        0,
        format!(
            "#{} {} {}  {}{}",
            row.id.as_u32(),
            row.protocol,
            addr,
            row.status,
            waiting_chip(row.intercepts.len()),
        ),
        instance_expanded,
    ));
    if !instance_expanded {
        return rows;
    }

    // Requests waiting for YOUR answer come first — they are the only rows in
    // the whole tree where something is blocked on the human.
    push_intercepts(&mut rows, key, &row.intercepts);

    // Lifecycle next, at a fixed place. At the bottom it moved every time a
    // peer connected or a request arrived, so it was never where you left it.
    rows.push(action_row(key, RowAction::Stop, 1, "[ stop server ]"));

    // ---- config (collapsed by default: settings, not traffic) ----
    let config = NodeId::Config(key);
    let config_expanded = state.is_open(&config);
    let mut config_items: Vec<TreeRow> = Vec::new();
    if let Some(params) = row.startup_params.as_ref().and_then(|p| p.as_object()) {
        for (k, v) in params {
            config_items.push(TreeRow::leaf(
                NodeId::ConfigItem(key, k.clone()),
                2,
                format!("{k}: {v}"),
                RowStyle::Normal,
            ));
        }
    }
    config_items.push(TreeRow::leaf(
        NodeId::ConfigItem(key, "instruction".into()),
        2,
        format!(
            "instruction: {}",
            crate::utils::truncate_for_log(&row.instruction, 80)
        ),
        RowStyle::Dim,
    ));
    config_items.push(TreeRow::leaf(
        NodeId::ConfigItem(key, "memory".into()),
        2,
        format!("memory: {} bytes", row.memory_len),
        RowStyle::Dim,
    ));
    if row.task_count > 0 {
        config_items.push(TreeRow::leaf(
            NodeId::ConfigItem(key, "tasks".into()),
            2,
            format!("scheduled tasks: {}", row.task_count),
            RowStyle::Dim,
        ));
    }
    rows.push(TreeRow::group(
        config.clone(),
        1,
        format!("config ({})", config_items.len()),
        config_expanded,
    ));
    if config_expanded {
        // The cap applies to the settings, never to the verb: `[ edit config ]`
        // is pushed after it, so an instance with many parameters cannot hide
        // the one row that lets you change them.
        push_capped(&mut rows, state, &config, 2, config_items);
        rows.push(action_row(key, RowAction::EditConfig, 2, "[ edit config ]"));
    }

    // ---- handlers (collapsed by default) ----
    let routing = NodeId::Routing(key);
    let routing_expanded = state.is_open(&routing);
    rows.push(TreeRow::group(
        routing.clone(),
        1,
        format!("handlers ({})", handler_count(row.routing.as_ref())),
        routing_expanded,
    ));
    if routing_expanded {
        let routes = route_rows(key, 2, row.routing.as_ref());
        push_capped(&mut rows, state, &routing, 2, routes);
        rows.push(action_row(key, RowAction::AddRoute, 2, "[ + add handler ]"));
        // Stated only when reachable: with a `*` rule nothing falls through.
        if !has_wildcard(row.routing.as_ref()) {
            rows.push(TreeRow::leaf(
                NodeId::RoutingFallback(key),
                2,
                "otherwise → LLM (the instance instruction)",
                RowStyle::Dim,
            ));
        }
    }

    // ---- peers, with each peer's requests beneath it ----
    let peers = NodeId::Peers(key);
    let peers_expanded = state.is_expanded(&peers);
    let live = row.conns.len();
    rows.push(TreeRow::group(
        peers.clone(),
        1,
        format!("peers ({live} live, {} recent)", row.recent.len()),
        peers_expanded,
    ));

    if peers_expanded {
        let mut peer_rows: Vec<TreeRow> = Vec::new();
        for conn in &row.conns {
            peer_rows.push(peer_with_requests(
                key,
                state,
                Some(conn.id),
                format!(
                    "{}  ↓{} ↑{}",
                    conn.remote_addr, conn.bytes_received, conn.bytes_sent
                ),
                if conn.active {
                    RowStyle::Good
                } else {
                    RowStyle::Dim
                },
                &row.requests,
                &mut rows,
            ));
            // Messaging one peer, where the protocol permits it: the row sits
            // with the peer's own traffic, since that is where the send lands.
            if conn.can_message && state.is_expanded(&NodeId::Peer(key, Some(conn.id))) {
                rows.push(action_row(
                    key,
                    RowAction::MessagePeer(conn.id),
                    3,
                    "[ message this peer ]",
                ));
            }
        }
        for closed in &row.recent {
            peer_rows.push(peer_with_requests(
                key,
                state,
                Some(closed.id),
                format!(
                    "{}  ↓{} ↑{}  (closed)",
                    closed.remote_addr, closed.bytes_received, closed.bytes_sent
                ),
                RowStyle::Dim,
                &row.requests,
                &mut rows,
            ));
        }
        // Connectionless events (DNS, UDP…) have no peer to hang from.
        if row.requests.iter().any(|r| r.connection_id.is_none()) {
            peer_rows.push(peer_with_requests(
                key,
                state,
                None,
                "(connectionless)".to_string(),
                RowStyle::Dim,
                &row.requests,
                &mut rows,
            ));
        }
        // `peer_with_requests` pushed straight into `rows`; the vec above only
        // tracks how many peers there were.
        if peer_rows.is_empty() {
            rows.push(TreeRow::leaf(
                NodeId::Peer(key, None),
                2,
                "(no connections yet)",
                RowStyle::Dim,
            ));
        }

        // Connecting a client belongs with the peers it will join, not on the
        // instance row: what it produces is another entry in this list.
        match &row.client_counterpart {
            Some(protocol) => rows.push(action_row(
                key,
                RowAction::AddClient,
                2,
                format!("[ + connect a {protocol} client ]"),
            )),
            None => rows.push(disabled_row(
                key,
                RowAction::AddClient,
                2,
                "(no client implementation for this protocol)",
            )),
        }
    }

    rows.push(action_row(
        key,
        RowAction::Wireshark,
        1,
        "[ view in wireshark ]",
    ));
    rows
}

/// Push one peer row plus its requests, returning a marker row so the caller
/// can tell whether any peer existed.
#[allow(clippy::too_many_arguments)]
fn peer_with_requests(
    key: UiKey,
    state: &TreeState,
    conn_id: Option<u32>,
    label: String,
    style: RowStyle,
    requests: &[AccessLogEntry],
    rows: &mut Vec<TreeRow>,
) -> TreeRow {
    let node = NodeId::Peer(key, conn_id);
    let mine: Vec<&AccessLogEntry> = requests
        .iter()
        .filter(|r| r.connection_id == conn_id)
        .collect();
    let expanded = state.is_expanded(&node);
    let row = TreeRow::group(
        node.clone(),
        2,
        format!("{label}  · {} req", mine.len()),
        expanded,
    );
    let mut row_styled = row;
    row_styled.style = style;
    rows.push(row_styled.clone());

    if expanded && !mine.is_empty() {
        let items: Vec<Vec<TreeRow>> = mine
            .iter()
            .map(|entry| request_rows(key, state, entry, 3))
            .collect();
        push_capped_groups(rows, state, &node, 3, items);
    }
    row_styled
}

/// A request row, plus its full request/response detail when expanded.
///
/// The detail is the same text the request modal shows; having it inline means
/// following a conversation does not cost a round trip through an overlay.
fn request_rows(key: UiKey, state: &TreeState, entry: &AccessLogEntry, depth: u16) -> Vec<TreeRow> {
    let node = NodeId::Request(key, entry.id);
    // Requests default to collapsed: a peer with thirty of them should read as
    // a list, not a wall of JSON.
    let expanded = state.is_open(&node);
    let mut rows = vec![TreeRow::group(
        node.clone(),
        depth,
        summary_line(entry),
        expanded,
    )];
    if expanded {
        let detail: Vec<TreeRow> = crate::tui::modal::request_detail::detail_lines(entry)
            .into_iter()
            .enumerate()
            .map(|(index, line)| {
                TreeRow::leaf(
                    NodeId::RequestDetail(key, entry.id, index),
                    depth + 1,
                    line,
                    RowStyle::Dim,
                )
            })
            .collect();
        push_capped_with_limit(&mut rows, state, &node, depth + 1, detail, DETAIL_LIMIT);
    }
    rows
}

/// How many handlers are configured. The fallback is not one of them.
fn handler_count(routing: Option<&crate::scripting::EventHandlerConfig>) -> usize {
    routing.map(|c| c.handlers.len()).unwrap_or(0)
}

/// Whether a `*` rule exists. When one does, nothing can fall through to the
/// LLM, so showing the "otherwise → LLM" note would claim a path that cannot
/// happen.
fn has_wildcard(routing: Option<&crate::scripting::EventHandlerConfig>) -> bool {
    routing.is_some_and(|c| {
        c.handlers.iter().any(|h| {
            matches!(
                h.event_pattern,
                crate::scripting::event_handler::EventPattern::Wildcard
            )
        })
    })
}

/// One row per configured handler, in match order.
///
/// Activating a row edits *that* handler. The always-present LLM fallback is
/// not included — it is not a handler, so it is stated separately by the caller.
fn route_rows(
    key: UiKey,
    depth: u16,
    routing: Option<&crate::scripting::EventHandlerConfig>,
) -> Vec<TreeRow> {
    use crate::scripting::event_handler::EventPattern;
    use crate::scripting::EventHandlerType;

    let mut rows = Vec::new();
    if let Some(config) = routing {
        for (index, handler) in config.handlers.iter().enumerate() {
            let pattern = match &handler.event_pattern {
                EventPattern::Specific(s) => s.clone(),
                EventPattern::Wildcard => "*".to_string(),
            };
            let (body, style) = match &handler.handler {
                EventHandlerType::Llm { instruction } => (
                    format!("LLM — {}", crate::utils::truncate_for_log(instruction, 40)),
                    RowStyle::Normal,
                ),
                EventHandlerType::Script {
                    language, resident, ..
                } => (
                    format!(
                        "SCRIPT ({language}{})",
                        if *resident { ", resident" } else { "" }
                    ),
                    RowStyle::Warn,
                ),
                EventHandlerType::Static { actions } => {
                    let names: Vec<&str> = actions
                        .iter()
                        .filter_map(|a| a.get("type").and_then(|t| t.as_str()))
                        .collect();
                    (
                        format!(
                            "STATIC — {}",
                            if names.is_empty() {
                                "(no actions)".to_string()
                            } else {
                                names.join(", ")
                            }
                        ),
                        RowStyle::Good,
                    )
                }
                EventHandlerType::Manual { .. } => (
                    "MANUAL — you answer each one here".to_string(),
                    RowStyle::Warn,
                ),
            };
            rows.push(TreeRow::leaf(
                NodeId::Route(key, index),
                depth,
                format!("{pattern} → {body}"),
                style,
            ));
        }
    }
    rows
}

/// Flatten one client into display rows.
pub fn client_rows(row: &ClientRow, state: &TreeState) -> Vec<TreeRow> {
    let key = UiKey::Client(row.id);
    let mut rows = Vec::new();

    let instance = NodeId::Instance(key);
    let instance_expanded = state.is_expanded(&instance);
    rows.push(TreeRow::group(
        instance.clone(),
        0,
        format!(
            "#{} {} → {}  {}{}",
            row.id.as_u32(),
            row.protocol,
            row.remote_addr,
            row.status,
            waiting_chip(row.intercepts.len()),
        ),
        instance_expanded,
    ));
    if !instance_expanded {
        return rows;
    }

    push_intercepts(&mut rows, key, &row.intercepts);

    // Lifecycle first, at a fixed place — see the server's note. Hang up and
    // reconnect keep the instance; remove takes it away entirely.
    match row.send_state {
        SendState::NotConnected => rows.push(action_row(key, RowAction::Connect, 1, "[ connect ]")),
        _ => rows.push(action_row(key, RowAction::Disconnect, 1, "[ disconnect ]")),
    }
    rows.push(action_row(key, RowAction::Stop, 1, "[ remove client ]"));

    // Sending is what a client is FOR, so it is the first thing under the
    // root — not something to discover three levels down under a peer.
    //
    // One row per action the protocol actually offers (telnet's send_command
    // and send_text, TCP's send_tcp_data), rather than a single generic
    // button: the verbs ARE the protocol, and a menu that only says "send a
    // request" hides what this client can do until you open it.
    match row.send_state {
        SendState::Ready if !row.send_actions.is_empty() => {
            let sends: Vec<TreeRow> = row
                .send_actions
                .iter()
                .enumerate()
                .map(|(index, name)| {
                    action_row(key, RowAction::SendAction(index), 1, format!("[ {name} ]"))
                })
                .collect();
            // Capped generously: a protocol with a long vocabulary must not
            // push config and peers off the screen, but the common case (a
            // handful of verbs) is never truncated.
            push_capped_with_limit(&mut rows, state, &instance, 1, sends, INLINE_ACTION_LIMIT);
        }
        // Connected, but the protocol declares nothing to send.
        SendState::Ready => rows.push(disabled_row(
            key,
            RowAction::Send,
            1,
            "(this protocol declares no client actions)",
        )),
        SendState::NotConnected => rows.push(disabled_row(
            key,
            RowAction::Send,
            1,
            "(cannot send — not connected)",
        )),
        SendState::ProtocolUnsupported => rows.push(disabled_row(
            key,
            RowAction::Send,
            1,
            "(cannot send — this client has no command channel yet)",
        )),
    }

    // ---- config (collapsed by default: settings, not traffic) ----
    let config = NodeId::Config(key);
    let config_expanded = state.is_open(&config);
    let mut config_items: Vec<TreeRow> = Vec::new();
    if let Some(params) = row.startup_params.as_ref().and_then(|p| p.as_object()) {
        for (k, v) in params {
            config_items.push(TreeRow::leaf(
                NodeId::ConfigItem(key, k.clone()),
                2,
                format!("{k}: {v}"),
                RowStyle::Normal,
            ));
        }
    }
    config_items.push(TreeRow::leaf(
        NodeId::ConfigItem(key, "instruction".into()),
        2,
        format!(
            "instruction: {}",
            crate::utils::truncate_for_log(&row.instruction, 80)
        ),
        RowStyle::Dim,
    ));
    config_items.push(TreeRow::leaf(
        NodeId::ConfigItem(key, "memory".into()),
        2,
        format!("memory: {} bytes", row.memory_len),
        RowStyle::Dim,
    ));
    rows.push(TreeRow::group(
        config.clone(),
        1,
        format!("config ({})", config_items.len()),
        config_expanded,
    ));
    if config_expanded {
        // The cap applies to the settings, never to the verb: `[ edit config ]`
        // is pushed after it, so an instance with many parameters cannot hide
        // the one row that lets you change them.
        push_capped(&mut rows, state, &config, 2, config_items);
        rows.push(action_row(key, RowAction::EditConfig, 2, "[ edit config ]"));
    }

    // ---- handlers (collapsed by default) ----
    //
    // A client's handlers decide how to REACT to what the server sends back;
    // sending is the `[ send a request ]` row above.
    let routing = NodeId::Routing(key);
    let routing_expanded = state.is_open(&routing);
    rows.push(TreeRow::group(
        routing.clone(),
        1,
        format!("handlers ({})", handler_count(row.routing.as_ref())),
        routing_expanded,
    ));
    if routing_expanded {
        let routes = route_rows(key, 2, row.routing.as_ref());
        push_capped(&mut rows, state, &routing, 2, routes);
        rows.push(action_row(key, RowAction::AddRoute, 2, "[ + add handler ]"));
        // Stated only when reachable: with a `*` rule nothing falls through.
        if !has_wildcard(row.routing.as_ref()) {
            rows.push(TreeRow::leaf(
                NodeId::RoutingFallback(key),
                2,
                "otherwise → the LLM decides how to react (instance instruction)",
                RowStyle::Dim,
            ));
        }
    }

    // ---- peer: one entry per connection, its traffic beneath ----
    //
    // The old layout split this into a `peer` group (the live connection) and
    // a sibling `attempts` list (bare history rows). Merged: every connection
    // — live or past — is one entry here, and the requests that flowed during
    // it sit underneath. Requests are attributed to connections by time:
    // attempts are chronological, so a request belongs to the last connection
    // that started before it.
    let peers = NodeId::Peers(key);
    let peers_expanded = state.is_expanded(&peers);
    let connection_count = row.history.len().max(usize::from(row.connection.is_some()));
    rows.push(TreeRow::group(
        peers.clone(),
        1,
        format!(
            "peer ({} connection{}, {} req)",
            connection_count,
            if connection_count == 1 { "" } else { "s" },
            row.requests.len()
        ),
        peers_expanded,
    ));
    if peers_expanded {
        if row.history.is_empty() {
            // No recorded attempts (older protocols): fall back to one entry
            // for the live connection, carrying everything.
            match &row.connection {
                Some(c) => {
                    peer_with_requests(
                        key,
                        state,
                        None,
                        format!("{}  ↓{} ↑{}", c.remote_addr, c.bytes_received, c.bytes_sent),
                        if c.active {
                            RowStyle::Good
                        } else {
                            RowStyle::Dim
                        },
                        &row.requests,
                        &mut rows,
                    );
                }
                None => rows.push(TreeRow::leaf(
                    NodeId::Peer(key, None),
                    2,
                    "(no connections yet)",
                    RowStyle::Dim,
                )),
            }
        } else {
            let starts: Vec<u64> = row.history.iter().map(|a| a.started_unix_ms).collect();
            let last = row.history.len() - 1;
            // Newest connection first.
            for (index, attempt) in row.history.iter().enumerate().rev() {
                // The window: from this connection's start (0 for the first,
                // so early requests are never orphaned) to the next one's.
                let from = if index == 0 { 0 } else { starts[index] };
                let to = starts.get(index + 1).copied().unwrap_or(u64::MAX);
                let mine: Vec<&AccessLogEntry> = row
                    .requests
                    .iter()
                    .filter(|r| r.unix_ms >= from && r.unix_ms < to)
                    .collect();

                let live =
                    index == last && attempt.ended_unix_ms.is_none() && row.connection.is_some();
                let label = if live {
                    let c = row.connection.as_ref().expect("checked above");
                    format!(
                        "{}  ↓{} ↑{}  · {} req",
                        c.remote_addr,
                        c.bytes_received,
                        c.bytes_sent,
                        mine.len()
                    )
                } else {
                    format!(
                        "{} — {}{}  · {} req",
                        attempt.remote_addr,
                        attempt.outcome,
                        if attempt.ended_unix_ms.is_some() {
                            " (closed)"
                        } else {
                            ""
                        },
                        mine.len()
                    )
                };

                let node = NodeId::Attempt(key, attempt.started_unix_ms);
                let expanded = state.is_expanded(&node);
                let mut group = TreeRow::group(node.clone(), 2, label, expanded);
                group.style = if live { RowStyle::Good } else { RowStyle::Dim };
                rows.push(group);
                if expanded && !mine.is_empty() {
                    let items: Vec<Vec<TreeRow>> = mine
                        .iter()
                        .map(|entry| request_rows(key, state, entry, 3))
                        .collect();
                    push_capped_groups(&mut rows, state, &node, 3, items);
                }
            }
        }
    }

    rows.push(action_row(
        key,
        RowAction::Wireshark,
        1,
        "[ view in wireshark ]",
    ));
    rows
}
