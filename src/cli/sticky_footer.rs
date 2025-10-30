//! Sticky footer rendering for rolling terminal
//!
//! Manages the fixed footer area at the bottom of the terminal that displays:
//! - Servers and connections (normal mode)
//! - Slash command suggestions (slash command mode)
//! - Input field (always)
//! - Status bar (always)

use anyhow::Result;
use crossterm::{
    cursor, execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{Clear, ClearType},
};
use std::io::Write;

use crate::ui::app::{ConnectionDisplayInfo, LogLevel, PacketStats, ServerDisplayInfo};

use super::input_state::InputState;

/// Content mode for the sticky footer
#[derive(Debug, Clone)]
pub enum FooterContent {
    /// Normal mode: show servers and connections
    Normal {
        servers: Vec<ServerDisplayInfo>,
        connections: Vec<ConnectionDisplayInfo>,
        expand_all: bool,
    },
    /// Slash command mode: show command suggestions
    SlashCommands { suggestions: Vec<String> },
}

/// Connection info for the status bar
#[derive(Debug, Clone, Default)]
pub struct ConnectionInfo {
    pub model: String,
    pub scripting_env: String,
    pub web_search_enabled: bool,
}

/// Sticky footer state and renderer
pub struct StickyFooter {
    /// Terminal width
    terminal_width: u16,
    /// Terminal height
    terminal_height: u16,
    /// Height of the scroll region (where output goes)
    scroll_region_height: u16,
    /// Current footer content mode
    content: FooterContent,
    /// Input state
    input: InputState,
    /// Connection info for status bar
    connection_info: ConnectionInfo,
    /// Packet statistics for status bar
    packet_stats: PacketStats,
    /// Current log level
    log_level: LogLevel,
    /// Custom status message (for testing footer height changes)
    custom_status: Option<String>,
    /// Number of blank lines at the top of scroll region (created by footer expansions)
    blank_lines_buffer: u16,
    /// Track last footer height to clear properly when shrinking
    last_footer_height: u16,
}

impl StickyFooter {
    /// Create a new sticky footer
    pub fn new(width: u16, height: u16) -> Result<Self> {
        let mut footer = Self {
            terminal_width: width,
            terminal_height: height,
            scroll_region_height: height.saturating_sub(10), // Initial guess
            content: FooterContent::Normal {
                servers: Vec::new(),
                connections: Vec::new(),
                expand_all: false,
            },
            input: InputState::new(),
            connection_info: ConnectionInfo::default(),
            packet_stats: PacketStats::default(),
            log_level: LogLevel::Info,
            custom_status: None,
            blank_lines_buffer: 0,
            last_footer_height: 0,
        };

        // Calculate actual footer height
        footer.recalculate_scroll_region();
        Ok(footer)
    }

    /// Set footer content mode
    pub fn set_content(&mut self, content: FooterContent) {
        self.content = content;
        self.recalculate_scroll_region();
    }

    /// Get mutable reference to input state
    pub fn input_mut(&mut self) -> &mut InputState {
        &mut self.input
    }

    /// Get reference to input state
    pub fn input(&self) -> &InputState {
        &self.input
    }

    /// Set connection info
    pub fn set_connection_info(&mut self, info: ConnectionInfo) {
        self.connection_info = info;
    }

    /// Set packet stats
    pub fn set_packet_stats(&mut self, stats: PacketStats) {
        self.packet_stats = stats;
    }

    /// Set log level
    pub fn set_log_level(&mut self, level: LogLevel) {
        self.log_level = level;
    }

    /// Set custom status message (for testing footer height changes)
    pub fn set_custom_status(&mut self, status: Option<String>) {
        self.custom_status = status;
        self.recalculate_scroll_region();
    }

    /// Get scroll region height
    pub fn scroll_region_height(&self) -> u16 {
        self.scroll_region_height
    }

    pub fn terminal_height(&self) -> u16 {
        self.terminal_height
    }

    pub fn terminal_width(&self) -> u16 {
        self.terminal_width
    }

    pub fn blank_lines_buffer(&self) -> u16 {
        self.blank_lines_buffer
    }

    pub fn decrement_blank_lines_buffer(&mut self) {
        if self.blank_lines_buffer > 0 {
            self.blank_lines_buffer -= 1;
        }
    }

    pub fn add_to_blank_lines_buffer(&mut self, lines: u16) {
        self.blank_lines_buffer += lines;
    }

    pub fn consume_blank_lines_buffer(&mut self, lines: u16) -> u16 {
        let consumed = self.blank_lines_buffer.min(lines);
        self.blank_lines_buffer -= consumed;
        consumed
    }

    /// Handle terminal resize
    pub fn handle_resize(&mut self, width: u16, height: u16) {
        self.terminal_width = width;
        self.terminal_height = height;
        self.recalculate_scroll_region();
    }

    /// Calculate lines needed for normal content (servers/connections)
    fn calculate_normal_content_lines(
        &self,
        servers: &[ServerDisplayInfo],
        connections: &[ConnectionDisplayInfo],
        expand_all: bool,
    ) -> u16 {
        // If custom status is set, use its line count
        if let Some(ref custom) = self.custom_status {
            return custom.lines().count() as u16;
        }

        if servers.is_empty() {
            return 0; // No message when no servers
        }

        let mut total_lines = 0;

        for (_idx, server) in servers.iter().enumerate() {
            // Server line
            let server_text = format!(
                "#{} {} :{} - {}",
                server.id, server.protocol, server.port, server.status
            );
            total_lines += self.wrapped_line_count(&server_text);

            // Connection lines
            let server_connections: Vec<_> = connections
                .iter()
                .filter(|c| c.server_id == server.id)
                .collect();

            if !server_connections.is_empty() {
                let max_to_show = if expand_all {
                    server_connections.len()
                } else {
                    3
                };

                for conn in server_connections.iter().take(max_to_show) {
                    let conn_text = format!("  #{} {} {}", conn.id, conn.address, conn.state);
                    total_lines += self.wrapped_line_count(&conn_text);
                }

                // "... N more" line if truncated
                if !expand_all && server_connections.len() > 3 {
                    total_lines += 1;
                }
            }
        }

        // Cap at 15 lines
        total_lines.min(15)
    }

    /// Calculate lines needed for input
    fn calculate_input_lines(&self) -> u16 {
        let lines = self.input.lines();
        let mut total = 0;

        for line in lines {
            total += self.wrapped_line_count(line);
        }

        // Ensure at least 1 line, cap at 12
        total.max(1).min(12)
    }

    /// Calculate how many visual lines a text string will take after wrapping
    fn wrapped_line_count(&self, text: &str) -> u16 {
        if text.is_empty() {
            return 1;
        }

        let width = self.terminal_width.saturating_sub(2) as usize; // -2 for padding
        if width == 0 {
            return 1;
        }

        // Use textwrap to calculate wrapped lines
        let wrapped = textwrap::wrap(text, width);
        wrapped.len().max(1) as u16
    }

    /// Recalculate scroll region height
    fn recalculate_scroll_region(&mut self) {
        let footer_height = self.calculate_footer_height();

        // Ensure scroll region is at least 5 lines
        self.scroll_region_height = self
            .terminal_height
            .saturating_sub(footer_height)
            .max(5);
    }

    /// Render the sticky footer (overlay at bottom of terminal)
    pub fn render(&mut self, stdout: &mut impl Write) -> Result<()> {
        self.recalculate_scroll_region();

        let footer_height = self.calculate_footer_height();

        // If footer is expanding, push content up by printing newlines
        if footer_height > self.last_footer_height {
            let expansion = footer_height - self.last_footer_height;

            // Move to last line of scroll region and print newlines to push content up
            let scroll_height = self.scroll_region_height;
            let last_scroll_line = scroll_height.saturating_sub(1);

            execute!(stdout, cursor::MoveTo(0, last_scroll_line))?;
            for _ in 0..expansion {
                execute!(stdout, Print("\n"))?;
            }
        }

        // Clear footer area - use max of old and new height to clear remnants when shrinking
        let height_to_clear = footer_height.max(self.last_footer_height);
        let clear_start = self.terminal_height.saturating_sub(height_to_clear);

        for line_offset in 0..height_to_clear {
            execute!(
                stdout,
                cursor::MoveTo(0, clear_start + line_offset),
                Clear(ClearType::CurrentLine),
            )?;
        }

        // Update tracked height for next render
        self.last_footer_height = footer_height;

        // Calculate fixed positions from bottom up
        // Input, separators, and status bar stay in fixed positions
        let status_line = self.terminal_height - 1;
        let separator_before_status = status_line - 1;
        let input_lines = self.calculate_input_lines();
        let input_start = separator_before_status - input_lines;
        let separator_before_input = input_start - 1;

        // Content is positioned above the input separator
        let content_lines = match &self.content {
            FooterContent::Normal {
                servers,
                connections,
                expand_all,
            } => self.calculate_normal_content_lines(servers, connections, *expand_all),
            FooterContent::SlashCommands { suggestions } => {
                suggestions.len().min(10) as u16
            }
        };

        // If we have content, position it with a separator above it
        if content_lines > 0 {
            let separator_before_content = separator_before_input - content_lines - 1;
            let content_start = separator_before_content + 1;

            // Render separator above content
            self.render_separator(stdout, separator_before_content)?;

            // Render content
            match &self.content {
                FooterContent::Normal {
                    servers,
                    connections,
                    expand_all,
                } => self.render_normal_content(stdout, content_start, servers, connections, *expand_all)?,
                FooterContent::SlashCommands { suggestions } => {
                    self.render_slash_commands(stdout, content_start, suggestions)?
                }
            };
        }

        // Render separator before input (always present)
        self.render_separator(stdout, separator_before_input)?;

        // Render input (fixed position)
        self.render_input(stdout, input_start)?;

        // Render separator before status bar (always present)
        self.render_separator(stdout, separator_before_status)?;

        // Render status bar (fixed position)
        self.render_status_bar(stdout, status_line)?;

        // Position cursor in input field and show it
        self.position_cursor(stdout)?;
        execute!(stdout, cursor::Show)?;

        stdout.flush()?;
        Ok(())
    }

    /// Render only the input portion of the footer (for efficient keystroke handling)
    pub fn render_input_only(&mut self, stdout: &mut impl Write) -> Result<()> {
        let footer_height = self.calculate_footer_height();
        let footer_start = self.terminal_height.saturating_sub(footer_height);

        // Calculate where input starts
        let content_lines = match &self.content {
            FooterContent::Normal {
                servers,
                connections,
                expand_all,
            } => self.calculate_normal_content_lines(servers, connections, *expand_all),
            FooterContent::SlashCommands { suggestions } => {
                suggestions.len().min(10) as u16
            }
        };

        // Separators before input: 1 if content exists, otherwise just 1 for the input separator
        let separators_before_input = if content_lines > 0 { 2 } else { 1 };
        let input_start = footer_start + content_lines + separators_before_input;
        let input_lines = self.calculate_input_lines();

        // Clear input area
        for line_offset in 0..input_lines {
            execute!(
                stdout,
                cursor::MoveTo(0, input_start + line_offset),
                Clear(ClearType::CurrentLine),
            )?;
        }

        // Render input and get the next line number
        let next_line = self.render_input(stdout, input_start)?;

        // Clear and render separator before status bar
        execute!(
            stdout,
            cursor::MoveTo(0, next_line),
            Clear(ClearType::CurrentLine),
        )?;
        let separator_line = self.render_separator(stdout, next_line)?;

        // Clear and render status bar
        execute!(
            stdout,
            cursor::MoveTo(0, separator_line),
            Clear(ClearType::CurrentLine),
        )?;
        self.render_status_bar(stdout, separator_line)?;

        // Position cursor
        self.position_cursor(stdout)?;
        execute!(stdout, cursor::Show)?;
        stdout.flush()?;
        Ok(())
    }

    /// Get the calculated footer height (public for clearing)
    pub fn calculate_footer_height(&self) -> u16 {
        let content_lines = match &self.content {
            FooterContent::Normal {
                servers,
                connections,
                expand_all,
            } => self.calculate_normal_content_lines(servers, connections, *expand_all),
            FooterContent::SlashCommands { suggestions } => {
                suggestions.len().min(10) as u16
            }
        };

        let input_lines = self.calculate_input_lines();
        let status_lines = 1;

        // Add separator lines:
        // - If we have content: 3 separators (one above content, one above input, one above status)
        // - If no content: 2 separators (one above input, one above status)
        let separator_lines = if content_lines > 0 { 3 } else { 2 };
        content_lines + separator_lines + input_lines + status_lines
    }

    /// Render normal content (servers and connections)
    fn render_normal_content(
        &self,
        stdout: &mut impl Write,
        start_line: u16,
        servers: &[ServerDisplayInfo],
        connections: &[ConnectionDisplayInfo],
        expand_all: bool,
    ) -> Result<u16> {
        let mut current_line = start_line;

        // If custom status is set, render it (separator handled by main render)
        if let Some(ref custom) = self.custom_status {
            for line in custom.lines() {
                execute!(
                    stdout,
                    cursor::MoveTo(0, current_line),
                    SetForegroundColor(Color::DarkGrey),
                    Print(line),
                    ResetColor,
                )?;
                current_line += 1;
            }
            return Ok(current_line);
        }

        if servers.is_empty() {
            // Don't show anything when no servers - no separator, no content
            return Ok(current_line);
        }

        let max_content_lines = self.calculate_normal_content_lines(servers, connections, expand_all);

        for server in servers.iter() {
            if current_line >= start_line + max_content_lines {
                break; // Hit the cap
            }

            // Server line
            let server_text = format!(
                "#{} {} :{} - {}",
                server.id, server.protocol, server.port, server.status
            );

            // Render wrapped server line
            let wrapped = self.wrap_text(&server_text);
            for line in wrapped {
                if current_line >= start_line + max_content_lines {
                    break;
                }
                execute!(stdout, cursor::MoveTo(0, current_line), Print(&line))?;
                current_line += 1;
            }

            // Connection lines
            let server_connections: Vec<_> = connections
                .iter()
                .filter(|c| c.server_id == server.id)
                .collect();

            if !server_connections.is_empty() {
                let max_to_show = if expand_all {
                    server_connections.len()
                } else {
                    3
                };

                for conn in server_connections.iter().take(max_to_show) {
                    if current_line >= start_line + max_content_lines {
                        break;
                    }

                    let conn_text = format!("  #{} {} {}", conn.id, conn.address, conn.state);
                    let wrapped = self.wrap_text(&conn_text);
                    for line in wrapped {
                        if current_line >= start_line + max_content_lines {
                            break;
                        }
                        execute!(stdout, cursor::MoveTo(0, current_line), Print(&line))?;
                        current_line += 1;
                    }
                }

                // "... N more" line
                if !expand_all && server_connections.len() > 3 {
                    if current_line < start_line + max_content_lines {
                        execute!(
                            stdout,
                            cursor::MoveTo(0, current_line),
                            SetForegroundColor(Color::DarkGrey),
                            Print(format!("  ... ({} more)", server_connections.len() - 3)),
                            ResetColor,
                        )?;
                        current_line += 1;
                    }
                }
            }
        }

        Ok(current_line)
    }

    /// Render slash command suggestions
    fn render_slash_commands(
        &self,
        stdout: &mut impl Write,
        start_line: u16,
        suggestions: &[String],
    ) -> Result<u16> {
        let mut current_line = start_line;

        let max_lines = suggestions.len().min(10);
        for suggestion in suggestions.iter().take(max_lines) {
            let wrapped = self.wrap_text(suggestion);
            for line in wrapped {
                execute!(
                    stdout,
                    cursor::MoveTo(0, current_line),
                    SetForegroundColor(Color::DarkGrey),
                    Print(&line),
                    ResetColor,
                )?;
                current_line += 1;
            }
        }

        Ok(current_line)
    }

    /// Render a separator line
    fn render_separator(&self, stdout: &mut impl Write, line: u16) -> Result<u16> {
        let separator = "─".repeat(self.terminal_width as usize);
        execute!(
            stdout,
            cursor::MoveTo(0, line),
            SetForegroundColor(Color::DarkGreen),
            Print(&separator),
            ResetColor,
        )?;
        Ok(line + 1)
    }

    /// Render input field
    fn render_input(&self, stdout: &mut impl Write, start_line: u16) -> Result<u16> {
        let mut current_line = start_line;

        let input_lines = self.input.lines();
        let max_input_lines = self.calculate_input_lines() as usize;

        for (idx, line) in input_lines.iter().enumerate() {
            if idx >= max_input_lines {
                break;
            }

            let prefix = if idx == 0 { "> " } else { "  " };
            let text_with_prefix = format!("{}{}", prefix, line);

            let wrapped = self.wrap_text(&text_with_prefix);
            for wrapped_line in wrapped {
                execute!(stdout, cursor::MoveTo(0, current_line), Print(&wrapped_line))?;
                current_line += 1;
            }
        }

        Ok(current_line)
    }

    /// Render status bar
    fn render_status_bar(&self, stdout: &mut impl Write, line: u16) -> Result<u16> {
        let web_status = if self.connection_info.web_search_enabled {
            "ON"
        } else {
            "OFF"
        };

        execute!(stdout, cursor::MoveTo(0, line))?;

        // Print each segment with appropriate coloring
        execute!(
            stdout,
            Print(" "),
            SetForegroundColor(Color::DarkGrey),
            Print("↓"),
            ResetColor,
            Print(format!("{}", self.packet_stats.packets_received)),
            SetForegroundColor(Color::DarkGrey),
            Print(" ↑"),
            ResetColor,
            Print(format!("{}", self.packet_stats.packets_sent)),
            SetForegroundColor(Color::DarkGrey),
            Print(" | Model:"),
            ResetColor,
            Print(format!("{}", &self.connection_info.model)),
            SetForegroundColor(Color::DarkGrey),
            Print(" | ^l Log:"),
            ResetColor,
            Print(format!("{}", self.log_level.as_str())),
            SetForegroundColor(Color::DarkGrey),
            Print(" | ^e Script:"),
            ResetColor,
            Print(format!("{}", &self.connection_info.scripting_env)),
            SetForegroundColor(Color::DarkGrey),
            Print(" | ^w WebSearch:"),
            ResetColor,
            Print(format!("{}", web_status)),
            ResetColor,
        )?;

        Ok(line + 1)
    }

    /// Position the cursor in the input field
    fn position_cursor(&self, stdout: &mut impl Write) -> Result<()> {
        let (cursor_row, cursor_col) = self.input.cursor_position();

        // Calculate visual position considering wrapping and "> " prefix
        let input_start_line = self.terminal_height
            - self.calculate_input_lines()
            - 2; // -2 for separator + status bar

        let mut visual_row = input_start_line;
        let mut visual_col = 0;

        let input_lines = self.input.lines();

        for (idx, line) in input_lines.iter().enumerate() {
            let prefix = if idx == 0 { "> " } else { "  " };

            if idx < cursor_row {
                // Count wrapped lines for previous rows
                let text_with_prefix = format!("{}{}", prefix, line);
                let wrapped = self.wrap_text(&text_with_prefix);
                visual_row += wrapped.len() as u16;
            } else if idx == cursor_row {
                // Handle empty input as a special case - cursor goes right after prefix
                if line.is_empty() && cursor_col == 0 {
                    visual_col = prefix.len() as u16;
                    break;
                }

                // Calculate position on current row
                // cursor_col is position within the actual text (not including prefix)
                // We need to add prefix length to get the visual column
                let cursor_in_line = prefix.len() + cursor_col;
                let text_with_prefix = format!("{}{}", prefix, line);
                let wrapped = self.wrap_text(&text_with_prefix);

                // Handle case where wrapped is empty
                if wrapped.is_empty() {
                    visual_col = prefix.len() as u16;
                } else {
                    let mut char_count = 0;
                    let mut found = false;
                    for (wrap_idx, wrapped_line) in wrapped.iter().enumerate() {
                        let line_end = char_count + wrapped_line.len();
                        if cursor_in_line <= line_end {
                            visual_row += wrap_idx as u16;
                            visual_col = (cursor_in_line - char_count) as u16;
                            found = true;
                            break;
                        }
                        char_count = line_end;
                        visual_row += 1;
                    }
                    // Fallback: if cursor position wasn't found in wrapped lines,
                    // place it at the end of the last line or after prefix
                    if !found {
                        visual_col = prefix.len() as u16;
                    }
                }
                break;
            }
        }

        execute!(stdout, cursor::MoveTo(visual_col as u16, visual_row))?;
        Ok(())
    }

    /// Word-wrap text to terminal width
    fn wrap_text(&self, text: &str) -> Vec<String> {
        let width = self.terminal_width.saturating_sub(2) as usize; // -2 for safety
        if width == 0 || text.is_empty() {
            return vec![String::new()];
        }

        textwrap::wrap(text, width)
            .into_iter()
            .map(|s| s.to_string())
            .collect()
    }
}
