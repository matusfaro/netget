//! Keyboard and mouse handling.
//!
//! Precedence: an open modal owns everything; then the global toggles (which
//! keep their legacy bindings for parity); then focus-specific handling.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use tokio::sync::mpsc;

use crate::events::EventHandler;
use crate::state::app_state::AppState;
use crate::tui::app::{DashboardApp, Focus, PaneKind, RailSel, Section, UiKey};
use crate::tui::hit::{ButtonId, HitTarget, SegmentId};
use crate::tui::modal::{confirm, Modal, PendingAction};
use crate::tui::render::band::pane_row_count;

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

fn cycle_focus(app: &mut DashboardApp, backward: bool) {
    let order = [
        Focus::ChatInput,
        Focus::Rail(RailSel::new(Section::Servers)),
        Focus::Rail(RailSel::new(Section::Clients)),
    ];
    let current = match &app.focus {
        Focus::ChatInput | Focus::ChatHistory => 0,
        Focus::Rail(sel) => match sel.section {
            Section::Servers => 1,
            Section::Clients => 2,
        },
    };
    let next = if backward {
        (current + order.len() - 1) % order.len()
    } else {
        (current + 1) % order.len()
    };
    app.focus = order[next].clone();
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
    let count = app.band_count(sel.section);
    let key_of_band = app.band_key(sel.section, sel.band);

    match key.code {
        KeyCode::Up => match (sel.pane, sel.row) {
            (Some(pane), Some(row)) => {
                let _ = pane;
                sel.row = Some(row.saturating_sub(1));
            }
            _ => {
                if sel.band > 0 {
                    sel.band -= 1;
                }
            }
        },
        KeyCode::Down => match (sel.pane, sel.row) {
            (Some(pane), Some(row)) => {
                let max = key_of_band
                    .map(|k| pane_row_count(app, k, pane))
                    .unwrap_or(0);
                if row + 1 < max {
                    sel.row = Some(row + 1);
                }
            }
            _ => {
                if sel.band + 1 < count {
                    sel.band += 1;
                }
            }
        },
        KeyCode::Left => {
            sel.row = None;
            sel.pane = match sel.pane {
                None => Some(PaneKind::Requests),
                Some(pane) => {
                    let idx = PaneKind::ALL.iter().position(|p| *p == pane).unwrap_or(0);
                    if idx == 0 {
                        None
                    } else {
                        Some(PaneKind::ALL[idx - 1])
                    }
                }
            };
        }
        KeyCode::Right => {
            sel.row = None;
            sel.pane = match sel.pane {
                None => Some(PaneKind::Info),
                Some(pane) => {
                    let idx = PaneKind::ALL.iter().position(|p| *p == pane).unwrap_or(0);
                    if idx + 1 >= PaneKind::ALL.len() {
                        None
                    } else {
                        Some(PaneKind::ALL[idx + 1])
                    }
                }
            };
        }
        KeyCode::Enter => match (sel.pane, sel.row) {
            (Some(_), None) => sel.row = Some(0),
            (Some(pane), Some(row)) => {
                if let Some(band_key) = key_of_band {
                    open_row_detail(app, band_key, pane, row);
                }
            }
            (None, _) => {
                if let Some(band_key) = key_of_band {
                    app.modals.push(Modal::BandDetail {
                        key: band_key,
                        scroll: 0,
                    });
                }
            }
        },
        KeyCode::Esc => {
            if sel.row.is_some() {
                sel.row = None;
            } else if sel.pane.is_some() {
                sel.pane = None;
            } else {
                app.focus = Focus::ChatInput;
                return Outcome::Continue;
            }
        }
        KeyCode::Char(' ') => {
            if let Some(band_key) = key_of_band {
                let band = app.rail.band_mut(band_key);
                band.maximized = !band.maximized;
            }
        }
        KeyCode::Char('x') => {
            if let Some(band_key) = key_of_band {
                push_stop_confirm(app, band_key);
            }
        }
        KeyCode::Char('e') | KeyCode::Char('r') | KeyCode::Char('d') | KeyCode::Char('n')
        | KeyCode::Char('c') | KeyCode::Char('a') => {
            handle_band_shortcut(app, key.code, sel.section, key_of_band, state).await;
        }
        _ => {}
    }

    if let Focus::Rail(current) = &mut app.focus {
        *current = sel;
    }
    app.clamp_selection();
    Outcome::Continue
}

fn open_row_detail(app: &mut DashboardApp, key: UiKey, pane: PaneKind, row: usize) {
    if pane != PaneKind::Requests {
        app.modals.push(Modal::BandDetail { key, scroll: 0 });
        return;
    }
    // Requests pane: the row indexes into that band's request list, offset by
    // the client's leading [send] row.
    let entry = match key {
        UiKey::Server(id) => app
            .snapshot
            .servers
            .iter()
            .find(|s| s.id == id)
            .and_then(|s| s.requests.get(row))
            .cloned(),
        UiKey::Client(id) => app
            .snapshot
            .clients
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| c.requests.get(row.saturating_sub(1)))
            .cloned(),
    };
    if let Some(entry) = entry {
        app.modals.push(Modal::RequestDetail {
            entry: Box::new(entry),
            scroll: 0,
        });
    }
}

fn push_stop_confirm(app: &mut DashboardApp, key: UiKey) {
    let (message, action) = match key {
        UiKey::Server(id) => (
            format!("Stop server #{}? Live connections will be dropped.", id.as_u32()),
            PendingAction::StopServer(id),
        ),
        UiKey::Client(id) => (
            format!("Stop client #{}?", id.as_u32()),
            PendingAction::StopClient(id),
        ),
    };
    app.modals.push(Modal::Confirm { message, action });
}

async fn handle_band_shortcut(
    app: &mut DashboardApp,
    code: KeyCode,
    section: Section,
    key: Option<UiKey>,
    _state: &AppState,
) {
    match code {
        KeyCode::Char('a') => app.push_system(format!(
            "Adding a {} interactively is coming in the next build stage; \
             for now ask the LLM in chat.",
            match section {
                Section::Servers => "server",
                Section::Clients => "client",
            }
        )),
        KeyCode::Char('e') => match key {
            Some(band_key) => app.modals.push(Modal::BandDetail {
                key: band_key,
                scroll: 0,
            }),
            None => {}
        },
        KeyCode::Char('r') => match key {
            Some(band_key) => app.modals.push(Modal::BandDetail {
                key: band_key,
                scroll: 0,
            }),
            None => {}
        },
        KeyCode::Char('d') => {
            if let Some(UiKey::Server(id)) = key {
                if let Some(row) = app.snapshot.servers.iter().find(|s| s.id == id) {
                    let protocol = row.protocol.clone();
                    match crate::protocol::server_registry::registry().resolve(&protocol) {
                        Ok(p) => {
                            let text =
                                format!("{} — {}\n{}", p.protocol_name(), p.description(), p.metadata().summary());
                            app.push_system(text);
                        }
                        Err(e) => app.push_system(format!("{e}")),
                    }
                }
            }
        }
        KeyCode::Char('c') => app.push_system(
            "Connecting a client to this server interactively is coming in the next build stage."
                .to_string(),
        ),
        KeyCode::Char('n') => app.push_system(
            "The send composer is coming in the next build stage.".to_string(),
        ),
        _ => {}
    }
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

    // A click anywhere in the modal keeps focus there; only Esc closes it.
    if app.modal().is_some() {
        return Outcome::Continue;
    }

    match target {
        HitTarget::ChatHistory => app.focus = Focus::ChatHistory,
        HitTarget::ChatInput => app.focus = Focus::ChatInput,
        HitTarget::SectionHeader(section) => app.focus = Focus::Rail(RailSel::new(section)),
        HitTarget::AddButton(section) => {
            app.focus = Focus::Rail(RailSel::new(section));
            handle_band_shortcut(app, KeyCode::Char('a'), section, None, state).await;
        }
        HitTarget::Band { key } => {
            if let Some((section, band)) = app.locate(key) {
                app.focus = Focus::Rail(RailSel {
                    section,
                    band,
                    pane: None,
                    row: None,
                });
            }
        }
        HitTarget::Pane { key, pane } => {
            if let Some((section, band)) = app.locate(key) {
                app.focus = Focus::Rail(RailSel {
                    section,
                    band,
                    pane: Some(pane),
                    row: None,
                });
            }
        }
        HitTarget::Row { key, pane, row } => {
            if let Some((section, band)) = app.locate(key) {
                app.focus = Focus::Rail(RailSel {
                    section,
                    band,
                    pane: Some(pane),
                    row: Some(row),
                });
                open_row_detail(app, key, pane, row);
            }
        }
        HitTarget::Button(button) => match button {
            ButtonId::Stop(key) => push_stop_confirm(app, key),
            ButtonId::StopAll => app.modals.push(Modal::Confirm {
                message: "Stop every running server and client?".to_string(),
                action: PendingAction::StopAll,
            }),
            ButtonId::Edit(key) | ButtonId::Routing(key) => {
                app.modals.push(Modal::BandDetail { key, scroll: 0 })
            }
            ButtonId::AddClientFor(_) => app.push_system(
                "Connecting a client to this server interactively is coming in the next build stage."
                    .to_string(),
            ),
            ButtonId::Send(_) => {
                app.push_system("The send composer is coming in the next build stage.".to_string())
            }
        },
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
    }
    Outcome::Continue
}

/// A detached sender for command paths that only need the return value.
fn dummy_channel() -> mpsc::UnboundedSender<String> {
    let (tx, _rx) = mpsc::unbounded_channel();
    tx
}
