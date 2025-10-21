//! Claude-style layout management
//! Scrollable output on top, fixed input at bottom

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};

use super::App;

/// Render the Claude-style UI with scrollable output and fixed input
pub fn render(f: &mut Frame, app: &App) {
    let size = f.area();

    // Create two vertical chunks: top (output) and bottom (input + status)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),      // Top: scrollable output (takes remaining space)
            Constraint::Length(3),    // Bottom: input area (3 lines)
            Constraint::Length(1),    // Status bar
        ])
        .split(size);

    // Render scrollable output in the top chunk
    render_output(f, app, chunks[0]);

    // Render fixed input at the bottom
    render_input(f, app, chunks[1]);

    // Render status bar
    render_status(f, app, chunks[2]);
}

/// Render the scrollable output area
fn render_output(f: &mut Frame, app: &App, area: Rect) {
    let messages: Vec<ListItem> = app
        .output_messages
        .iter()
        .map(|msg| {
            let content = Line::from(Span::raw(msg));
            ListItem::new(content)
        })
        .collect();

    // All borders same color (Midnight Commander style)
    let border_style = Style::default().bg(Color::Blue).fg(Color::Cyan);

    // Highlight the title for focused panel
    let (title, title_style) = if app.is_output_focused() {
        (
            "Output",
            Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
        )
    } else {
        (
            "Output [Tab to focus]",
            Style::default().bg(Color::Blue).fg(Color::Cyan)
        )
    };

    let list = List::new(messages)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(title, title_style))
                .border_style(border_style)
                .style(Style::default().bg(Color::Blue).fg(Color::White))
        )
        .style(Style::default().bg(Color::Blue).fg(Color::White));

    // Calculate scroll position
    // scroll_offset = 0 means at bottom (showing latest)
    // scroll_offset > 0 means scrolled up
    let total_messages = app.output_messages.len();
    let visible_height = area.height.saturating_sub(2) as usize; // -2 for borders

    // Calculate how many messages to skip from the top
    // When scroll_offset = 0, we want to show the last N messages
    let skip = if total_messages > visible_height {
        total_messages.saturating_sub(visible_height + app.scroll_offset)
    } else {
        0
    };

    // Create scrollable list with offset
    let list = if total_messages > visible_height {
        let messages_scrolled: Vec<ListItem> = app
            .output_messages
            .iter()
            .skip(skip)
            .take(visible_height)
            .map(|msg| {
                let content = Line::from(Span::raw(msg));
                ListItem::new(content)
            })
            .collect();

        List::new(messages_scrolled)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(Span::styled(
                        format!("Output [{}/{}]", skip + visible_height, total_messages),
                        title_style
                    ))
                    .border_style(border_style)
                    .style(Style::default().bg(Color::Blue).fg(Color::White))
            )
            .style(Style::default().bg(Color::Blue).fg(Color::White))
    } else {
        list
    };

    f.render_widget(list, area);

    // Render scrollbar if content overflows
    if total_messages > visible_height {
        let scrollbar_fg = if app.is_output_focused() { Color::Cyan } else { Color::Gray };
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .style(Style::default().fg(scrollbar_fg).bg(Color::Blue));

        let mut scrollbar_state = ScrollbarState::new(total_messages.saturating_sub(visible_height))
            .position(total_messages.saturating_sub(visible_height + app.scroll_offset));

        let scrollbar_area = Rect {
            x: area.x + area.width - 1,
            y: area.y + 1,
            width: 1,
            height: area.height.saturating_sub(2),
        };

        f.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }
}

/// Render the fixed input area
fn render_input(f: &mut Frame, app: &App, area: Rect) {
    let title = if let Some(pos) = app.history_position {
        format!("Input [History {}/{}]", pos + 1, app.command_history.len())
    } else {
        "Input".to_string()
    };

    // All borders same color (Midnight Commander style)
    let border_style = Style::default().bg(Color::Blue).fg(Color::Cyan);

    // Highlight the title for focused panel
    let title_style = if app.is_input_focused() {
        Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
    } else {
        Style::default().bg(Color::Blue).fg(Color::Cyan)
    };

    let input = Paragraph::new(app.input.as_str())
        .style(Style::default().bg(Color::Blue).fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(title, title_style))
                .border_style(border_style)
                .style(Style::default().bg(Color::Blue).fg(Color::White))
        );

    f.render_widget(input, area);

    // Set cursor position (only when input is focused)
    if app.is_input_focused() {
        let cursor_x = area.x + 1 + app.cursor_position as u16;
        let cursor_y = area.y + 1;
        f.set_cursor_position((cursor_x.min(area.x + area.width - 2), cursor_y));
    }
}

/// Render the status bar
fn render_status(f: &mut Frame, app: &App, area: Rect) {
    let status_text = format!(
        " {} | {} | {} | {} | ↑{} ↓{} ",
        if app.connection_info.mode.is_empty() { "Idle" } else { &app.connection_info.mode },
        if app.connection_info.protocol.is_empty() { "-" } else { &app.connection_info.protocol },
        if app.connection_info.local_addr.is_some() {
            app.connection_info.local_addr.as_ref().unwrap()
        } else {
            "no connection"
        },
        &app.connection_info.model,
        app.packet_stats.bytes_received,
        app.packet_stats.bytes_sent,
    );

    let status = Paragraph::new(status_text)
        .style(
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        );

    f.render_widget(status, area);
}
