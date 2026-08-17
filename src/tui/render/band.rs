//! One instance band: a scrollable tree.
//!
//! The instance is the root; `config`, `routing` and `peers` hang off it, a
//! peer's requests hang off that peer, and an expanded request shows its full
//! detail one level deeper. Indentation carries the nesting, so nothing has to
//! be squeezed into a fixed-width column.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::app::{DashboardApp, Focus, Section, UiKey};
use crate::tui::bands::BandLayout;
use crate::tui::hit::HitTarget;
use crate::tui::tree::{self, RowStyle, TreeRow, TreeState};

/// Columns per nesting level.
const INDENT: usize = 2;

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

    let rows = band_rows(app, key);

    // A collapsed band shows only its root row; a frame would cost the single
    // line it has.
    if layout.collapsed || area.height <= 2 {
        if let Some(root) = rows.first() {
            let title_area = Rect {
                height: 1,
                ..area
            };
            frame.render_widget(Paragraph::new(row_line(app, root, false)), title_area);
        }
        return;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if selected {
            app.styles.accent
        } else {
            app.styles.separator
        });
    let body = block.inner(area);
    frame.render_widget(block, area);
    if body.height == 0 {
        return;
    }

    // Scroll so the selected row stays inside the band.
    let selected_row = match &app.focus {
        Focus::Rail(sel) if selected => sel.row,
        _ => None,
    };
    let viewport = body.height as usize;
    let max_offset = rows.len().saturating_sub(viewport);
    let mut offset = app.rail.band(key).map(|b| b.scroll).unwrap_or(0).min(max_offset);
    if let Some(row) = selected_row {
        if row < offset {
            offset = row;
        } else if row >= offset + viewport {
            offset = row + 1 - viewport;
        }
    }
    app.rail.band_mut(key).scroll = offset;

    let mut lines: Vec<Line> = Vec::new();
    for (screen_index, row) in rows.iter().skip(offset).take(viewport).enumerate() {
        let absolute = offset + screen_index;
        let is_selected = selected && selected_row == Some(absolute);
        lines.push(row_line(app, row, is_selected));

        let row_area = Rect {
            x: body.x,
            y: body.y + screen_index as u16,
            width: body.width,
            height: 1,
        };
        app.hits.push(
            row_area,
            HitTarget::TreeRow {
                key,
                index: absolute,
            },
        );
        // The action sits at the right edge; register it after the row so it
        // wins the click (hit testing resolves most-recent-first).
        if let Some((label, id)) = &row.button {
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

    // Position indicator when the tree overflows its band.
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

/// Flatten a band's tree using its stored expansion state.
pub fn band_rows(app: &DashboardApp, key: UiKey) -> Vec<TreeRow> {
    let empty = TreeState::default();
    let state = app.rail.band(key).map(|b| &b.tree).unwrap_or(&empty);
    match key {
        UiKey::Server(id) => app
            .snapshot
            .servers
            .iter()
            .find(|s| s.id == id)
            .map(|row| tree::server_rows(row, state))
            .unwrap_or_default(),
        UiKey::Client(id) => app
            .snapshot
            .clients
            .iter()
            .find(|c| c.id == id)
            .map(|row| tree::client_rows(row, state))
            .unwrap_or_default(),
    }
}

/// Row count for a band, for bounding the selection.
pub fn band_row_count(app: &DashboardApp, key: UiKey) -> usize {
    band_rows(app, key).len()
}

fn style_for(app: &DashboardApp, style: RowStyle) -> Style {
    match style {
        RowStyle::Instance | RowStyle::Group => app.styles.title,
        RowStyle::Normal => app.styles.normal,
        RowStyle::Dim => app.styles.dimmed,
        RowStyle::Good => app.styles.success,
        RowStyle::Warn => app.styles.warning,
        RowStyle::Bad => app.styles.error,
        RowStyle::Button => app.styles.button,
    }
}

fn row_line<'a>(app: &DashboardApp, row: &TreeRow, selected: bool) -> Line<'a> {
    let indent = " ".repeat(row.depth as usize * INDENT);
    let marker = match row.expanded {
        Some(true) => "▾ ",
        Some(false) => "▸ ",
        None => "  ",
    };
    let base = if selected {
        app.styles.selected
    } else {
        style_for(app, row.style)
    };

    let mut spans = vec![
        Span::styled(
            format!("{indent}{marker}"),
            if selected { base } else { app.styles.separator },
        ),
        Span::styled(row.label.clone(), base),
    ];
    if let Some((label, _)) = &row.button {
        spans.push(Span::styled("  ", app.styles.dimmed));
        spans.push(Span::styled(label.clone(), app.styles.button));
    }
    Line::from(spans)
}
