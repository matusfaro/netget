//! Row construction and rendering for the rail's single tree.
//!
//! Every instance contributes its own subtree; this module concatenates them
//! into the one flat list the rail scrolls, tagging each row with the instance
//! it came from so a click or a shortcut knows what it is acting on.

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::tui::app::{DashboardApp, UiKey};
use crate::tui::tree::{self, RowStyle, TreeRow, TreeState};

/// Columns per nesting level.
const INDENT: usize = 2;

/// A row of the rail: a tree row plus the instance that owns it.
pub struct RailRow {
    pub key: UiKey,
    pub row: TreeRow,
}

/// Every instance's tree, concatenated: servers first, then clients.
pub fn rail_rows(app: &DashboardApp) -> Vec<RailRow> {
    let empty = TreeState::default();
    let mut rows = Vec::new();

    for server in &app.snapshot.servers {
        let key = UiKey::Server(server.id);
        let state = app.rail.band(key).map(|b| &b.tree).unwrap_or(&empty);
        rows.extend(
            tree::server_rows(server, state)
                .into_iter()
                .map(|row| RailRow { key, row }),
        );
    }
    for client in &app.snapshot.clients {
        let key = UiKey::Client(client.id);
        let state = app.rail.band(key).map(|b| &b.tree).unwrap_or(&empty);
        rows.extend(
            tree::client_rows(client, state)
                .into_iter()
                .map(|row| RailRow { key, row }),
        );
    }
    rows
}

/// Total row count, for bounding the selection.
pub fn rail_row_count(app: &DashboardApp) -> usize {
    rail_rows(app).len()
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

pub fn row_line<'a>(app: &DashboardApp, row: &TreeRow, selected: bool) -> Line<'a> {
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

    Line::from(vec![
        Span::styled(
            format!("{indent}{marker}"),
            if selected { base } else { app.styles.separator },
        ),
        Span::styled(row.label.clone(), base),
    ])
}
