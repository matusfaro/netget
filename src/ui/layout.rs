//! Claude-style layout management
//! Scrollable output on top, fixed input at bottom

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
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

    // Render slash command suggestions popup if active
    if app.should_show_slash_suggestions() {
        render_slash_suggestions(f, app, chunks[1]);
    }
}

/// Render the scrollable output area
fn render_output(f: &mut Frame, app: &App, area: Rect) {
    // Join all messages with newlines for wrapping paragraph
    let text = app.output_messages.join("\n");

    // All borders same color (Midnight Commander style)
    let border_style = Style::default().bg(Color::Blue).fg(Color::Cyan);

    // Highlight the title for focused panel
    let (title, title_style) = if app.is_output_focused() {
        (
            "Output (↑↓ to scroll)",
            Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
        )
    } else {
        (
            "Output [Tab to focus]",
            Style::default().bg(Color::Blue).fg(Color::Cyan)
        )
    };

    // Build paragraph with wrapping
    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(title, title_style))
                .border_style(border_style)
                .style(Style::default().bg(Color::Blue).fg(Color::White))
        )
        .style(Style::default().bg(Color::Blue).fg(Color::White))
        .wrap(Wrap { trim: false })
        .scroll((app.scroll_offset as u16, 0));

    f.render_widget(paragraph, area);
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

/// Render slash command suggestions popup
fn render_slash_suggestions(f: &mut Frame, app: &App, input_area: Rect) {
    // Calculate popup position (above the input area)
    let height = (app.slash_suggestions.len() as u16 + 2).min(10); // +2 for borders, max 10 lines
    let width = 60.min(input_area.width);

    // Position popup above input area
    let popup_area = Rect {
        x: input_area.x,
        y: input_area.y.saturating_sub(height),
        width,
        height,
    };

    // Create list items
    let items: Vec<ListItem> = app
        .slash_suggestions
        .iter()
        .map(|suggestion| {
            let content = Line::from(Span::raw(suggestion));
            ListItem::new(content)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    "Slash Commands",
                    Style::default().bg(Color::Blue).fg(Color::Yellow).add_modifier(Modifier::BOLD)
                ))
                .border_style(Style::default().bg(Color::Blue).fg(Color::Yellow))
                .style(Style::default().bg(Color::Blue).fg(Color::White))
        )
        .style(Style::default().bg(Color::Blue).fg(Color::White));

    f.render_widget(list, popup_area);
}
