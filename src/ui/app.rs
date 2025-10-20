//! Application state and rendering logic for the TUI

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use super::layout::AppLayout;

/// Main application state for the TUI
pub struct App {
    /// User input buffer
    pub input: String,
    /// Cursor position in input
    pub cursor_position: usize,
    /// LLM response messages
    pub llm_messages: Vec<String>,
    /// Status/summary messages
    pub status_messages: Vec<String>,
    /// Connection information
    pub connection_info: ConnectionInfo,
    /// Packet statistics
    pub packet_stats: PacketStats,
    /// Scroll positions for messages
    pub llm_scroll: u16,
    pub status_scroll: u16,
}

#[derive(Default, Clone)]
pub struct ConnectionInfo {
    pub mode: String,
    pub protocol: String,
    pub model: String,
    pub local_addr: Option<String>,
    pub remote_addr: Option<String>,
    pub state: String,
}

#[derive(Default, Clone)]
pub struct PacketStats {
    pub packets_received: u64,
    pub packets_sent: u64,
    pub bytes_received: u64,
    pub bytes_sent: u64,
}

impl Default for App {
    fn default() -> Self {
        Self {
            input: String::new(),
            cursor_position: 0,
            llm_messages: Vec::new(),
            status_messages: Vec::new(),
            connection_info: ConnectionInfo::default(),
            packet_stats: PacketStats::default(),
            llm_scroll: 0,
            status_scroll: 0,
        }
    }
}

impl App {
    /// Create a new App instance
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a message to the LLM messages panel
    pub fn add_llm_message(&mut self, message: String) {
        self.llm_messages.push(message);
    }

    /// Add a message to the status panel
    pub fn add_status_message(&mut self, message: String) {
        self.status_messages.push(message);
    }

    /// Handle character input
    pub fn enter_char(&mut self, c: char) {
        self.input.insert(self.cursor_position, c);
        self.cursor_position += 1;
    }

    /// Handle backspace
    pub fn delete_char(&mut self) {
        if self.cursor_position > 0 {
            self.input.remove(self.cursor_position - 1);
            self.cursor_position -= 1;
        }
    }

    /// Move cursor left
    pub fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    /// Move cursor right
    pub fn move_cursor_right(&mut self) {
        if self.cursor_position < self.input.len() {
            self.cursor_position += 1;
        }
    }

    /// Submit current input and return it
    pub fn submit_input(&mut self) -> String {
        let input = self.input.clone();
        self.input.clear();
        self.cursor_position = 0;
        input
    }

    /// Render the UI
    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let layout = AppLayout::new(area);

        // Render user input panel
        self.render_input_panel(frame, layout.input);

        // Render LLM messages panel
        self.render_llm_panel(frame, layout.llm_output);

        // Render connection info panel
        self.render_connection_panel(frame, layout.connection_info);

        // Render status panel
        self.render_status_panel(frame, layout.status);
    }

    fn render_input_panel(&self, frame: &mut Frame, area: Rect) {
        let input_text = Paragraph::new(self.input.as_str())
            .style(Style::default().fg(Color::White).bg(Color::Blue))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("User Input (Press Enter to submit, Ctrl+C to quit)")
                    .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                    .style(Style::default().bg(Color::Blue)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(input_text, area);

        // Set cursor position
        frame.set_cursor_position((area.x + self.cursor_position as u16 + 1, area.y + 1));
    }

    fn render_llm_panel(&self, frame: &mut Frame, area: Rect) {
        let messages: Vec<ListItem> = self
            .llm_messages
            .iter()
            .map(|m| ListItem::new(m.as_str()).style(Style::default().fg(Color::White).bg(Color::Blue)))
            .collect();

        let messages_list = List::new(messages)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("LLM Responses")
                    .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                    .style(Style::default().bg(Color::Blue)),
            );

        frame.render_widget(messages_list, area);
    }

    fn render_connection_panel(&self, frame: &mut Frame, area: Rect) {
        let info_text = vec![
            Line::from(vec![
                Span::styled("Mode: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD).bg(Color::Blue)),
                Span::styled(&self.connection_info.mode, Style::default().fg(Color::White).bg(Color::Blue)),
            ]),
            Line::from(vec![
                Span::styled("Protocol: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD).bg(Color::Blue)),
                Span::styled(&self.connection_info.protocol, Style::default().fg(Color::White).bg(Color::Blue)),
            ]),
            Line::from(vec![
                Span::styled("Model: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD).bg(Color::Blue)),
                Span::styled(&self.connection_info.model, Style::default().fg(Color::LightGreen).bg(Color::Blue)),
            ]),
            Line::from(vec![
                Span::styled("Local: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD).bg(Color::Blue)),
                Span::styled(
                    self.connection_info
                        .local_addr
                        .as_deref()
                        .unwrap_or("None"),
                    Style::default().fg(Color::White).bg(Color::Blue),
                ),
            ]),
            Line::from(vec![
                Span::styled("Remote: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD).bg(Color::Blue)),
                Span::styled(
                    self.connection_info
                        .remote_addr
                        .as_deref()
                        .unwrap_or("None"),
                    Style::default().fg(Color::White).bg(Color::Blue),
                ),
            ]),
            Line::from(vec![
                Span::styled("State: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD).bg(Color::Blue)),
                Span::styled(&self.connection_info.state, Style::default().fg(Color::White).bg(Color::Blue)),
            ]),
            Line::from(Span::styled("", Style::default().bg(Color::Blue))),
            Line::from(vec![Span::styled(
                "Packet Statistics:",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD).bg(Color::Blue),
            )]),
            Line::from(vec![
                Span::styled("  Packets RX: ", Style::default().fg(Color::LightCyan).bg(Color::Blue)),
                Span::styled(self.packet_stats.packets_received.to_string(), Style::default().fg(Color::White).bg(Color::Blue)),
            ]),
            Line::from(vec![
                Span::styled("  Packets TX: ", Style::default().fg(Color::LightCyan).bg(Color::Blue)),
                Span::styled(self.packet_stats.packets_sent.to_string(), Style::default().fg(Color::White).bg(Color::Blue)),
            ]),
            Line::from(vec![
                Span::styled("  Bytes RX: ", Style::default().fg(Color::LightCyan).bg(Color::Blue)),
                Span::styled(self.packet_stats.bytes_received.to_string(), Style::default().fg(Color::White).bg(Color::Blue)),
            ]),
            Line::from(vec![
                Span::styled("  Bytes TX: ", Style::default().fg(Color::LightCyan).bg(Color::Blue)),
                Span::styled(self.packet_stats.bytes_sent.to_string(), Style::default().fg(Color::White).bg(Color::Blue)),
            ]),
        ];

        let info_paragraph = Paragraph::new(info_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Connection Info")
                    .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                    .style(Style::default().bg(Color::Blue)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(info_paragraph, area);
    }

    fn render_status_panel(&self, frame: &mut Frame, area: Rect) {
        let messages: Vec<ListItem> = self
            .status_messages
            .iter()
            .map(|m| ListItem::new(m.as_str()).style(Style::default().fg(Color::LightCyan).bg(Color::Blue)))
            .collect();

        let status_list = List::new(messages)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Status / Activity Log")
                    .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                    .style(Style::default().bg(Color::Blue)),
            );

        frame.render_widget(status_list, area);
    }
}
