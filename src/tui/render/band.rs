//! One instance band: a title row plus vertical sub-panes
//! (info | config | routing | peers | requests).

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::scripting::EventHandlerType;
use crate::state::client::ClientStatus;
use crate::state::server::ServerStatus;
use crate::tui::app::{DashboardApp, Focus, PaneKind, Section, UiKey};
use crate::tui::bands::BandLayout;
use crate::tui::hit::{ButtonId, HitTarget};
use crate::tui::modal::request_detail::summary_line;
use crate::tui::projection::{ClientRow, ServerRow};

/// Narrow rails drop panes in this order (least → most essential).
const DROP_ORDER: [PaneKind; 3] = [PaneKind::Connections, PaneKind::Config, PaneKind::Routing];
const MIN_PANE_WIDTH: u16 = 14;

pub fn draw(
    frame: &mut Frame,
    app: &mut DashboardApp,
    area: Rect,
    section: Section,
    index: usize,
    layout: BandLayout,
    selected: bool,
) {
    let Some(key) = app.band_key(section, index) else {
        return;
    };
    app.hits.push(area, HitTarget::Band { key });

    // Collapsed bands are a single summary line with no box — a border would
    // cost the only row they have.
    if layout.collapsed || area.height <= 2 {
        let title_area = Rect {
            height: 1,
            ..area
        };
        frame.render_widget(Paragraph::new(title_line(app, key, selected)), title_area);
        return;
    }

    // Each instance gets its own framed pane, so two servers never read as one
    // continuous list.
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if selected {
            app.styles.accent
        } else {
            app.styles.separator
        })
        .title(title_line(app, key, selected));
    let body = block.inner(area);
    frame.render_widget(block, area);

    if body.height == 0 {
        return;
    }
    draw_panes(frame, app, body, key, selected);
}

fn status_style(app: &DashboardApp, key: UiKey) -> (String, Style) {
    match key {
        UiKey::Server(id) => {
            let Some(row) = app.snapshot.servers.iter().find(|s| s.id == id) else {
                return (String::new(), app.styles.dimmed);
            };
            match &row.status {
                ServerStatus::Running => ("RUNNING".into(), app.styles.success),
                ServerStatus::Starting => ("STARTING".into(), app.styles.warning),
                ServerStatus::Stopped => ("STOPPED".into(), app.styles.dimmed),
                ServerStatus::Error(e) => (
                    format!("ERROR: {}", crate::utils::truncate_for_log(e, 40)),
                    app.styles.error,
                ),
            }
        }
        UiKey::Client(id) => {
            let Some(row) = app.snapshot.clients.iter().find(|c| c.id == id) else {
                return (String::new(), app.styles.dimmed);
            };
            match &row.status {
                ClientStatus::Connected => ("CONNECTED".into(), app.styles.success),
                ClientStatus::Connecting => ("CONNECTING".into(), app.styles.warning),
                ClientStatus::Disconnected => ("DISCONNECTED".into(), app.styles.dimmed),
                ClientStatus::Error(e) => (
                    format!("ERROR: {}", crate::utils::truncate_for_log(e, 40)),
                    app.styles.error,
                ),
            }
        }
    }
}

fn title_line<'a>(app: &DashboardApp, key: UiKey, selected: bool) -> Line<'a> {
    let (status_text, status_style) = status_style(app, key);
    let name = match key {
        UiKey::Server(id) => app
            .snapshot
            .servers
            .iter()
            .find(|s| s.id == id)
            .map(|r| {
                let addr = r
                    .local_addr
                    .clone()
                    .unwrap_or_else(|| format!(":{}", r.port));
                format!("#{} {} {}", id.as_u32(), r.protocol, addr)
            })
            .unwrap_or_default(),
        UiKey::Client(id) => app
            .snapshot
            .clients
            .iter()
            .find(|c| c.id == id)
            .map(|r| format!("#{} {} → {}", id.as_u32(), r.protocol, r.remote_addr))
            .unwrap_or_default(),
    };

    let name_style = if selected {
        app.styles.accent
    } else {
        app.styles.title
    };
    Line::from(vec![
        Span::styled(format!(" {name} "), name_style),
        Span::styled(format!("{status_text} "), status_style),
    ])
}

/// Count shown next to a pane's name, so the band spends no rows on numbers
/// the header can carry.
fn pane_count(app: &DashboardApp, key: UiKey, pane: PaneKind) -> Option<usize> {
    match key {
        UiKey::Server(id) => {
            let row = app.snapshot.servers.iter().find(|s| s.id == id)?;
            match pane {
                PaneKind::Connections => Some(row.conns.len()),
                PaneKind::Requests => Some(row.requests.len()),
                PaneKind::Routing => Some(row.routing.as_ref().map_or(0, |r| r.handlers.len())),
                PaneKind::Config => Some(row.task_count),
                PaneKind::Info => None,
            }
        }
        UiKey::Client(id) => {
            let row = app.snapshot.clients.iter().find(|c| c.id == id)?;
            match pane {
                // Most client protocols never populate `connection` — status is
                // the honest signal for whether a peer is attached.
                PaneKind::Connections => Some(usize::from(
                    row.connection.is_some() || row.status == ClientStatus::Connected,
                )),
                PaneKind::Requests => Some(row.requests.len()),
                PaneKind::Routing => Some(row.routing.as_ref().map_or(0, |r| r.handlers.len())),
                PaneKind::Config => Some(row.task_count),
                PaneKind::Info => None,
            }
        }
    }
}

/// Which panes fit at this width, in display order.
fn visible_panes(width: u16) -> Vec<PaneKind> {
    let mut panes: Vec<PaneKind> = PaneKind::ALL.to_vec();
    for drop in DROP_ORDER {
        if panes.len() as u16 * MIN_PANE_WIDTH <= width {
            break;
        }
        panes.retain(|p| *p != drop);
    }
    panes
}

fn draw_panes(frame: &mut Frame, app: &mut DashboardApp, area: Rect, key: UiKey, selected: bool) {
    let panes = visible_panes(area.width);
    if panes.is_empty() {
        return;
    }
    let total_percent: u16 = panes.iter().map(|p| p.width_percent()).sum();
    let constraints: Vec<Constraint> = panes
        .iter()
        .map(|p| Constraint::Ratio(p.width_percent() as u32, total_percent as u32))
        .collect();
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    let focused_pane = match &app.focus {
        Focus::Rail(sel) if selected => sel.pane,
        _ => None,
    };
    let focused_row = match &app.focus {
        Focus::Rail(sel) if selected => sel.row,
        _ => None,
    };

    for (pane, chunk) in panes.iter().zip(chunks.iter()) {
        let is_focused = focused_pane == Some(*pane);
        let border_style = if is_focused {
            app.styles.accent
        } else {
            app.styles.separator
        };
        let heading = match pane_count(app, key, *pane) {
            Some(count) => format!(" {} ({})", pane.title(), count),
            None => format!(" {}", pane.title()),
        };
        let block = Block::default()
            .borders(Borders::LEFT)
            .border_style(border_style)
            .title(Span::styled(
                heading,
                if is_focused {
                    app.styles.accent
                } else {
                    app.styles.dimmed
                },
            ));
        let inner = block.inner(*chunk);
        frame.render_widget(block, *chunk);
        app.hits.push(*chunk, HitTarget::Pane { key, pane: *pane });

        let rows = pane_rows(app, key, *pane);
        let scroll = app
            .rail
            .band(key)
            .and_then(|b| b.pane_scroll.get(pane).copied())
            .unwrap_or(0);
        let visible = inner.height as usize;
        let start = scroll.min(rows.len().saturating_sub(1).max(0));

        let mut lines: Vec<Line> = Vec::new();
        for (offset, (text, style, target)) in
            rows.iter().skip(start).take(visible).enumerate()
        {
            let row_index = start + offset;
            let row_selected = is_focused && focused_row == Some(row_index);
            let style = if row_selected {
                app.styles.selected
            } else {
                *style
            };
            lines.push(Line::from(Span::styled(text.clone(), style)));
            if let Some(hit) = target {
                let row_area = Rect {
                    x: inner.x,
                    y: inner.y + offset as u16,
                    width: inner.width,
                    height: 1,
                };
                if row_area.y < inner.y + inner.height {
                    app.hits.push(row_area, hit.clone());
                }
            } else {
                let row_area = Rect {
                    x: inner.x,
                    y: inner.y + offset as u16,
                    width: inner.width,
                    height: 1,
                };
                if row_area.y < inner.y + inner.height {
                    app.hits.push(
                        row_area,
                        HitTarget::Row {
                            key,
                            pane: *pane,
                            row: row_index,
                        },
                    );
                }
            }
        }
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

/// Rows of one pane: display text, style, and an optional explicit hit target
/// (buttons); `None` means the row is a plain selectable row.
type PaneRow = (String, Style, Option<HitTarget>);

fn pane_rows(app: &DashboardApp, key: UiKey, pane: PaneKind) -> Vec<PaneRow> {
    match key {
        UiKey::Server(id) => match app.snapshot.servers.iter().find(|s| s.id == id) {
            Some(row) => server_pane_rows(app, row, pane),
            None => Vec::new(),
        },
        UiKey::Client(id) => match app.snapshot.clients.iter().find(|c| c.id == id) {
            Some(row) => client_pane_rows(app, row, pane),
            None => Vec::new(),
        },
    }
}

fn routing_rows(
    app: &DashboardApp,
    key: UiKey,
    routing: &Option<crate::scripting::EventHandlerConfig>,
) -> Vec<PaneRow> {
    // The pane is a summary; the editor is behind `r`. Without a visible
    // affordance there is nothing telling you routing can be changed at all.
    let mut rows: Vec<PaneRow> = vec![(
        "[ edit routing ]".to_string(),
        app.styles.button,
        Some(HitTarget::Button(ButtonId::Routing(key))),
    )];
    if let Some(config) = routing {
        for handler in &config.handlers {
            let pattern = match &handler.event_pattern {
                crate::scripting::event_handler::EventPattern::Specific(s) => s.clone(),
                crate::scripting::event_handler::EventPattern::Wildcard => "*".to_string(),
            };
            let (kind, style) = match &handler.handler {
                EventHandlerType::Llm { .. } => ("LLM", app.styles.info),
                EventHandlerType::Script { language, .. } => {
                    return_script_label(language, app, &mut rows, &pattern);
                    continue;
                }
                EventHandlerType::Static { actions } => {
                    rows.push((
                        format!("{pattern} → STATIC ({})", actions.len()),
                        app.styles.success,
                        None,
                    ));
                    continue;
                }
            };
            rows.push((format!("{pattern} → {kind}"), style, None));
        }
    }
    rows.push((
        "otherwise → LLM".to_string(),
        app.styles.dimmed,
        None,
    ));
    rows
}

fn return_script_label(
    language: &str,
    app: &DashboardApp,
    rows: &mut Vec<PaneRow>,
    pattern: &str,
) {
    rows.push((
        format!("{pattern} → SCRIPT ({language})"),
        app.styles.warning,
        None,
    ));
}

fn server_pane_rows(app: &DashboardApp, row: &ServerRow, pane: PaneKind) -> Vec<PaneRow> {
    match pane {
        // Counts live in the pane headings ("peers (3)"), so info spends its
        // narrow column on actions rather than numbers.
        PaneKind::Info => {
            let mut rows = Vec::new();
            rows.push((
                "[ edit ]".to_string(),
                app.styles.button,
                Some(HitTarget::Button(ButtonId::Edit(UiKey::Server(row.id)))),
            ));
            if row.client_counterpart.is_some() {
                rows.push((
                    "[ + client ]".to_string(),
                    app.styles.button,
                    Some(HitTarget::Button(ButtonId::AddClientFor(row.id))),
                ));
            }
            rows.push((
                "[ stop ]".to_string(),
                app.styles.failure,
                Some(HitTarget::Button(ButtonId::Stop(UiKey::Server(row.id)))),
            ));
            rows
        }
        PaneKind::Config => {
            let mut rows = Vec::new();
            if let Some(params) = row.startup_params.as_ref().and_then(|p| p.as_object()) {
                for (k, v) in params {
                    rows.push((format!("{k}: {v}"), app.styles.normal, None));
                }
            }
            if rows.is_empty() {
                rows.push(("(defaults)".to_string(), app.styles.dimmed, None));
            }
            rows.push((
                format!(
                    "instr: {}",
                    crate::utils::truncate_for_log(&row.instruction, 60)
                ),
                app.styles.dimmed,
                None,
            ));
            rows.push((
                format!("memory: {} bytes", row.memory_len),
                app.styles.dimmed,
                None,
            ));
            rows
        }
        PaneKind::Routing => routing_rows(app, UiKey::Server(row.id), &row.routing),
        PaneKind::Connections => {
            let mut rows: Vec<PaneRow> = row
                .conns
                .iter()
                .map(|c| {
                    (
                        format!("{} ↓{} ↑{}", c.remote_addr, c.bytes_received, c.bytes_sent),
                        if c.active {
                            app.styles.connection
                        } else {
                            app.styles.dimmed
                        },
                        None,
                    )
                })
                .collect();
            if !row.recent.is_empty() {
                rows.push(("── recent ──".to_string(), app.styles.separator, None));
                rows.extend(row.recent.iter().map(|c| {
                    (
                        format!("{} ↓{} ↑{}", c.remote_addr, c.bytes_received, c.bytes_sent),
                        app.styles.dimmed,
                        None,
                    )
                }));
            }
            if rows.is_empty() {
                rows.push(("(no connections)".to_string(), app.styles.dimmed, None));
            }
            rows
        }
        PaneKind::Requests => {
            if row.requests.is_empty() {
                vec![("(no requests yet)".to_string(), app.styles.dimmed, None)]
            } else {
                row.requests
                    .iter()
                    .map(|e| (summary_line(e), app.styles.normal, None))
                    .collect()
            }
        }
    }
}

fn client_pane_rows(app: &DashboardApp, row: &ClientRow, pane: PaneKind) -> Vec<PaneRow> {
    match pane {
        PaneKind::Info => {
            let mut rows = Vec::new();
            rows.push((
                "[ edit ]".to_string(),
                app.styles.button,
                Some(HitTarget::Button(ButtonId::Edit(UiKey::Client(row.id)))),
            ));
            rows.push((
                "[ stop ]".to_string(),
                app.styles.failure,
                Some(HitTarget::Button(ButtonId::Stop(UiKey::Client(row.id)))),
            ));
            rows
        }
        PaneKind::Config => {
            let mut rows = Vec::new();
            if let Some(params) = row.startup_params.as_ref().and_then(|p| p.as_object()) {
                for (k, v) in params {
                    rows.push((format!("{k}: {v}"), app.styles.normal, None));
                }
            }
            if rows.is_empty() {
                rows.push(("(defaults)".to_string(), app.styles.dimmed, None));
            }
            rows.push((
                format!(
                    "instr: {}",
                    crate::utils::truncate_for_log(&row.instruction, 60)
                ),
                app.styles.dimmed,
                None,
            ));
            rows
        }
        PaneKind::Routing => routing_rows(app, UiKey::Client(row.id), &row.routing),
        PaneKind::Connections => {
            let mut rows: Vec<PaneRow> = Vec::new();
            if let Some(c) = &row.connection {
                rows.push((
                    format!("{} ↓{} ↑{}", c.remote_addr, c.bytes_received, c.bytes_sent),
                    if c.active {
                        app.styles.connection
                    } else {
                        app.styles.dimmed
                    },
                    None,
                ));
            }
            if !row.history.is_empty() {
                rows.push(("── attempts ──".to_string(), app.styles.separator, None));
                rows.extend(row.history.iter().rev().map(|a| {
                    (
                        format!("{} {}", a.remote_addr, a.outcome),
                        app.styles.dimmed,
                        None,
                    )
                }));
            }
            if rows.is_empty() {
                rows.push(("(not connected)".to_string(), app.styles.dimmed, None));
            }
            rows
        }
        PaneKind::Requests => {
            let mut rows: Vec<PaneRow> = Vec::new();
            rows.push((
                if row.sendable {
                    "[ send ]".to_string()
                } else {
                    "[ send ] (unsupported)".to_string()
                },
                if row.sendable {
                    app.styles.button
                } else {
                    app.styles.dimmed
                },
                Some(HitTarget::Button(ButtonId::Send(row.id))),
            ));
            if row.requests.is_empty() {
                rows.push(("(no requests yet)".to_string(), app.styles.dimmed, None));
            } else {
                rows.extend(
                    row.requests
                        .iter()
                        .map(|e| (summary_line(e), app.styles.normal, None)),
                );
            }
            rows
        }
    }
}

/// Row count for a pane, so the key handler can bound row selection.
pub fn pane_row_count(app: &DashboardApp, key: UiKey, pane: PaneKind) -> usize {
    pane_rows(app, key, pane).len()
}
