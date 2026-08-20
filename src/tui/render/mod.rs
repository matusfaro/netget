//! Frame composition: chat on the left, instance rail on the right, global
//! status bar along the bottom, modals on top.

pub mod band;
pub mod chat;
pub mod overlay;
pub mod rail;
pub mod status_bar;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::DashboardApp;

/// Minimum terminal size the dashboard renders at.
pub const MIN_WIDTH: u16 = 80;
pub const MIN_HEIGHT: u16 = 24;

/// Chat pane never narrower than this; the rail takes what is left.
const CHAT_MIN_WIDTH: u16 = 40;
const CHAT_PERCENT: u16 = 42;

pub fn draw(frame: &mut Frame, app: &mut DashboardApp) {
    app.hits.clear();
    let area = frame.area();

    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let notice = Paragraph::new(format!(
            "Terminal too small: {}x{} (need {}x{}).\nResize, or run with --legacy-tui.",
            area.width, area.height, MIN_WIDTH, MIN_HEIGHT
        ))
        .style(app.styles.warning);
        frame.render_widget(notice, area);
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    let body = rows[0];
    let status = rows[1];

    let chat_width = ((body.width as u32 * CHAT_PERCENT as u32) / 100) as u16;
    let chat_width = chat_width.max(CHAT_MIN_WIDTH).min(body.width.saturating_sub(20));
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(chat_width), Constraint::Min(20)])
        .split(body);

    chat::draw(frame, app, columns[0]);
    rail::draw(frame, app, columns[1]);
    status_bar::draw(frame, app, status);

    if app.modal().is_some() {
        overlay::draw(frame, app, area);
    }
}

/// Centre a modal rect of the given percentage inside `area`.
pub fn centered(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let width = (area.width * percent_x / 100).max(20).min(area.width);
    let height = (area.height * percent_y / 100).max(7).min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}
