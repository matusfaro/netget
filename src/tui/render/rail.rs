//! The instance rail: one borderless, scrollable tree holding every server and
//! client.
//!
//! There is deliberately no per-instance frame and no servers/clients split.
//! Both cost rows and neither carries information the root row does not — an
//! instance already says what it is. Collapsing what you are not looking at is
//! what makes room, so the height algorithm that used to divide the rail into
//! bands is gone.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::{DashboardApp, Focus, Section};
use crate::tui::hit::HitTarget;

use super::band;

/// Rows taken by the header line.
const HEADER_HEIGHT: u16 = 1;

pub fn draw(frame: &mut Frame, app: &mut DashboardApp, area: Rect) {
    if area.height == 0 {
        return;
    }

    let servers = app.snapshot.servers.len();
    let clients = app.snapshot.clients.len();
    let focused = matches!(app.focus, Focus::Rail(_));

    // Header: counts on the left, the two add affordances on the right.
    let header_area = Rect {
        height: HEADER_HEIGHT,
        ..area
    };
    let header = Line::from(vec![
        Span::styled(
            " INSTANCES ",
            if focused {
                app.styles.accent
            } else {
                app.styles.title
            },
        ),
        Span::styled(
            format!("({servers} server{}, {clients} client{})",
                if servers == 1 { "" } else { "s" },
                if clients == 1 { "" } else { "s" }),
            app.styles.dimmed,
        ),
    ]);
    frame.render_widget(Paragraph::new(header), header_area);

    let mut x = header_area.x + header_area.width;
    for (label, section) in [
        ("[ + client ]", Section::Clients),
        ("[ + server ]", Section::Servers),
    ] {
        let width = label.chars().count() as u16 + 1;
        if x < header_area.x + width {
            break;
        }
        x -= width;
        let button = Rect {
            x,
            y: header_area.y,
            width: width - 1,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(Span::styled(label, app.styles.button)),
            button,
        );
        app.hits.push(button, HitTarget::AddButton(section));
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

    let rows = band::rail_rows(app);
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "  nothing running — press a for a server, or click [ + server ]",
                app.styles.dimmed,
            )),
            body,
        );
        return;
    }

    // Scroll so the selection stays visible.
    let viewport = body.height as usize;
    let max_offset = rows.len().saturating_sub(viewport);
    let mut offset = app.rail.scroll.min(max_offset);
    if let Focus::Rail(sel) = &app.focus {
        if let Some(row) = sel.row {
            if row < offset {
                offset = row;
            } else if row >= offset + viewport {
                offset = row + 1 - viewport;
            }
        }
    }
    app.rail.scroll = offset;

    let selected_row = match &app.focus {
        Focus::Rail(sel) => sel.row,
        _ => None,
    };

    let mut lines: Vec<Line> = Vec::new();
    for (screen_index, row) in rows.iter().skip(offset).take(viewport).enumerate() {
        let absolute = offset + screen_index;
        let is_selected = focused && selected_row == Some(absolute);
        lines.push(band::row_line(app, &row.row, is_selected));

        let row_area = Rect {
            x: body.x,
            y: body.y + screen_index as u16,
            width: body.width,
            height: 1,
        };
        app.hits.push(
            row_area,
            HitTarget::TreeRow {
                key: row.key,
                index: absolute,
            },
        );
        if let Some((label, id)) = &row.row.button {
            let width = label.chars().count() as u16;
            if row_area.width > width + 1 {
                let button_area = Rect {
                    x: row_area.x + row_area.width - width,
                    y: row_area.y,
                    width,
                    height: 1,
                };
                app.hits.push(button_area, HitTarget::Button(id.clone()));
            }
        }
    }
    frame.render_widget(Paragraph::new(lines), body);

    if rows.len() > viewport {
        let hint = format!(" {}–{}/{} ", offset + 1, offset + viewport, rows.len());
        let width = (hint.chars().count() as u16).min(body.width);
        let hint_area = Rect {
            x: body.x + body.width.saturating_sub(width),
            y: body.y + body.height - 1,
            width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(Span::styled(hint, app.styles.dimmed)),
            hint_area,
        );
    }
}
