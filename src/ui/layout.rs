//! Claude-style layout management
//! Scrollable output on top, fixed input at bottom

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use super::App;

/// Render the Claude-style UI with scrollable output and fixed input
pub fn render(f: &mut Frame, app: &mut App) {
    let size = f.area();

    // Calculate how much space the input needs
    let input_lines = app.calculate_input_height(0);

    // Constrain input height: minimum 3 lines, maximum 12 lines, +2 for borders
    let input_height = input_lines.max(3).min(12) + 2;

    // Create two vertical chunks: top (output) and bottom (input + status)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),                  // Top: scrollable output (takes remaining space)
            Constraint::Length(input_height),     // Bottom: input area (dynamic, 3-12 lines)
            Constraint::Length(1),                // Status bar
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
    // Convert messages to colored lines
    let mut lines: Vec<Line> = Vec::new();
    for msg in &app.output_messages {
        let line = if msg.starts_with("[ERROR]") {
            Line::from(vec![
                Span::styled("✗ ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw(msg.strip_prefix("[ERROR]").unwrap()),
            ])
        } else if msg.starts_with("[WARN]") {
            Line::from(vec![
                Span::styled("⚠ ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw(msg.strip_prefix("[WARN]").unwrap()),
            ])
        } else if msg.starts_with("[INFO]") {
            Line::from(vec![
                Span::styled("● ", Style::default().fg(Color::Blue)),
                Span::raw(msg.strip_prefix("[INFO]").unwrap()),
            ])
        } else if msg.starts_with("[DEBUG]") {
            Line::from(vec![
                Span::styled("○ ", Style::default().fg(Color::LightBlue)),
                Span::raw(msg.strip_prefix("[DEBUG]").unwrap()),
            ])
        } else if msg.starts_with("[TRACE]") {
            Line::from(vec![
                Span::styled("· ", Style::default().fg(Color::DarkGray)),
                Span::styled(msg.strip_prefix("[TRACE]").unwrap(), Style::default().fg(Color::DarkGray)),
            ])
        } else if msg.starts_with("[USER]") {
            Line::from(vec![
                Span::styled("▶ ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(msg.strip_prefix("[USER]").unwrap(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            ])
        } else if msg.starts_with("[SERVER]") {
            Line::from(vec![
                Span::styled("◆ ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(msg.strip_prefix("[SERVER]").unwrap()),
            ])
        } else if msg.starts_with("[CONN]") {
            Line::from(vec![
                Span::styled("◇ ", Style::default().fg(Color::LightCyan)),
                Span::raw(msg.strip_prefix("[CONN]").unwrap()),
            ])
        } else {
            Line::from(Span::raw(msg.as_str()))
        };
        lines.push(line);
    }

    // All borders same color (Midnight Commander style)
    let border_style = Style::default().bg(Color::Blue).fg(Color::Cyan);

    // Highlight the title for focused panel
    let (title, title_style) = if app.is_output_focused() {
        (
            format!("Output | Log: {} | ↑↓: scroll", app.log_level.as_str()),
            Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
        )
    } else {
        (
            format!("Output | Log: {} | [Tab to focus]", app.log_level.as_str()),
            Style::default().bg(Color::Blue).fg(Color::Cyan)
        )
    };

    // Calculate scroll position
    // app.scroll_offset: 0=bottom, higher=scrolled up
    // Paragraph::scroll: 0=top, higher=scrolled down
    let inner_width = area.width.saturating_sub(2) as usize;
    let inner_height = area.height.saturating_sub(2) as usize;

    // Estimate total lines (accounting for wrapping)
    // Count actual text content length, not styled spans
    let total_lines = if inner_width == 0 {
        lines.len()
    } else {
        lines.iter().map(|line| {
            let line_len = line.width();
            if line_len == 0 {
                1
            } else {
                (line_len + inner_width - 1) / inner_width
            }
        }).sum::<usize>()
    };

    // Convert to Text AFTER calculating total_lines
    let text = Text::from(lines);

    // Calculate scroll position from top
    // When scroll_offset=0, show bottom (scroll to max)
    // When scroll_offset increases, scroll up (decrease scroll from top)
    let max_scroll = total_lines.saturating_sub(inner_height);
    let scroll_from_top = max_scroll.saturating_sub(app.scroll_offset);

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
        .scroll((scroll_from_top as u16, 0));

    f.render_widget(paragraph, area);
}

/// Render the fixed input area
fn render_input(f: &mut Frame, app: &mut App, area: Rect) {
    let title = if let Some(pos) = app.history_position {
        format!("Input [History {}/{}] | ↑↓: history | Enter: submit", pos + 1, app.command_history.len())
    } else {
        "Input | ↑↓: history | Enter: submit | Ctrl+N: newline".to_string()
    };

    // All borders same color (Midnight Commander style)
    let border_style = Style::default().bg(Color::Blue).fg(Color::Cyan);

    // Highlight the title for focused panel
    let title_style = if app.is_input_focused() {
        Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
    } else {
        Style::default().bg(Color::Blue).fg(Color::Cyan)
    };

    // Style the textarea to match the Midnight Commander theme
    let mut textarea = app.textarea.clone();
    textarea.set_block(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(title, title_style))
            .border_style(border_style)
            .style(Style::default().bg(Color::Blue).fg(Color::White))
    );
    textarea.set_style(Style::default().bg(Color::Blue).fg(Color::White));

    // Set cursor style - make it visible
    if app.is_input_focused() {
        textarea.set_cursor_line_style(Style::default().bg(Color::Blue).fg(Color::White));
        textarea.set_cursor_style(Style::default().bg(Color::White).fg(Color::Blue));
    }

    // TextArea automatically scrolls to keep the cursor visible
    f.render_widget(&textarea, area);
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
