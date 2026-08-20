//! Keyboard and mouse handling.
//!
//! Precedence: an open modal owns everything; then the global toggles (which
//! keep their legacy bindings for parity); then focus-specific handling.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use tokio::sync::mpsc;

use crate::events::EventHandler;
use crate::state::app_state::AppState;
use crate::tui::app::{DashboardApp, Focus, RailSel, Section, UiKey};
use crate::tui::hit::{HitTarget, SegmentId};
use crate::tui::modal::{confirm, Modal, PendingAction};
use crate::tui::uimsg::{ActionOrigin, UiMsg};

/// What the event loop should do after handling an event.
pub enum Outcome {
    Continue,
    Quit,
}

pub async fn handle_key(
    app: &mut DashboardApp,
    key: KeyEvent,
    state: &AppState,
    event_handler: &EventHandler,
    status_tx: &mpsc::UnboundedSender<String>,
) -> Outcome {
    app.dirty = true;

    // Modal first: it owns all input while open.
    if app.modal().is_some() {
        return handle_modal_key(app, key, state).await;
    }

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    // Global bindings (parity with the legacy TUI).
    match key.code {
        KeyCode::Char('c') | KeyCode::Char('C') if ctrl => return Outcome::Quit,
        KeyCode::Char('l') | KeyCode::Char('L') if ctrl => {
            let level = app.core.log_level.cycle();
            app.core.set_log_level(level);
            app.push_system(format!("Log level: {}", level.as_str()));
            return Outcome::Continue;
        }
        KeyCode::Char('w') | KeyCode::Char('W') if ctrl => {
            let mode = state.cycle_web_search_mode().await;
            app.status.web_search = format!("{mode:?}").to_uppercase();
            app.push_system(format!("Web search: {mode:?}"));
            return Outcome::Continue;
        }
        KeyCode::Char('h') | KeyCode::Char('H') if ctrl => {
            let mode = state.cycle_event_handler_mode().await;
            app.status.handler_mode = format!("{mode:?}").to_uppercase();
            app.push_system(format!("Handler mode: {mode:?}"));
            return Outcome::Continue;
        }
        KeyCode::Char('e') | KeyCode::Char('E') if ctrl => {
            let (mode, switched) = state.cycle_scripting_mode().await;
            if switched {
                app.status.scripting = mode.as_str().to_string();
                app.push_system(format!("Scripting: {}", mode.as_str()));
            }
            return Outcome::Continue;
        }
        KeyCode::Char('t') | KeyCode::Char('T') if ctrl => {
            app.mouse_capture = !app.mouse_capture;
            app.push_system(if app.mouse_capture {
                "Mouse capture ON"
            } else {
                "Mouse capture OFF — native text selection works; Ctrl-T to re-enable"
            });
            return Outcome::Continue;
        }
        KeyCode::F(1) => {
            app.modals.push(Modal::Help { scroll: 0 });
            return Outcome::Continue;
        }
        KeyCode::Tab if !alt => {
            cycle_focus(app, false);
            return Outcome::Continue;
        }
        KeyCode::BackTab => {
            cycle_focus(app, true);
            return Outcome::Continue;
        }
        _ => {}
    }

    match app.focus.clone() {
        Focus::ChatInput => handle_chat_key(app, key, state, event_handler, status_tx).await,
        Focus::ChatHistory => {
            match key.code {
                KeyCode::Up => app.chat.scroll_up(1),
                KeyCode::Down => app.chat.scroll_down(1),
                KeyCode::PageUp => app.chat.scroll_up(10),
                KeyCode::PageDown => app.chat.scroll_down(10),
                KeyCode::End | KeyCode::Esc => {
                    app.chat.scroll_to_follow();
                    app.focus = Focus::ChatInput;
                }
                _ => {}
            }
            Outcome::Continue
        }
        Focus::Rail(sel) => handle_rail_key(app, key, sel, state).await,
    }
}

fn cycle_focus(app: &mut DashboardApp, _backward: bool) {
    // Two stops now that the rail is a single pane.
    app.focus = match app.focus {
        Focus::Rail(_) => Focus::ChatInput,
        _ => Focus::Rail(RailSel::new()),
    };
    app.clamp_selection();
}

async fn handle_chat_key(
    app: &mut DashboardApp,
    key: KeyEvent,
    state: &AppState,
    event_handler: &EventHandler,
    status_tx: &mpsc::UnboundedSender<String>,
) -> Outcome {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    match key.code {
        KeyCode::Enter if ctrl || alt => {
            app.input.insert_newline();
        }
        KeyCode::Char('n') | KeyCode::Char('N') if ctrl || alt => {
            app.input.insert_newline();
        }
        KeyCode::Enter => {
            let text = app.input.text();
            app.input.clear();
            app.core.slash_suggestions.clear();
            crate::tui::commands::submit(app, text, state, event_handler, status_tx).await;
        }
        KeyCode::PageUp => {
            app.focus = Focus::ChatHistory;
            app.chat.scroll_up(10);
        }
        KeyCode::Up if app.input.is_on_first_line() => history_previous(app),
        KeyCode::Down if app.input.is_on_last_line() => history_next(app),
        KeyCode::Esc => {
            app.input.clear();
            app.core.slash_suggestions.clear();
        }
        _ => {
            app.input.handle_key(key.code, key.modifiers);
        }
    }
    let text = app.input.text();
    app.core.update_slash_suggestions(&text);
    Outcome::Continue
}

/// Command-history navigation, mirroring the legacy TUI: entering history
/// stashes the in-progress input, leaving it restores it.
fn history_previous(app: &mut DashboardApp) {
    use crate::cli::input_state::InputState;
    if app.core.command_history.is_empty() {
        return;
    }
    match app.core.history_position {
        None => {
            let current = app.input.text();
            if !current.is_empty() {
                app.core.history_temp_input = Some(current);
            }
            let pos = app.core.command_history.len() - 1;
            app.core.history_position = Some(pos);
            app.input = InputState::from_lines(
                app.core.command_history[pos]
                    .lines()
                    .map(|s| s.to_string())
                    .collect(),
            );
            app.input.move_to_bottom();
            app.input.move_to_end_of_line();
        }
        Some(pos) if pos > 0 => {
            let new_pos = pos - 1;
            app.core.history_position = Some(new_pos);
            app.input = InputState::from_lines(
                app.core.command_history[new_pos]
                    .lines()
                    .map(|s| s.to_string())
                    .collect(),
            );
            app.input.move_to_bottom();
            app.input.move_to_end_of_line();
        }
        _ => {}
    }
}

fn history_next(app: &mut DashboardApp) {
    use crate::cli::input_state::InputState;
    match app.core.history_position {
        Some(pos) if pos + 1 < app.core.command_history.len() => {
            let new_pos = pos + 1;
            app.core.history_position = Some(new_pos);
            app.input = InputState::from_lines(
                app.core.command_history[new_pos]
                    .lines()
                    .map(|s| s.to_string())
                    .collect(),
            );
            app.input.move_to_bottom();
            app.input.move_to_end_of_line();
        }
        Some(_) => {
            app.core.history_position = None;
            let temp = app.core.history_temp_input.take().unwrap_or_default();
            app.input = InputState::from_lines(temp.lines().map(|s| s.to_string()).collect());
            app.input.move_to_bottom();
            app.input.move_to_end_of_line();
        }
        None => {}
    }
}

async fn handle_rail_key(
    app: &mut DashboardApp,
    key: KeyEvent,
    mut sel: RailSel,
    state: &AppState,
) -> Outcome {
    let rows = crate::tui::render::band::rail_rows(app);
    let owner = sel.row.and_then(|row| rows.get(row)).and_then(|r| r.key);

    match key.code {
        KeyCode::Down => {
            sel.row = Some(match sel.row {
                None => 0,
                Some(row) if row + 1 < rows.len() => row + 1,
                Some(row) => row,
            });
        }
        KeyCode::Up => {
            sel.row = Some(match sel.row {
                None => 0,
                Some(row) => row.saturating_sub(1),
            });
        }
        KeyCode::PageDown => {
            let step = 10;
            sel.row = Some(match sel.row {
                None => step.min(rows.len().saturating_sub(1)),
                Some(row) => (row + step).min(rows.len().saturating_sub(1)),
            });
        }
        KeyCode::PageUp => {
            sel.row = Some(sel.row.unwrap_or(0).saturating_sub(10));
        }
        KeyCode::Home => sel.row = Some(0),
        KeyCode::End => sel.row = Some(rows.len().saturating_sub(1)),
        // Right expands a group or steps into it; Left collapses, or steps out
        // to the parent when there is nothing to collapse.
        KeyCode::Right => {
            if let (Some(key_owner), Some(row)) = (owner, sel.row) {
                if let Some(rail_row) = rows.get(row) {
                    let node = rail_row.row.node.clone();
                    if rail_row.row.expanded == Some(false) {
                        app.rail.band_mut(key_owner).tree.expand(&node);
                    } else if rail_row.row.expanded == Some(true) && row + 1 < rows.len() {
                        sel.row = Some(row + 1);
                    }
                }
            }
        }
        KeyCode::Left => {
            if let (Some(key_owner), Some(row)) = (owner, sel.row) {
                if let Some(rail_row) = rows.get(row) {
                    let node = rail_row.row.node.clone();
                    let depth = rail_row.row.depth;
                    if rail_row.row.expanded == Some(true) {
                        app.rail.band_mut(key_owner).tree.collapse(&node);
                    } else if depth > 0 {
                        if let Some(parent) = rows[..row]
                            .iter()
                            .rposition(|candidate| candidate.row.depth < depth)
                        {
                            sel.row = Some(parent);
                        }
                    }
                }
            }
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            if let Some(row) = sel.row {
                activate_row(app, owner, &rows, row, state).await;
            } else if !rows.is_empty() {
                sel.row = Some(0);
            }
        }
        KeyCode::Esc => {
            app.focus = Focus::ChatInput;
            return Outcome::Continue;
        }
        KeyCode::Char('x') => {
            if let Some(key_owner) = owner {
                stop_instance(app, key_owner, state).await;
            }
        }
        KeyCode::Char('e')
        | KeyCode::Char('r')
        | KeyCode::Char('d')
        | KeyCode::Char('n')
        | KeyCode::Char('c')
        | KeyCode::Char('w')
        | KeyCode::Char('a') => {
            handle_band_shortcut(app, key.code, owner, state).await;
        }
        _ => {}
    }

    if let Focus::Rail(current) = &mut app.focus {
        *current = sel;
    }
    app.clamp_selection();
    Outcome::Continue
}

/// Enter on a tree row: toggle a group, lift a "… N more" cap, or open the
/// editor that owns the row.
async fn activate_row(
    app: &mut DashboardApp,
    key: Option<UiKey>,
    rows: &[crate::tui::render::band::RailRow],
    row: usize,
    state: &AppState,
) {
    use crate::tui::tree::{NodeId, RowAction};

    let Some(rail_row) = rows.get(row) else {
        return;
    };
    let tree_row = &rail_row.row;
    let node = tree_row.node.clone();

    // The rail's own rows own no instance, so they are handled before anything
    // that needs one.
    if let NodeId::NewInstance(section) = node {
        open_protocol_picker(app, section, None, state).await;
        return;
    }
    let Some(key) = key else {
        return;
    };

    // A group toggles; that includes a request, whose detail is its children.
    if tree_row.expanded.is_some() {
        app.rail.band_mut(key).tree.toggle(&node);
        return;
    }

    match node {
        NodeId::More(group) => {
            app.rail.band_mut(key).tree.show_all(&group);
        }
        NodeId::ConfigItem(k, _) => open_editor(app, k),
        // Entering a route edits that route, not the table it sits in — the
        // row you pressed is the one you meant.
        NodeId::Route(k, index) => open_routing_at(app, k, state, RouteTarget::Edit(index)),
        NodeId::Action(k, action) => match action {
            RowAction::EditConfig => open_editor(app, k),
            RowAction::EditRoute(index) => open_routing_at(app, k, state, RouteTarget::Edit(index)),
            RowAction::AddRoute => open_routing_at(app, k, state, RouteTarget::New),
            RowAction::AddClient => {
                if let UiKey::Server(id) = k {
                    open_client_for_server(app, id, state).await;
                }
            }
            RowAction::Send => {
                if let UiKey::Client(id) = k {
                    open_composer(app, id, None, state).await;
                }
            }
            RowAction::SendAction(index) => {
                if let UiKey::Client(id) = k {
                    open_composer(app, id, Some(index), state).await;
                }
            }
            RowAction::MessagePeer(conn_id) => {
                if let UiKey::Server(id) = k {
                    open_peer_composer(app, id, conn_id, state);
                }
            }
            RowAction::DisconnectPeer(conn_id) => {
                if let UiKey::Server(id) = k {
                    disconnect_peer(app, id, conn_id, state);
                }
            }
            RowAction::Disconnect => {
                if let UiKey::Client(id) = k {
                    disconnect_client(app, id, state).await;
                }
            }
            RowAction::Connect => {
                if let UiKey::Client(id) = k {
                    connect_client(app, id, state);
                }
            }
            RowAction::Stop => stop_instance(app, k, state).await,
            RowAction::Wireshark => open_wireshark(app, k),
        },
        NodeId::Intercept(k, intercept_id) => open_intercept(app, k, intercept_id, state),
        NodeId::RoutingFallback(_) | NodeId::RequestDetail(..) => {}
        _ => {}
    }
}

/// Open the answer modal for a pending intercept.
fn open_intercept(app: &mut DashboardApp, key: UiKey, intercept_id: u64, state: &AppState) {
    use crate::tui::modal::intercept::InterceptModel;

    let (protocol, view) = match key {
        UiKey::Server(id) => {
            let Some(row) = app.snapshot.servers.iter().find(|s| s.id == id) else {
                return;
            };
            (
                row.protocol.clone(),
                row.intercepts
                    .iter()
                    .find(|v| v.id == intercept_id)
                    .cloned(),
            )
        }
        UiKey::Client(id) => {
            let Some(row) = app.snapshot.clients.iter().find(|c| c.id == id) else {
                return;
            };
            (
                row.protocol.clone(),
                row.intercepts
                    .iter()
                    .find(|v| v.id == intercept_id)
                    .cloned(),
            )
        }
    };
    let Some(view) = view else {
        app.push_system(format!(
            "request #{intercept_id} is no longer waiting (answered or timed out)"
        ));
        return;
    };
    let (_events, vocabulary) = crate::tui::modal::routing::vocabulary(key, &protocol, state);
    app.modals.push(Modal::Intercept(Box::new(InterceptModel {
        id: view.id,
        owner: key,
        protocol,
        event_type: view.event_type,
        description: view.description,
        event_data: view.event_data,
        vocabulary,
        error: None,
        focused: 0,
    })));
}

/// Which handler the routing editor should open on.
enum RouteTarget {
    /// The table itself, nothing opened.
    List,
    /// Edit an existing handler by index.
    Edit(usize),
    /// Start a new handler.
    New,
}

/// Stop an instance immediately — no confirmation. Stopping is cheap to redo
/// (recreate from the picker) and the dialog was pure friction; only the bulk
/// actions (stop all, quit) keep a confirm.
async fn stop_instance(app: &mut DashboardApp, key: UiKey, state: &AppState) {
    let action = match key {
        UiKey::Server(id) => PendingAction::StopServer(id),
        UiKey::Client(id) => PendingAction::StopClient(id),
    };
    let line = confirm::execute(&action, state).await;
    app.push_system(line);
}

/// Hang up a client's connection, keeping the row for `[ connect ]` later.
async fn disconnect_client(app: &mut DashboardApp, id: crate::state::ClientId, state: &AppState) {
    if state.disconnect_client(id).await {
        app.push_system(format!(
            "client #{} disconnected — [ connect ] re-establishes it",
            id.as_u32()
        ));
    } else {
        app.push_system(format!("client #{} is already gone", id.as_u32()));
    }
}

/// (Re)connect a disconnected client. Spawned: connecting is network I/O and
/// must not block the event loop (see `crate::tui::uimsg`).
fn connect_client(app: &mut DashboardApp, id: crate::state::ClientId, state: &AppState) {
    app.push_system(format!("connecting client #{}…", id.as_u32()));
    let llm = app.llm_client.clone();
    let status_tx = app.status_tx.clone();
    let ui_tx = app.ui_tx.clone();
    let state = state.clone();
    tokio::spawn(async move {
        let message = match crate::cli::client_startup::start_client_by_id(
            &state, id, &llm, &status_tx,
        )
        .await
        {
            Ok(_) => format!("client #{} connected", id.as_u32()),
            Err(e) => format!("client #{} failed to connect: {e}", id.as_u32()),
        };
        let _ = ui_tx.send(UiMsg::Chat(message));
    });
}

async fn handle_band_shortcut(
    app: &mut DashboardApp,
    code: KeyCode,
    key: Option<UiKey>,
    state: &AppState,
) {
    match code {
        // `a` adds a server; a client normally comes from `c` on a server, and
        // the header's [ + client ] covers the standalone case.
        KeyCode::Char('a') => open_protocol_picker(app, Section::Servers, None, state).await,
        KeyCode::Char('e') => {
            if let Some(band_key) = key {
                open_editor(app, band_key);
            }
        }
        KeyCode::Char('r') => {
            if let Some(band_key) = key {
                open_routing(app, band_key, state);
            }
        }
        KeyCode::Char('d') => {
            if let Some(UiKey::Server(id)) = key {
                if let Some(row) = app.snapshot.servers.iter().find(|s| s.id == id) {
                    let protocol = row.protocol.clone();
                    match crate::protocol::server_registry::registry().resolve(&protocol) {
                        Ok(p) => {
                            let text = format!(
                                "{} — {}\n{}",
                                p.protocol_name(),
                                p.description(),
                                p.metadata().summary()
                            );
                            app.push_system(text);
                        }
                        Err(e) => app.push_system(format!("{e}")),
                    }
                }
            }
        }
        KeyCode::Char('c') => {
            if let Some(UiKey::Server(id)) = key {
                open_client_for_server(app, id, state).await;
            }
        }
        KeyCode::Char('n') => {
            if let Some(UiKey::Client(id)) = key {
                open_composer(app, id, None, state).await;
            }
        }
        KeyCode::Char('w') => {
            if let Some(band_key) = key {
                open_wireshark(app, band_key);
            }
        }
        _ => {}
    }
}

/// Open the Wireshark recipe for a running instance, from the snapshot.
///
/// A server's bind host comes from its bound address (the real one, port 0
/// resolved) and the interface from its startup params, where the raw-socket
/// protocols keep it; a client contributes the address it dials.
fn open_wireshark(app: &mut DashboardApp, key: UiKey) {
    use crate::tui::wireshark::{CapturePlan, CaptureTarget, Platform, Role};

    let target = match key {
        UiKey::Server(id) => {
            let Some(row) = app.snapshot.servers.iter().find(|s| s.id == id) else {
                return;
            };
            let bound: Option<std::net::SocketAddr> =
                row.local_addr.as_deref().and_then(|a| a.parse().ok());
            let param = |name: &str| -> Option<String> {
                row.startup_params
                    .as_ref()
                    .and_then(|p| p.get(name))
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            };
            CaptureTarget {
                protocol: row.protocol.clone(),
                role: Role::Server,
                host: bound.map(|a| a.ip().to_string()).or_else(|| param("host")),
                port: bound.map(|a| a.port()).or(Some(row.port)),
                interface: param("interface"),
            }
        }
        UiKey::Client(id) => {
            let Some(row) = app.snapshot.clients.iter().find(|c| c.id == id) else {
                return;
            };
            CaptureTarget::client(&row.protocol, Some(&row.remote_addr))
        }
    };
    let plan = CapturePlan::build(target, Platform::current());
    app.modals.push(Modal::Wireshark {
        plan: Box::new(plan),
        scroll: 0,
    });
}

/// Open the protocol picker for a section. `prefill_remote` aims a new client
/// at a specific address (the `[+ client]` affordance on a server band).
async fn open_protocol_picker(
    app: &mut DashboardApp,
    section: Section,
    prefill_remote: Option<String>,
    state: &AppState,
) {
    let caps = state.get_system_capabilities().await;
    let entries = crate::tui::modal::protocol_picker::entries(section, &caps);
    if entries.is_empty() {
        app.push_system("No protocols compiled into this build for that side.");
        return;
    }
    app.modals.push(Modal::ProtocolPicker {
        section,
        entries,
        filter: String::new(),
        selected: 0,
        prefill_remote,
    });
}

fn open_editor(app: &mut DashboardApp, key: UiKey) {
    use crate::tui::modal::form::FormModel;
    let model = match key {
        UiKey::Server(id) => app
            .snapshot
            .servers
            .iter()
            .find(|s| s.id == id)
            .map(FormModel::for_edit_server),
        UiKey::Client(id) => app
            .snapshot
            .clients
            .iter()
            .find(|c| c.id == id)
            .map(FormModel::for_edit_client),
    };
    if let Some(model) = model {
        app.modals.push(Modal::Form(Box::new(model)));
    }
}

fn open_routing(app: &mut DashboardApp, key: UiKey, state: &AppState) {
    open_routing_at(app, key, state, RouteTarget::List);
}

/// Open the routing editor, optionally landing straight on one handler.
///
/// Going through the table first was a step with nothing in it: you had already
/// pointed at the handler you wanted by pressing Enter on its row.
fn open_routing_at(app: &mut DashboardApp, key: UiKey, state: &AppState, target: RouteTarget) {
    use crate::tui::modal::routing::RoutingModel;
    let model = match key {
        UiKey::Server(id) => app
            .snapshot
            .servers
            .iter()
            .find(|s| s.id == id)
            .map(|row| RoutingModel::new(key, &row.protocol, row.routing.as_ref(), state)),
        UiKey::Client(id) => app
            .snapshot
            .clients
            .iter()
            .find(|c| c.id == id)
            .map(|row| RoutingModel::new(key, &row.protocol, row.routing.as_ref(), state)),
    };
    if let Some(mut model) = model {
        match target {
            RouteTarget::List => {}
            RouteTarget::New => model.add(),
            RouteTarget::Edit(index) => {
                if index < model.handlers.len() {
                    model.selected = index;
                    model.edit_selected();
                }
            }
        }
        app.modals.push(Modal::Routing(Box::new(model)));
    }
}

/// `[ + connect a client ]` on a server: create a client of the counterpart protocol
/// pointed at that very server, so the pair can talk to each other.
async fn open_client_for_server(
    app: &mut DashboardApp,
    server_id: crate::state::ServerId,
    state: &AppState,
) {
    let Some(row) = app.snapshot.servers.iter().find(|s| s.id == server_id) else {
        return;
    };
    let Some(client_protocol) = row.client_counterpart.clone() else {
        app.push_system(format!(
            "{} has no client implementation compiled into this build",
            row.protocol
        ));
        return;
    };
    let port = row
        .local_addr
        .as_ref()
        .and_then(|a| a.rsplit_once(':').and_then(|(_, p)| p.parse::<u16>().ok()))
        .unwrap_or(row.port);
    let remote = format!("127.0.0.1:{port}");

    use crate::tui::modal::form::{FieldTarget, FormModel};
    let mut model = FormModel::for_create(Section::Clients, &client_protocol, None);
    model.set_field_value(&FieldTarget::RemoteAddr, remote.clone());
    model.set_field_value(
        &FieldTarget::Instruction,
        format!(
            "You are a {client_protocol} client connected to our own server #{} at {remote}.",
            server_id.as_u32()
        ),
    );

    // Everything this client needs is known, so connect it rather than showing
    // a form with nothing left to fill in.
    app.push_system(format!(
        "Connecting a {client_protocol} client to {remote}…"
    ));
    let llm = app.llm_client.clone();
    let status_tx = app.status_tx.clone();
    let ui_tx = app.ui_tx.clone();
    let state = state.clone();
    tokio::spawn(async move {
        let result = model
            .apply(&state, llm, &status_tx)
            .await
            .map_err(|e| e.to_string());
        let _ = ui_tx.send(UiMsg::ActionDone {
            origin: ActionOrigin::Form,
            result,
        });
    });
}

/// Compose an action for one live server connection. The vocabulary is the
/// server protocol's sync actions — its wire verbs (send_tcp_data and
/// friends), not its management ones.
fn open_peer_composer(
    app: &mut DashboardApp,
    server_id: crate::state::ServerId,
    connection_id: u32,
    _state: &AppState,
) {
    use crate::tui::modal::composer::ComposerModel;
    let Some(row) = app.snapshot.servers.iter().find(|s| s.id == server_id) else {
        return;
    };
    let Ok(protocol) = crate::protocol::server_registry::registry().resolve(&row.protocol) else {
        app.push_system(format!("{} is not a registered protocol", row.protocol));
        return;
    };
    let actions = protocol.get_sync_actions();
    if actions.is_empty() {
        app.push_system(format!("{} declares no wire actions to send", row.protocol));
        return;
    }
    app.modals
        .push(Modal::Composer(Box::new(ComposerModel::for_peer(
            server_id,
            connection_id,
            &row.protocol,
            actions,
        ))));
}

/// Close one live server connection from the server's side.
///
/// Goes through the peer handle with the protocol's own `close_connection`
/// action, so it is available exactly where `[ message this peer ]` is and
/// runs the same teardown the model's close would. Immediate, like the
/// client's `[ disconnect ]`: only the bulk actions confirm.
fn disconnect_peer(
    app: &mut DashboardApp,
    server_id: crate::state::ServerId,
    connection_id: u32,
    state: &AppState,
) {
    use crate::tui::modal::composer::{ComposerModel, ComposerTarget};
    let target = ComposerTarget::Peer {
        server: server_id,
        connection: connection_id,
    };
    app.push_system(format!("{}: disconnecting…", target.describe()));
    let ui_tx = app.ui_tx.clone();
    let state = state.clone();
    tokio::spawn(async move {
        let message = match ComposerModel::deliver(
            target,
            &state,
            serde_json::json!({"type": "close_connection"}),
        )
        .await
        {
            Ok(outcome) => format!(
                "{}: {}",
                target.describe(),
                crate::tui::modal::composer::describe(&outcome)
            ),
            Err(e) => e.to_string(),
        };
        let _ = ui_tx.send(UiMsg::Chat(message));
    });
}

/// Open the send composer for a client.
///
/// `action_index` selects one of the protocol's actions up front — that is
/// what the inlined `[ send_command ]`-style rows pass, so pressing one lands
/// straight on that action's parameters instead of on a menu. `None` (the `n`
/// shortcut) opens on the action list.
async fn open_composer(
    app: &mut DashboardApp,
    client_id: crate::state::ClientId,
    action_index: Option<usize>,
    state: &AppState,
) {
    let Some(row) = app.snapshot.clients.iter().find(|c| c.id == client_id) else {
        return;
    };
    match row.send_state {
        crate::tui::projection::SendState::Ready => {}
        crate::tui::projection::SendState::NotConnected => {
            app.push_system(format!(
                "client #{} is not connected — nothing to send through",
                client_id.as_u32()
            ));
            return;
        }
        crate::tui::projection::SendState::ProtocolUnsupported => {
            app.push_system(format!(
                "the {} client cannot take injected actions yet: its connection loop has not \
                 adopted the command channel (see src/client/command_support.rs)",
                row.protocol
            ));
            return;
        }
    }
    use crate::tui::modal::composer::ComposerModel;
    let actions = ComposerModel::vocabulary(&row.protocol, state);
    if actions.is_empty() {
        app.push_system(format!("{} declares no client actions", row.protocol));
        return;
    }
    let mut model = ComposerModel::new(client_id, &row.protocol, actions);
    if let Some(index) = action_index {
        // The row's index comes from the same vocabulary call, so it is in
        // range; guard anyway rather than silently opening the wrong action.
        if index < model.actions.len() {
            model.selected = index;
            model.choose();
        }
    }
    app.modals.push(Modal::Composer(Box::new(model)));
}

async fn handle_modal_key(app: &mut DashboardApp, key: KeyEvent, state: &AppState) -> Outcome {
    let is_confirm = matches!(app.modal(), Some(Modal::Confirm { .. }));
    let is_approval = matches!(app.modal(), Some(Modal::WebApproval { .. }));

    if is_approval {
        use crate::state::app_state::WebApprovalResponse;
        let response = match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                Some(WebApprovalResponse::Allow)
            }
            KeyCode::Char('a') | KeyCode::Char('A') => Some(WebApprovalResponse::AlwaysAllow),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                Some(WebApprovalResponse::Deny)
            }
            KeyCode::Char('c') | KeyCode::Char('C')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                Some(WebApprovalResponse::Deny)
            }
            _ => None,
        };
        if let Some(response) = response {
            if let Some(Modal::WebApproval { response_tx, .. }) = app.modals.pop() {
                let _ = response_tx.send(response);
            }
        }
        return Outcome::Continue;
    }

    if is_confirm {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                if let Some(Modal::Confirm { action, .. }) = app.modals.pop() {
                    if action == PendingAction::Quit {
                        return Outcome::Quit;
                    }
                    let line = confirm::execute(&action, state).await;
                    app.push_system(line);
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.modals.pop();
            }
            _ => {}
        }
        return Outcome::Continue;
    }

    match app.modal() {
        Some(Modal::ProtocolPicker { .. }) => return handle_picker_key(app, key, state).await,
        Some(Modal::Form(_)) => return handle_form_key(app, key, state).await,
        Some(Modal::TextEditor { .. }) => return handle_text_editor_key(app, key),
        Some(Modal::Composer(_)) => return handle_composer_key(app, key, state).await,
        Some(Modal::Routing(_)) => return handle_routing_key(app, key, state).await,
        Some(Modal::Intercept(_)) => return handle_intercept_key(app, key, state).await,
        _ => {}
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.modals.pop();
        }
        KeyCode::Up => app.modal_mut().map(|m| m.scroll_by(-1)).unwrap_or(()),
        KeyCode::Down => app.modal_mut().map(|m| m.scroll_by(1)).unwrap_or(()),
        KeyCode::PageUp => app.modal_mut().map(|m| m.scroll_by(-10)).unwrap_or(()),
        KeyCode::PageDown => app.modal_mut().map(|m| m.scroll_by(10)).unwrap_or(()),
        _ => {}
    }
    Outcome::Continue
}

async fn handle_picker_key(app: &mut DashboardApp, key: KeyEvent, state: &AppState) -> Outcome {
    use crate::tui::modal::form::{FieldTarget, FormModel};
    use crate::tui::modal::protocol_picker;

    let Some(Modal::ProtocolPicker {
        section,
        entries,
        filter,
        selected,
        prefill_remote,
    }) = app.modals.last_mut()
    else {
        return Outcome::Continue;
    };

    match key.code {
        KeyCode::Esc => {
            app.modals.pop();
        }
        KeyCode::Up => *selected = selected.saturating_sub(1),
        KeyCode::Down => {
            let count = protocol_picker::filter(entries, filter).len();
            if *selected + 1 < count {
                *selected += 1;
            }
        }
        KeyCode::Backspace => {
            filter.pop();
            *selected = 0;
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            filter.push(c);
            *selected = 0;
        }
        KeyCode::Enter => {
            // Take everything needed out of the picker first, so the borrow on
            // `app.modals` is released before the instance is started.
            let matches = protocol_picker::filter(entries, filter);
            let Some(entry) = matches.get(*selected) else {
                return Outcome::Continue;
            };
            let section = *section;
            let protocol = entry.name.clone();
            let remote = prefill_remote.clone();
            let default_port = if entry.has_binding_defaults {
                entry.default_port
            } else {
                None
            };

            let mut model = FormModel::for_create(section, &protocol, default_port);
            if let Some(remote) = remote {
                model.set_field_value(&FieldTarget::RemoteAddr, remote);
            }
            let missing = model.missing_required();
            app.modals.pop();

            // Picking a protocol starts it immediately on defaults — the point
            // of the picker is "give me one of these", not "fill in a form".
            // Everything stays editable afterwards (`e` config, `r` routing).
            // The form only appears when something genuinely cannot be
            // defaulted, such as a client's remote address.
            if let Some(missing) = missing {
                model.error = Some(format!(
                    "{protocol} needs {missing} before it can start — fill it in and press [ Apply ]"
                ));
                app.modals.push(Modal::Form(Box::new(model)));
                return Outcome::Continue;
            }

            app.push_system(format!("Starting {protocol} on defaults…"));
            let llm = app.llm_client.clone();
            let status_tx = app.status_tx.clone();
            let ui_tx = app.ui_tx.clone();
            let state = state.clone();
            tokio::spawn(async move {
                let result = model
                    .apply(&state, llm, &status_tx)
                    .await
                    .map_err(|e| e.to_string());
                let _ = ui_tx.send(UiMsg::ActionDone {
                    origin: ActionOrigin::Form,
                    result,
                });
            });
        }
        _ => {}
    }
    Outcome::Continue
}

async fn handle_form_key(app: &mut DashboardApp, key: KeyEvent, state: &AppState) -> Outcome {
    use crate::tui::modal::form::FieldTarget;
    use crate::tui::modal::text_editor::TextEditorModel;

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let Some(Modal::Form(form)) = app.modals.last_mut() else {
        return Outcome::Continue;
    };

    // Inline editing of a single-line field.
    if let Some(buffer) = form.editing.as_mut() {
        match key.code {
            KeyCode::Enter => form.commit_edit(),
            KeyCode::Esc => form.cancel_edit(),
            KeyCode::Backspace => {
                buffer.pop();
            }
            KeyCode::Char(c) if !ctrl => buffer.push(c),
            _ => {}
        }
        return Outcome::Continue;
    }

    match key.code {
        KeyCode::Esc => {
            app.modals.pop();
        }
        KeyCode::Tab => form.cycle_focus(false),
        KeyCode::BackTab => form.cycle_focus(true),
        KeyCode::Up => {
            form.focused_button = None;
            form.move_selection(-1);
        }
        KeyCode::Down => {
            form.focused_button = None;
            form.move_selection(1);
        }
        KeyCode::Enter if form.focused_action().is_some() => {
            let action = form.focused_action().unwrap();
            return run_form_action(app, action, state).await;
        }
        KeyCode::Enter => {
            let Some(field) = form.selected_field().cloned() else {
                return Outcome::Continue;
            };
            if field.multiline {
                let json = matches!(field.target, FieldTarget::EventHandlersJson);
                let editor = TextEditorModel::new(&field.label, &field.help, &field.value, json);
                app.modals.push(Modal::TextEditor {
                    editor: Box::new(editor),
                    target: field.target,
                });
            } else if field.target == FieldTarget::SendFirst {
                let toggled = if field.value == "true" {
                    "false"
                } else {
                    "true"
                };
                form.set_field_value(&FieldTarget::SendFirst, toggled.to_string());
            } else {
                form.begin_edit();
            }
        }
        // Typing on a selected field edits it, starting with the character
        // just typed. Requiring Enter first made every field a two-step, and
        // nothing else here wants bare letters.
        KeyCode::Char(c) if !ctrl && form.focused_button.is_none() => {
            if form
                .selected_field()
                .is_some_and(|field| !field.multiline && field.target != FieldTarget::SendFirst)
            {
                form.begin_edit();
                if let Some(buffer) = form.editing.as_mut() {
                    buffer.push(c);
                }
            }
        }
        KeyCode::Backspace if form.focused_button.is_none() => {
            // Same for backspace: start editing and delete, rather than
            // silently doing nothing until Enter is pressed.
            if form
                .selected_field()
                .is_some_and(|field| !field.multiline && field.target != FieldTarget::SendFirst)
            {
                form.begin_edit();
                if let Some(buffer) = form.editing.as_mut() {
                    buffer.pop();
                }
            }
        }
        _ => {}
    }
    Outcome::Continue
}

/// Run one instance-form button. Applying is spawned, never awaited on the
/// event loop: creating a server or connecting a client does network I/O, and
/// awaiting it here froze the whole dashboard until the kernel gave up.
async fn run_form_action(
    app: &mut DashboardApp,
    action: crate::tui::hit::ModalAction,
    state: &AppState,
) -> Outcome {
    use crate::tui::hit::ModalAction;

    let Some(Modal::Form(form)) = app.modals.last_mut() else {
        return Outcome::Continue;
    };
    match action {
        ModalAction::FormCancel => {
            app.modals.pop();
        }
        ModalAction::FormWireshark => {
            // Stacked over the form, so Esc returns to it with nothing lost —
            // the point is to start the capture, then come back and Apply.
            let plan = crate::tui::wireshark::CapturePlan::build(
                form.capture_target(),
                crate::tui::wireshark::Platform::current(),
            );
            app.modals.push(Modal::Wireshark {
                plan: Box::new(plan),
                scroll: 0,
            });
        }
        ModalAction::FormApply => {
            if form.busy {
                return Outcome::Continue;
            }
            form.busy = true;
            form.error = None;
            let model = form.clone();
            let llm = app.llm_client.clone();
            let status_tx = app.status_tx.clone();
            let ui_tx = app.ui_tx.clone();
            let state = state.clone();
            tokio::spawn(async move {
                let result = model
                    .apply(&state, llm, &status_tx)
                    .await
                    .map_err(|e| e.to_string());
                let _ = ui_tx.send(UiMsg::ActionDone {
                    origin: ActionOrigin::Form,
                    result,
                });
            });
        }
        _ => {}
    }
    Outcome::Continue
}

/// The text editor: Tab leaves the text for the `[ Accept ]` / `[ Cancel ]`
/// buttons (and cycles back), Enter presses the focused one, and typing
/// returns to the text. Tab used to insert a tab character here, which left
/// a chord as the only way to accept — the one thing the buttons exist to
/// avoid. Indentation inside the editor is the spacebar's job.
fn handle_text_editor_key(app: &mut DashboardApp, key: KeyEvent) -> Outcome {
    use crate::tui::hit::ModalAction;

    let Some(Modal::TextEditor { editor, .. }) = app.modals.last_mut() else {
        return Outcome::Continue;
    };

    match key.code {
        KeyCode::Esc => {
            app.modals.pop();
        }
        KeyCode::Tab => editor.cycle_focus(false),
        KeyCode::BackTab => editor.cycle_focus(true),
        KeyCode::Enter | KeyCode::Char(' ') if editor.focused_action().is_some() => {
            match editor.focused_action() {
                Some(ModalAction::EditorAccept) => text_editor_accept(app),
                Some(ModalAction::EditorCancel) => {
                    app.modals.pop();
                }
                _ => {}
            }
        }
        _ => {
            editor.focused_button = None;
            editor.textarea.input(tui_textarea::Input::from(key));
        }
    }
    Outcome::Continue
}

/// Accept the text editor's content into whatever opened it. Shared by the
/// focused and the clicked `[ Accept ]` button so the two cannot diverge.
fn text_editor_accept(app: &mut DashboardApp) {
    use crate::tui::modal::form::FieldTarget;

    let Some(Modal::TextEditor { editor, target }) = app.modals.last_mut() else {
        return;
    };
    let Some(text) = editor.accept() else {
        return; // Validation failed; the editor shows why.
    };
    let target = target.clone();
    app.modals.pop();
    match app.modals.last_mut() {
        Some(Modal::Form(form)) => form.set_field_value(&target, text),
        // Opened from a routing draft: the target says which half of the
        // handler body was being edited.
        Some(Modal::Routing(model)) => {
            if let Some(draft) = model.draft.as_mut() {
                match target {
                    FieldTarget::DraftActions | FieldTarget::EventHandlersJson => {
                        match serde_json::from_str::<serde_json::Value>(&text) {
                            Ok(serde_json::Value::Array(actions)) => {
                                draft.actions = actions;
                                draft.error = None;
                            }
                            Ok(_) => {
                                draft.error = Some("response actions must be a JSON array".into())
                            }
                            Err(e) => draft.error = Some(format!("invalid JSON: {e}")),
                        }
                    }
                    FieldTarget::DraftInstruction => draft.instruction = text,
                    _ => draft.code = text,
                }
            }
        }
        _ => {}
    }
}

async fn handle_composer_key(app: &mut DashboardApp, key: KeyEvent, state: &AppState) -> Outcome {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let Some(Modal::Composer(composer)) = app.modals.last_mut() else {
        return Outcome::Continue;
    };

    if let Some(buffer) = composer.editing.as_mut() {
        match key.code {
            KeyCode::Enter => composer.commit_edit(),
            KeyCode::Esc => composer.cancel_edit(),
            KeyCode::Backspace => {
                buffer.pop();
            }
            KeyCode::Char(c) if !ctrl => buffer.push(c),
            _ => {}
        }
        return Outcome::Continue;
    }

    match key.code {
        KeyCode::Esc => {
            if composer.chosen.is_some() {
                composer.back_to_actions();
            } else {
                app.modals.pop();
            }
        }
        KeyCode::Tab => composer.cycle_focus(false),
        KeyCode::BackTab => composer.cycle_focus(true),
        KeyCode::Up => {
            composer.focused_button = None;
            composer.move_selection(-1);
        }
        KeyCode::Down => {
            composer.focused_button = None;
            composer.move_selection(1);
        }
        KeyCode::Enter if composer.focused_action().is_some() => {
            let action = composer.focused_action().unwrap();
            return run_composer_action(app, action, state).await;
        }
        KeyCode::Enter => {
            if composer.chosen.is_some() {
                composer.begin_edit();
            } else {
                composer.choose();
            }
        }
        // Space flips a boolean field, like a checkbox.
        KeyCode::Char(' ')
            if composer.chosen.is_some()
                && composer.raw_json.is_none()
                && composer.focused_button.is_none()
                && composer
                    .selected_field()
                    .is_some_and(|f| f.kind == crate::tui::modal::composer::FieldKind::Bool) =>
        {
            composer.begin_edit();
        }
        // Typing on a parameter field edits it (see the form's note).
        KeyCode::Char(c)
            if !ctrl
                && composer.chosen.is_some()
                && composer.raw_json.is_none()
                && composer.focused_button.is_none()
                && composer
                    .selected_field()
                    .is_some_and(|f| f.kind == crate::tui::modal::composer::FieldKind::Text) =>
        {
            composer.begin_edit();
            if let Some(buffer) = composer.editing.as_mut() {
                buffer.push(c);
            }
        }
        KeyCode::Backspace
            if composer.chosen.is_some()
                && composer.raw_json.is_none()
                && composer.focused_button.is_none()
                && composer
                    .selected_field()
                    .is_some_and(|f| f.kind == crate::tui::modal::composer::FieldKind::Text) =>
        {
            composer.begin_edit();
            if let Some(buffer) = composer.editing.as_mut() {
                buffer.pop();
            }
        }
        _ => {}
    }
    Outcome::Continue
}

/// Run one composer button. Sending is spawned (network I/O must not block
/// the event loop); the toggles act in place.
async fn run_composer_action(
    app: &mut DashboardApp,
    action: crate::tui::hit::ModalAction,
    state: &AppState,
) -> Outcome {
    use crate::tui::hit::ModalAction;

    let Some(Modal::Composer(composer)) = app.modals.last_mut() else {
        return Outcome::Continue;
    };
    match action {
        ModalAction::ComposerBack => composer.back_to_actions(),
        ModalAction::ComposerRaw => composer.toggle_raw_json(),
        ModalAction::ComposerSend => {
            use crate::tui::modal::composer::{ComposerModel, ComposerTarget};

            // Answering a parked request: resolving is a channel send under a
            // short lock, so it happens inline, and both the composer and the
            // question it answered close together.
            if let ComposerTarget::Intercept { id, .. } = composer.target {
                let actions = match composer.build_answer() {
                    Ok(actions) => actions,
                    Err(e) => {
                        composer.error = Some(e.to_string());
                        return Outcome::Continue;
                    }
                };
                let names: Vec<String> = actions
                    .iter()
                    .map(|a| {
                        a.get("type")
                            .and_then(|t| t.as_str())
                            .unwrap_or("(no type)")
                            .to_string()
                    })
                    .collect();
                match state.resolve_intercept(id, actions).await {
                    Ok(()) => {
                        app.modals.pop();
                        if matches!(app.modals.last(), Some(Modal::Intercept(m)) if m.id == id) {
                            app.modals.pop();
                        }
                        app.push_system(format!(
                            "answered request #{id} with {}",
                            names.join(", ")
                        ));
                    }
                    Err(e) => composer.error = Some(e),
                }
                return Outcome::Continue;
            }

            // Validate synchronously: a missing required field is the user's
            // to fix right here, so the composer stays open showing it.
            let action = match composer.build_action() {
                Ok(action) => action,
                Err(e) => {
                    composer.error = Some(e.to_string());
                    return Outcome::Continue;
                }
            };

            // Everything past this point is network work. Close the composer
            // and report asynchronously — it used to sit on "sending…" until
            // the outcome arrived, which for a client whose loop is parked on
            // a MANUAL question meant a frozen-looking modal and then a bare
            // timeout error.
            let target = composer.target;
            let name = action
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("action")
                .to_string();
            app.modals.pop();
            app.push_system(format!("{}: sending {name}…", target.describe()));

            let ui_tx = app.ui_tx.clone();
            let state = state.clone();
            tokio::spawn(async move {
                let message = match ComposerModel::deliver(target, &state, action).await {
                    Ok(outcome) => format!(
                        "{}: {}",
                        target.describe(),
                        crate::tui::modal::composer::describe(&outcome)
                    ),
                    // No target prefix here: send_to_client / send_to_peer
                    // already name what failed, and prefixing produced
                    // "client #2: client #2 did not report…".
                    Err(e) => match ComposerModel::queue_hint(target, &state).await {
                        Some(hint) => format!("{e} — {hint}"),
                        None => e.to_string(),
                    },
                };
                let _ = ui_tx.send(UiMsg::Chat(message));
            });
        }
        _ => {}
    }
    Outcome::Continue
}

/// The intercept modal: three buttons, Tab between them, Enter acts.
async fn handle_intercept_key(app: &mut DashboardApp, key: KeyEvent, state: &AppState) -> Outcome {
    let Some(Modal::Intercept(model)) = app.modals.last_mut() else {
        return Outcome::Continue;
    };
    match key.code {
        // Esc keeps the request waiting — closing the window must not be the
        // thing that silently refuses a peer.
        KeyCode::Esc => {
            app.modals.pop();
        }
        KeyCode::Tab | KeyCode::Right | KeyCode::Down => model.cycle_focus(false),
        KeyCode::BackTab | KeyCode::Left | KeyCode::Up => model.cycle_focus(true),
        KeyCode::Enter | KeyCode::Char(' ') => {
            if let Some(action) = model.focused_action() {
                return run_intercept_action(app, action, state).await;
            }
        }
        _ => {}
    }
    Outcome::Continue
}

/// Run one intercept button: compose, send, or fail closed.
async fn run_intercept_action(
    app: &mut DashboardApp,
    action: crate::tui::hit::ModalAction,
    state: &AppState,
) -> Outcome {
    use crate::tui::hit::ModalAction;
    use crate::tui::modal::composer::ComposerModel;

    let Some(Modal::Intercept(model)) = app.modals.last_mut() else {
        return Outcome::Continue;
    };
    match action {
        // The same composer as the `[ send ]` rows: an action list, then
        // fields. Its Send resolves the intercept (see `run_composer_action`).
        ModalAction::InterceptCompose => {
            if model.vocabulary.is_empty() {
                model.error = Some(format!(
                    "{} declares no actions to answer with — Answer with nothing or Fail closed",
                    model.protocol
                ));
                return Outcome::Continue;
            }
            let composer = ComposerModel::for_intercept(
                model.id,
                model.owner,
                &model.protocol,
                model.vocabulary.clone(),
            );
            app.modals.push(Modal::Composer(Box::new(composer)));
        }
        ModalAction::InterceptSend => {
            // Zero actions is a real answer — "acknowledge, say nothing" — the
            // same semantics as an empty static handler, and exactly what a
            // lifecycle event like connection-opened usually deserves. It is
            // delivered (Ok) and therefore distinct from a timeout (Err).
            let id = model.id;
            // Resolving is a channel send under a short lock — no network I/O,
            // safe to do inline (the waiting dispatcher does the wire work).
            match state.resolve_intercept(id, Vec::new()).await {
                Ok(()) => {
                    app.modals.pop();
                    app.push_system(format!(
                        "answered request #{id} with nothing (acknowledged, no reply sent)"
                    ));
                }
                Err(e) => model.error = Some(e),
            }
        }
        ModalAction::InterceptDismiss => {
            let id = model.id;
            if state.dismiss_intercept(id).await {
                app.modals.pop();
                app.push_system(format!(
                    "refused request #{id} — the peer got the fail-closed reply"
                ));
            } else {
                model.error = Some(format!("request #{id} is no longer waiting"));
            }
        }
        _ => {}
    }
    Outcome::Continue
}

pub async fn handle_mouse(app: &mut DashboardApp, event: MouseEvent, state: &AppState) -> Outcome {
    let target = app.hits.hit(event.column, event.row).cloned();

    match event.kind {
        MouseEventKind::ScrollUp => {
            match target {
                Some(HitTarget::ChatHistory) | Some(HitTarget::ChatInput) => {
                    app.focus = Focus::ChatHistory;
                    app.chat.scroll_up(3);
                }
                Some(HitTarget::ModalBody) | Some(HitTarget::ModalRow(_)) => {
                    if let Some(modal) = app.modal_mut() {
                        modal.scroll_by(-3);
                    }
                }
                Some(HitTarget::TreeRow { .. }) | Some(HitTarget::Band { .. }) => {
                    app.rail.scroll = app.rail.scroll.saturating_sub(3);
                }
                _ => {}
            }
            app.dirty = true;
            return Outcome::Continue;
        }
        MouseEventKind::ScrollDown => {
            match target {
                Some(HitTarget::ChatHistory) | Some(HitTarget::ChatInput) => {
                    app.chat.scroll_down(3);
                }
                Some(HitTarget::ModalBody) | Some(HitTarget::ModalRow(_)) => {
                    if let Some(modal) = app.modal_mut() {
                        modal.scroll_by(3);
                    }
                }
                Some(HitTarget::TreeRow { .. }) | Some(HitTarget::Band { .. }) => {
                    let rows = crate::tui::render::band::rail_row_count(app);
                    app.rail.scroll = (app.rail.scroll + 3).min(rows.saturating_sub(1));
                }
                _ => {}
            }
            app.dirty = true;
            return Outcome::Continue;
        }
        MouseEventKind::Down(MouseButton::Left) => {}
        _ => return Outcome::Continue,
    }

    app.dirty = true;
    let Some(target) = target else {
        return Outcome::Continue;
    };

    // Buttons inside a modal are clickable; every other click in a modal is
    // swallowed so it cannot reach the rail beneath.
    if app.modal().is_some() {
        if let HitTarget::ModalActionButton(action) = target {
            return run_modal_action(app, action, state).await;
        }
        // Clicking a row selects it. The lists inside modals looked
        // interactive and were not: only buttons and the outer body were
        // registered, so a click on a field or a protocol did nothing.
        if let HitTarget::ModalRow(index) = target {
            select_modal_row(app, index);
            return Outcome::Continue;
        }
        return Outcome::Continue;
    }

    match target {
        HitTarget::ChatHistory => app.focus = Focus::ChatHistory,
        HitTarget::ChatInput => app.focus = Focus::ChatInput,
        HitTarget::SectionHeader(_) => app.focus = Focus::Rail(RailSel::new()),
        HitTarget::Band { .. } => app.focus = Focus::Rail(RailSel::new()),
        HitTarget::TreeRow { key, index } => {
            app.focus = Focus::Rail(RailSel { row: Some(index) });
            let rows = crate::tui::render::band::rail_rows(app);
            activate_row(app, key, &rows, index, state).await;
        }
        HitTarget::StatusSegment(segment) => match segment {
            SegmentId::LogLevel => {
                let level = app.core.log_level.cycle();
                app.core.set_log_level(level);
                app.push_system(format!("Log level: {}", level.as_str()));
            }
            SegmentId::WebSearch => {
                let mode = state.cycle_web_search_mode().await;
                app.status.web_search = format!("{mode:?}").to_uppercase();
            }
            SegmentId::Handler => {
                let mode = state.cycle_event_handler_mode().await;
                app.status.handler_mode = format!("{mode:?}").to_uppercase();
            }
            SegmentId::Scripting => {
                let (mode, switched) = state.cycle_scripting_mode().await;
                if switched {
                    app.status.scripting = mode.as_str().to_string();
                }
            }
            SegmentId::Help => app.modals.push(Modal::Help { scroll: 0 }),
            SegmentId::Model | SegmentId::Backend | SegmentId::Usage => {
                let lines = crate::tui::command_exec::execute(
                    crate::events::UserCommand::ShowUsage,
                    state,
                    &dummy_channel(),
                )
                .await;
                for line in lines {
                    app.push_system(line);
                }
            }
        },
        HitTarget::ModalBody | HitTarget::ModalRow(_) | HitTarget::ModalButton(_) => {}
        HitTarget::ModalActionButton(_) => {}
    }
    Outcome::Continue
}

async fn handle_routing_key(app: &mut DashboardApp, key: KeyEvent, state: &AppState) -> Outcome {
    use crate::tui::modal::routing::DraftFocus;
    use crate::tui::modal::text_editor::TextEditorModel;

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let Some(Modal::Routing(model)) = app.modals.last_mut() else {
        return Outcome::Continue;
    };

    // Handler draft open. Tab walks kind → pattern → the kind's fields →
    // buttons; ←/→ changes whatever choice has focus; Enter acts on it.
    if let Some(draft) = model.draft.as_mut() {
        if let Some(buffer) = draft.editing.as_mut() {
            match key.code {
                KeyCode::Enter => {
                    let text = buffer.clone();
                    draft.editing = None;
                    match draft.focus {
                        DraftFocus::Pattern => draft.pattern = text,
                        DraftFocus::Timeout => draft.timeout_secs = text,
                        _ => {}
                    }
                }
                KeyCode::Esc => draft.editing = None,
                KeyCode::Backspace => {
                    buffer.pop();
                }
                KeyCode::Char(c) if !ctrl => buffer.push(c),
                _ => {}
            }
            return Outcome::Continue;
        }

        let backward_left = key.code == KeyCode::Left;
        match key.code {
            KeyCode::Esc => model.draft = None,
            KeyCode::Tab => draft.cycle_focus(false),
            KeyCode::BackTab => draft.cycle_focus(true),
            KeyCode::Left | KeyCode::Right => match draft.focus {
                DraftFocus::Kind => {
                    let kind = if backward_left {
                        draft.kind.previous()
                    } else {
                        draft.kind.next()
                    };
                    draft.set_kind(kind);
                }
                DraftFocus::Pattern => {
                    let event_ids = model.event_ids.clone();
                    if let Some(draft) = model.draft.as_mut() {
                        draft.cycle_pattern(&event_ids, backward_left);
                    }
                }
                DraftFocus::Language => draft.cycle_language(backward_left),
                DraftFocus::Resident => draft.resident = !draft.resident,
                DraftFocus::Button(_) => draft.cycle_focus(backward_left),
                _ => {}
            },
            KeyCode::Up if draft.focus == DraftFocus::Actions => {
                draft.selected_action = draft.selected_action.saturating_sub(1);
            }
            KeyCode::Down if draft.focus == DraftFocus::Actions => {
                if draft.selected_action + 1 < draft.actions.len() {
                    draft.selected_action += 1;
                }
            }
            KeyCode::Char('d') if draft.focus == DraftFocus::Actions => {
                if draft.selected_action < draft.actions.len() {
                    draft.actions.remove(draft.selected_action);
                    draft.selected_action = draft.selected_action.saturating_sub(1);
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                use crate::tui::modal::form::FieldTarget;
                match draft.focus {
                    DraftFocus::Button(_) => match draft.focused_action() {
                        Some(crate::tui::hit::ModalAction::DraftSave) => {
                            match model.commit_draft() {
                                Ok(()) => model.error = None,
                                Err(e) => {
                                    if let Some(draft) = model.draft.as_mut() {
                                        draft.error = Some(e.to_string());
                                    }
                                }
                            }
                        }
                        Some(crate::tui::hit::ModalAction::DraftCancel) => model.draft = None,
                        _ => {}
                    },
                    DraftFocus::Kind => draft.set_kind(draft.kind.next()),
                    DraftFocus::Pattern => draft.editing = Some(draft.pattern.clone()),
                    DraftFocus::Language => draft.cycle_language(false),
                    DraftFocus::Resident => draft.resident = !draft.resident,
                    DraftFocus::Timeout => draft.editing = Some(draft.timeout_secs.clone()),
                    DraftFocus::Instruction => {
                        let editor = TextEditorModel::new(
                            "per-event instruction",
                            "What the model should do when this event arrives.",
                            &draft.instruction,
                            false,
                        );
                        app.modals.push(Modal::TextEditor {
                            editor: Box::new(editor),
                            target: FieldTarget::DraftInstruction,
                        });
                    }
                    DraftFocus::Code => {
                        let editor = TextEditorModel::new(
                            "script code",
                            "The script receives the event on stdin and writes {\"actions\": [...]}.",
                            &draft.code,
                            false,
                        );
                        app.modals.push(Modal::TextEditor {
                            editor: Box::new(editor),
                            target: FieldTarget::DraftCode,
                        });
                    }
                    DraftFocus::Actions => {
                        let initial = if draft.actions.is_empty() {
                            example_actions(model_actions(model))
                        } else {
                            serde_json::to_string_pretty(&draft.actions).unwrap_or_default()
                        };
                        let editor = TextEditorModel::new(
                            "response actions",
                            "A JSON array of actions. {{event.field}} interpolates from the event.",
                            &initial,
                            true,
                        );
                        app.modals.push(Modal::TextEditor {
                            editor: Box::new(editor),
                            target: FieldTarget::DraftActions,
                        });
                    }
                }
            }
            // Typing on the two free-text fields edits them in place.
            KeyCode::Char(c) if !ctrl => match draft.focus {
                DraftFocus::Pattern => {
                    draft.editing = Some(format!("{}{c}", draft.pattern));
                }
                DraftFocus::Timeout => {
                    draft.editing = Some(format!("{}{c}", draft.timeout_secs));
                }
                _ => {}
            },
            KeyCode::Backspace => match draft.focus {
                DraftFocus::Pattern => {
                    let mut text = draft.pattern.clone();
                    text.pop();
                    draft.editing = Some(text);
                }
                DraftFocus::Timeout => {
                    let mut text = draft.timeout_secs.clone();
                    text.pop();
                    draft.editing = Some(text);
                }
                _ => {}
            },
            _ => {}
        }
        return Outcome::Continue;
    }

    // Handler list and buttons. Tab moves between them; Enter activates what
    // has focus. The letter shortcuts still work, but nothing depends on them.
    use crate::tui::modal::routing::RoutingFocus;

    match key.code {
        KeyCode::Tab => model.cycle_focus(false),
        KeyCode::BackTab => model.cycle_focus(true),
        KeyCode::Esc => {
            app.modals.pop();
        }
        KeyCode::Up if model.focus == RoutingFocus::List => model.move_selection(-1),
        KeyCode::Down if model.focus == RoutingFocus::List => model.move_selection(1),
        KeyCode::Left | KeyCode::Right => model.cycle_focus(key.code == KeyCode::Left),
        KeyCode::Enter => match model.focused_button() {
            Some(action) => return run_routing_action(app, action, state).await,
            None => model.edit_selected(),
        },
        // Kept as accelerators for anyone who wants them.
        KeyCode::Char('a') => model.add(),
        KeyCode::Char('e') => model.edit_selected(),
        KeyCode::Char('d') => model.delete_selected(),
        KeyCode::Char('K') => model.reorder(-1),
        KeyCode::Char('J') => model.reorder(1),
        _ => {}
    }
    Outcome::Continue
}

/// Run one routing-editor button.
async fn run_routing_action(
    app: &mut DashboardApp,
    action: crate::tui::hit::ModalAction,
    state: &AppState,
) -> Outcome {
    use crate::tui::hit::ModalAction;

    let Some(Modal::Routing(model)) = app.modals.last_mut() else {
        return Outcome::Continue;
    };
    match action {
        ModalAction::RoutingAdd => model.add(),
        ModalAction::RoutingEdit => model.edit_selected(),
        ModalAction::RoutingDelete => model.delete_selected(),
        ModalAction::RoutingMoveUp => model.reorder(-1),
        ModalAction::RoutingMoveDown => model.reorder(1),
        ModalAction::RoutingCancel => {
            app.modals.pop();
        }
        ModalAction::RoutingSave => {
            if model.busy {
                return Outcome::Continue;
            }
            model.busy = true;
            model.error = None;
            let snapshot = model.clone();
            let llm = app.llm_client.clone();
            let status_tx = app.status_tx.clone();
            let ui_tx = app.ui_tx.clone();
            let state = state.clone();
            tokio::spawn(async move {
                let result = snapshot
                    .apply(&state, llm, &status_tx)
                    .await
                    .map_err(|e| e.to_string());
                let _ = ui_tx.send(UiMsg::ActionDone {
                    origin: ActionOrigin::Routing,
                    result,
                });
            });
        }
        // Draft/form actions are handled by their own modals.
        _ => {}
    }
    Outcome::Continue
}

fn model_actions(
    model: &crate::tui::modal::routing::RoutingModel,
) -> &[crate::llm::actions::ActionDefinition] {
    &model.actions
}

/// A starter JSON array using the protocol's first action as a template, so
/// the editor opens with something valid rather than a blank page.
fn example_actions(actions: &[crate::llm::actions::ActionDefinition]) -> String {
    match actions.first() {
        Some(action) => {
            let mut example = action.example.clone();
            if example.get("type").is_none() {
                if let Some(obj) = example.as_object_mut() {
                    obj.insert(
                        "type".to_string(),
                        serde_json::Value::String(action.name.clone()),
                    );
                }
            }
            serde_json::to_string_pretty(&serde_json::Value::Array(vec![example]))
                .unwrap_or_else(|_| "[]".to_string())
        }
        None => "[]".to_string(),
    }
}

/// Point the open modal's selection at the row that was clicked.
///
/// Selection only — a click never *acts*, so a mis-click cannot send a
/// request or delete a handler. Enter (or the buttons) still does that.
fn select_modal_row(app: &mut DashboardApp, index: usize) {
    match app.modals.last_mut() {
        Some(Modal::Form(form)) => {
            if index < form.fields.len() {
                form.focused_button = None;
                form.selected = index;
            }
        }
        Some(Modal::Composer(composer)) => {
            let len = if composer.chosen.is_some() {
                composer.fields.len()
            } else {
                composer.actions.len()
            };
            if index < len {
                composer.focused_button = None;
                composer.selected = index;
            }
        }
        Some(Modal::Routing(model)) => {
            if model.draft.is_none() && index < model.handlers.len() {
                model.focus = crate::tui::modal::routing::RoutingFocus::List;
                model.selected = index;
            }
        }
        Some(Modal::ProtocolPicker { selected, .. }) => *selected = index,
        _ => {}
    }
}

/// Dispatch a modal button to the editor that owns it.
pub async fn run_modal_action(
    app: &mut DashboardApp,
    action: crate::tui::hit::ModalAction,
    state: &AppState,
) -> Outcome {
    use crate::tui::hit::ModalAction;
    match action {
        ModalAction::FormApply | ModalAction::FormCancel | ModalAction::FormWireshark => {
            run_form_action(app, action, state).await
        }
        ModalAction::DraftSave => {
            if let Some(Modal::Routing(model)) = app.modals.last_mut() {
                match model.commit_draft() {
                    Ok(()) => model.error = None,
                    Err(e) => {
                        if let Some(draft) = model.draft.as_mut() {
                            draft.error = Some(e.to_string());
                        }
                    }
                }
            }
            Outcome::Continue
        }
        ModalAction::DraftCancel => {
            if let Some(Modal::Routing(model)) = app.modals.last_mut() {
                model.draft = None;
            }
            Outcome::Continue
        }
        ModalAction::DraftKind(kind) => {
            if let Some(Modal::Routing(model)) = app.modals.last_mut() {
                if let Some(draft) = model.draft.as_mut() {
                    draft.set_kind(kind);
                    draft.focus = crate::tui::modal::routing::DraftFocus::Kind;
                }
            }
            Outcome::Continue
        }
        ModalAction::InterceptCompose
        | ModalAction::InterceptSend
        | ModalAction::InterceptDismiss => run_intercept_action(app, action, state).await,
        ModalAction::ComposerSend | ModalAction::ComposerRaw | ModalAction::ComposerBack => {
            run_composer_action(app, action, state).await
        }
        ModalAction::EditorAccept => {
            text_editor_accept(app);
            Outcome::Continue
        }
        ModalAction::EditorCancel => {
            if matches!(app.modals.last(), Some(Modal::TextEditor { .. })) {
                app.modals.pop();
            }
            Outcome::Continue
        }
        ModalAction::ConfirmYes => {
            if let Some(Modal::Confirm { action, .. }) = app.modals.pop() {
                if action == PendingAction::Quit {
                    return Outcome::Quit;
                }
                let line = confirm::execute(&action, state).await;
                app.push_system(line);
            }
            Outcome::Continue
        }
        ModalAction::ConfirmNo => {
            if matches!(app.modals.last(), Some(Modal::Confirm { .. })) {
                app.modals.pop();
            }
            Outcome::Continue
        }
        _ => run_routing_action(app, action, state).await,
    }
}

/// Fold the result of a spawned action back into the UI: on success the
/// originating modal closes and the summary goes to chat; on failure the modal
/// stays open showing the error, so the user can fix and retry.
pub fn handle_ui_msg(app: &mut DashboardApp, msg: UiMsg) {
    app.dirty = true;
    let (origin, result) = match msg {
        UiMsg::Chat(text) => {
            app.push_system(text);
            return;
        }
        UiMsg::ActionDone { origin, result } => (origin, result),
    };

    let matches_origin = match (origin, app.modal()) {
        (ActionOrigin::Form, Some(Modal::Form(_))) => true,
        (ActionOrigin::Routing, Some(Modal::Routing(_))) => true,
        _ => false,
    };

    match result {
        Ok(summary) => {
            if matches_origin {
                app.modals.pop();
            }
            app.push_system(summary);
        }
        Err(error) => {
            if matches_origin {
                match app.modals.last_mut() {
                    Some(Modal::Form(form)) => {
                        form.busy = false;
                        form.error = Some(error);
                    }
                    Some(Modal::Routing(model)) => {
                        model.busy = false;
                        model.error = Some(error);
                    }
                    Some(Modal::Composer(composer)) => {
                        composer.busy = false;
                        composer.error = Some(error);
                    }
                    _ => {}
                }
            } else {
                // The user moved on; the failure still has to be visible.
                app.push_system(format!("✗ {error}"));
            }
        }
    }
}

/// A detached sender for command paths that only need the return value.
fn dummy_channel() -> mpsc::UnboundedSender<String> {
    let (tx, _rx) = mpsc::unbounded_channel();
    tx
}
