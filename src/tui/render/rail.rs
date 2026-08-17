//! The instance rail: SERVERS and CLIENTS sections, each a stack of bands.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::{DashboardApp, Focus, Section};
use crate::tui::bands;
use crate::tui::hit::HitTarget;

use super::band;

/// Rows taken by a section header line.
const HEADER_HEIGHT: u16 = 1;

pub fn draw(frame: &mut Frame, app: &mut DashboardApp, area: Rect) {
    // Split the rail between the two sections proportionally to how much each
    // wants, so a session with only servers gives them everything.
    let server_count = app.snapshot.servers.len().max(1) as u16;
    let client_count = app.snapshot.clients.len().max(1) as u16;
    let want_servers = (server_count * bands::BAND_PREF + HEADER_HEIGHT).min(area.height);
    let want_clients = (client_count * bands::BAND_PREF + HEADER_HEIGHT).min(area.height);
    let total_want = want_servers + want_clients;
    let servers_height = if total_want <= area.height {
        want_servers
    } else {
        ((want_servers as u32 * area.height as u32) / total_want as u32) as u16
    };
    let servers_height = servers_height
        .max(HEADER_HEIGHT + bands::BAND_MIN.min(area.height))
        .min(area.height.saturating_sub(HEADER_HEIGHT + 1));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(servers_height),
            Constraint::Min(HEADER_HEIGHT + 1),
        ])
        .split(area);

    draw_section(frame, app, chunks[0], Section::Servers);
    draw_section(frame, app, chunks[1], Section::Clients);
}

fn draw_section(frame: &mut Frame, app: &mut DashboardApp, area: Rect, section: Section) {
    if area.height == 0 {
        return;
    }
    let count = app.band_count(section);
    let (label, add_label) = match section {
        Section::Servers => ("SERVERS", "[ + server ]"),
        Section::Clients => ("CLIENTS", "[ + client ]"),
    };

    let section_focused = matches!(&app.focus, Focus::Rail(sel) if sel.section == section);

    // Header: label + count on the left, [add] button on the right.
    let header_area = Rect {
        height: HEADER_HEIGHT,
        ..area
    };
    let title_style = if section_focused {
        app.styles.accent
    } else {
        app.styles.title
    };
    let header = Line::from(vec![
        Span::styled(format!(" {label} "), title_style),
        Span::styled(format!("({count})  "), app.styles.dimmed),
    ]);
    frame.render_widget(Paragraph::new(header), header_area);
    app.hits.push(header_area, HitTarget::SectionHeader(section));

    let add_width = add_label.chars().count() as u16;
    if header_area.width > add_width + 2 {
        let add_area = Rect {
            x: header_area.x + header_area.width - add_width - 1,
            y: header_area.y,
            width: add_width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(Span::styled(add_label, app.styles.button)),
            add_area,
        );
        app.hits.push(add_area, HitTarget::AddButton(section));
    }

    let body = Rect {
        x: area.x,
        y: area.y + HEADER_HEIGHT,
        width: area.width,
        height: area.height.saturating_sub(HEADER_HEIGHT),
    };
    if body.height == 0 {
        return;
    }

    if count == 0 {
        let hint = match section {
            Section::Servers => "no servers — press a (or click [ + server ]) to add one",
            Section::Clients => {
                "no clients — press a here, or c on a server to connect one to it"
            }
        };
        frame.render_widget(
            Paragraph::new(Span::styled(format!("  {hint}"), app.styles.dimmed)),
            body,
        );
        return;
    }

    let selected = match &app.focus {
        Focus::Rail(sel) if sel.section == section => sel.band,
        _ => usize::MAX, // nothing selected in this section
    };
    let selected_idx = if selected == usize::MAX { 0 } else { selected };
    let maximized = app
        .band_key(section, selected_idx)
        .and_then(|key| app.rail.band(key).map(|b| b.maximized))
        .unwrap_or(false);

    let mut layouts = bands::allocate(count, body.height, selected_idx, maximized);
    // Reclaim space from bands whose tree is shorter than their slot (a
    // collapsed instance needs three rows, not ten).
    let desired: Vec<u16> = (0..count)
        .map(|index| {
            app.band_key(section, index)
                .map(|key| {
                    let rows = super::band::band_row_count(app, key) as u16;
                    rows.saturating_add(2)
                })
                .unwrap_or(bands::BAND_MIN)
        })
        .collect();
    bands::fit_to_content(&mut layouts, &desired);

    let mut y = body.y;
    for (index, layout) in layouts.iter().enumerate() {
        if layout.height == 0 {
            continue;
        }
        let band_area = Rect {
            x: body.x,
            y,
            width: body.width,
            height: layout.height.min(body.y + body.height - y),
        };
        if band_area.height == 0 {
            break;
        }
        let is_selected = selected == index;
        band::draw(frame, app, band_area, section, index, *layout, is_selected);
        y += band_area.height;
        if y >= body.y + body.height {
            break;
        }
    }
}
