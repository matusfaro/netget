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
    style::{Print, ResetColor, SetForegroundColor},
    terminal::{Clear, ClearType},
};
use std::io::Write;
use tokio::sync::oneshot;

use crate::state::app_state::{ConversationInfo, WebApprovalResponse, WebSearchMode};
use crate::ui::app::{ConnectionDisplayInfo, LogLevel, PacketStats, ServerDisplayInfo};

use super::input_state::InputState;
use super::theme::ColorPalette;

// Layout constants for two-column footer
const INPUTS_LEFT_MARGIN: u16 = 6;
const INPUTS_COLUMN_WIDTH: u16 = 30;
const COLUMN_MARGIN: u16 = 4;

/// Pending web approval request
pub struct PendingApproval {
    pub url: String,
    pub response_tx: oneshot::Sender<WebApprovalResponse>,
}

/// Content mode for the sticky footer
#[derive(Debug, Clone)]
pub enum FooterContent {
    /// Normal mode: show servers and connections
    Normal {
        servers: Vec<ServerDisplayInfo>,
        connections: Vec<ConnectionDisplayInfo>,
        expand_all: bool,
        conversations: Vec<ConversationInfo>,
    },
    /// Slash command mode: show command suggestions
    SlashCommands { suggestions: Vec<String> },
}

/// Connection info for the status bar
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub model: String,
    pub scripting_env: String,
    pub web_search_mode: WebSearchMode,
}

impl Default for ConnectionInfo {
    fn default() -> Self {
        Self {
            model: String::new(),
            scripting_env: String::new(),
            web_search_mode: WebSearchMode::On,
        }
    }
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
    /// Pending web approval request (if any)
    pub pending_approval: Option<PendingApproval>,
    /// System capabilities (for privilege warnings in status bar)
    system_capabilities: crate::privilege::SystemCapabilities,
    /// Color palette for theming
    palette: ColorPalette,
}

impl StickyFooter {
    /// Create a new sticky footer
    pub fn new(width: u16, height: u16, system_capabilities: crate::privilege::SystemCapabilities, palette: ColorPalette) -> Result<Self> {
        let mut footer = Self {
            terminal_width: width,
            terminal_height: height,
            scroll_region_height: height.saturating_sub(10), // Initial guess
            content: FooterContent::Normal {
                servers: Vec::new(),
                connections: Vec::new(),
                expand_all: false,
                conversations: Vec::new(),
            },
            input: InputState::new(),
            connection_info: ConnectionInfo::default(),
            packet_stats: PacketStats::default(),
            log_level: LogLevel::Info,
            custom_status: None,
            blank_lines_buffer: 0,
            last_footer_height: 0,
            pending_approval: None,
            system_capabilities,
            palette,
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

    /// Calculate lines needed for normal content (two-column layout)
    fn calculate_normal_content_lines(
        &self,
        servers: &[ServerDisplayInfo],
        connections: &[ConnectionDisplayInfo],
        expand_all: bool,
        conversations: &[ConversationInfo],
    ) -> u16 {
        // If custom status is set, use its line count
        if let Some(ref custom) = self.custom_status {
            return custom.lines().count() as u16;
        }

        // Calculate inputs column height (User + Scripting conversations)
        let input_convs: Vec<_> = conversations
            .iter()
            .filter(|c| matches!(&c.source, crate::state::app_state::ConversationSource::User | crate::state::app_state::ConversationSource::Scripting))
            .collect();

        let mut inputs_height = 0u16;
        if !input_convs.is_empty() {
            inputs_height += 1; // Header line
            inputs_height += input_convs.len() as u16; // Each conversation is 1 line (truncated to fit column width)
        }

        // Calculate servers column height
        let mut servers_height = 0u16;
        if !servers.is_empty() {
            servers_height += 1; // Header line

            for server in servers.iter() {
                servers_height += 1; // Server line

                // Connection lines for this server
                let server_connections: Vec<_> = connections
                    .iter()
                    .filter(|c| c.server_id == server.id)
                    .collect();

                if !server_connections.is_empty() {
                    let max_to_show = if expand_all {
                        server_connections.len()
                    } else {
                        10
                    };

                    servers_height += max_to_show.min(server_connections.len()) as u16;

                    // Add conversation sub-items for each connection
                    for conn in server_connections.iter().take(max_to_show.min(server_connections.len())) {
                        // Count conversations for this specific connection
                        let conn_convs: Vec<_> = conversations
                            .iter()
                            .filter(|conv| {
                                matches!(&conv.source,
                                    crate::state::app_state::ConversationSource::Network { server_id, connection_id }
                                    if server_id.as_u32().to_string() == server.id && connection_id.map(|id| id.to_string()) == Some(conn.id.to_string())
                                )
                            })
                            .collect();
                        servers_height += conn_convs.len() as u16;
                    }

                    // "... N more" line if truncated
                    if !expand_all && server_connections.len() > 10 {
                        servers_height += 1;
                    }
                }
            }
        }

        // Return the max of the two columns (or 0 if both empty)
        inputs_height.max(servers_height)
    }

    /// Calculate lines needed for input (or approval prompt if pending)
    fn calculate_input_lines(&self) -> u16 {
        // If approval is pending, use 1 line for the approval prompt
        if self.pending_approval.is_some() {
            return 1;
        }

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
                conversations,
            } => self.calculate_normal_content_lines(servers, connections, *expand_all, conversations),
            FooterContent::SlashCommands { suggestions } => {
                suggestions.len().min(10) as u16
            }
        };

        // If we have content, render it without separator above
        if content_lines > 0 {
            let content_start = separator_before_input - content_lines;

            // Render content
            match &self.content {
                FooterContent::Normal {
                    servers,
                    connections,
                    expand_all,
                    conversations,
                } => self.render_normal_content(stdout, content_start, servers, connections, *expand_all, conversations)?,
                FooterContent::SlashCommands { suggestions } => {
                    // Slash commands still need a separator
                    let separator_before_content = content_start - 1;
                    self.render_separator(stdout, separator_before_content)?;
                    self.render_slash_commands(stdout, content_start, suggestions)?
                }
            };
        }

        // Render separator before input (always present) with ┴ joins for columns
        self.render_separator_with_joins(
            stdout,
            separator_before_input,
            &self.content,
        )?;

        // Render input or approval prompt (fixed position)
        if let Some(ref approval) = self.pending_approval {
            // Render approval prompt instead of input
            self.render_approval_prompt(stdout, input_start, &approval.url)?;
            // Hide cursor during approval
            execute!(stdout, cursor::Hide)?;
        } else {
            // Render normal input
            self.render_input(stdout, input_start)?;
            // Position cursor in input field and show it
            self.position_cursor(stdout)?;
            execute!(stdout, cursor::Show)?;
        }

        // Render separator before status bar (always present)
        self.render_separator(stdout, separator_before_status)?;

        // Render status bar (fixed position)
        self.render_status_bar(stdout, status_line)?;

        stdout.flush()?;
        Ok(())
    }

    /// Render only the input portion of the footer (for efficient keystroke handling)
    pub fn render_input_only(&mut self, stdout: &mut impl Write) -> Result<()> {
        // Calculate fixed positions from bottom up (same as render())
        let status_line = self.terminal_height - 1;
        let separator_before_status = status_line - 1;
        let input_lines = self.calculate_input_lines();
        let input_start = separator_before_status - input_lines;

        // Clear input area
        for line_offset in 0..input_lines {
            execute!(
                stdout,
                cursor::MoveTo(0, input_start + line_offset),
                Clear(ClearType::CurrentLine),
            )?;
        }

        // Render input or approval prompt and get the next line number
        let next_line = if let Some(ref approval) = self.pending_approval {
            // Render approval prompt
            let result = self.render_approval_prompt(stdout, input_start, &approval.url)?;
            execute!(stdout, cursor::Hide)?;
            result
        } else {
            // Render normal input
            self.render_input(stdout, input_start)?
        };

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

        // Position cursor (only if not in approval mode)
        if self.pending_approval.is_none() {
            self.position_cursor(stdout)?;
            execute!(stdout, cursor::Show)?;
        }

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
                conversations,
            } => self.calculate_normal_content_lines(servers, connections, *expand_all, conversations),
            FooterContent::SlashCommands { suggestions } => {
                suggestions.len().min(10) as u16
            }
        };

        let input_lines = self.calculate_input_lines();
        let status_lines = 1;

        // Add separator lines based on content type:
        // - Normal mode (servers/inputs): 2 separators (one above input, one above status) - no separator above content
        // - SlashCommands mode: 3 separators (one above content, one above input, one above status)
        // - No content: 2 separators (one above input, one above status)
        let separator_lines = match &self.content {
            FooterContent::Normal { .. } if content_lines > 0 => 2,
            FooterContent::SlashCommands { .. } if content_lines > 0 => 3,
            _ => 2,
        };
        content_lines + separator_lines + input_lines + status_lines
    }

    /// Render normal content (two-column layout with floating headers)
    fn render_normal_content(
        &self,
        stdout: &mut impl Write,
        start_line: u16,
        servers: &[ServerDisplayInfo],
        connections: &[ConnectionDisplayInfo],
        expand_all: bool,
        conversations: &[ConversationInfo],
    ) -> Result<u16> {
        // If custom status is set, render it (separator handled by main render)
        if let Some(ref custom) = self.custom_status {
            let mut current_line = start_line;
            for line in custom.lines() {
                execute!(
                    stdout,
                    cursor::MoveTo(0, current_line),
                    SetForegroundColor(self.palette.dimmed),
                    Print(line),
                    ResetColor,
                )?;
                current_line += 1;
            }
            return Ok(current_line);
        }

        // Filter conversations for inputs column
        let input_convs: Vec<_> = conversations
            .iter()
            .filter(|c| matches!(&c.source, crate::state::app_state::ConversationSource::User | crate::state::app_state::ConversationSource::Scripting))
            .collect();

        // Calculate heights for each column
        let inputs_height = if input_convs.is_empty() { 0 } else { 1 + input_convs.len() as u16 };
        let mut servers_height = 0u16;
        if !servers.is_empty() {
            servers_height = 1; // Header
            for server in servers {
                servers_height += 1; // Server line
                let server_conns: Vec<_> = connections.iter().filter(|c| c.server_id == server.id).collect();
                let max_to_show = if expand_all { server_conns.len() } else { 10.min(server_conns.len()) };
                servers_height += max_to_show as u16;
                if !expand_all && server_conns.len() > 10 {
                    servers_height += 1;
                }
            }
        }

        // If both columns are empty, don't render anything
        if inputs_height == 0 && servers_height == 0 {
            return Ok(start_line);
        }

        let total_height = inputs_height.max(servers_height);
        let servers_column_start = INPUTS_LEFT_MARGIN + INPUTS_COLUMN_WIDTH + COLUMN_MARGIN;

        // Render line by line
        for line_offset in 0..total_height {
            let current_line = start_line + line_offset;

            // Determine if we should render inputs column content for this line
            let inputs_start_offset = total_height.saturating_sub(inputs_height);
            let render_inputs = line_offset >= inputs_start_offset;

            // Determine if we should render servers column content for this line
            let servers_start_offset = total_height.saturating_sub(servers_height);
            let render_servers = line_offset >= servers_start_offset;

            // Clear the line first
            execute!(stdout, cursor::MoveTo(0, current_line), Clear(ClearType::CurrentLine))?;

            // Render inputs column
            if render_inputs {
                let inputs_line_idx = line_offset - inputs_start_offset;
                if inputs_line_idx == 0 {
                    // Header line - always use ┌
                    execute!(
                        stdout,
                        cursor::MoveTo(INPUTS_LEFT_MARGIN, current_line),
                        SetForegroundColor(self.palette.separator),
                        Print("┌──── "),
                        ResetColor,
                        Print("Inputs")
                    )?;
                } else {
                    // Content line
                    let conv_idx = (inputs_line_idx - 1) as usize;
                    if conv_idx < input_convs.len() {
                        let conv = input_convs[conv_idx];
                        // Just show the details without the [User] prefix
                        let text = self.truncate_to_width(&conv.details, INPUTS_COLUMN_WIDTH - 2);
                        let is_completed = conv.end_time.is_some();
                        execute!(
                            stdout,
                            cursor::MoveTo(INPUTS_LEFT_MARGIN, current_line),
                            SetForegroundColor(self.palette.separator),
                            Print("│ "),
                            ResetColor,
                        )?;
                        if is_completed {
                            execute!(stdout, SetForegroundColor(self.palette.dimmed))?;
                        }
                        execute!(stdout, Print(&text), ResetColor)?;
                    }
                }
            }

            // Render column separator and servers column
            if render_servers {
                let servers_line_idx = line_offset - servers_start_offset;
                if servers_line_idx == 0 {
                    // Header line - always use ┌
                    execute!(
                        stdout,
                        cursor::MoveTo(servers_column_start, current_line),
                        SetForegroundColor(self.palette.separator),
                        Print("┌──── "),
                        ResetColor,
                        Print("Servers")
                    )?;
                } else {
                    // Content line - need to build server/connection content
                    let mut content_line_idx = servers_line_idx - 1;

                    for server in servers {
                        if content_line_idx == 0 {
                            // This is the server line
                            let text = format!("#{} {} :{} - {}", server.id, server.protocol, server.port, server.status);
                            let is_inactive = server.status == "Stopped" || server.status.starts_with("Error:");
                            execute!(
                                stdout,
                                cursor::MoveTo(servers_column_start, current_line),
                                SetForegroundColor(self.palette.separator),
                                Print("│ "),
                                ResetColor,
                            )?;
                            if is_inactive {
                                execute!(stdout, SetForegroundColor(self.palette.dimmed))?;
                            }
                            execute!(stdout, Print(&text), ResetColor)?;
                            break;
                        }
                        content_line_idx -= 1;

                        // Check connections for this server
                        let server_conns: Vec<_> = connections.iter().filter(|c| c.server_id == server.id).collect();
                        let max_to_show = if expand_all { server_conns.len() } else { 10.min(server_conns.len()) };

                        // Check if this is a connection line or a conversation sub-item
                        let mut found = false;
                        for (conn_idx, conn) in server_conns.iter().take(max_to_show).enumerate() {
                            if conn_idx as u16 == content_line_idx {
                                // This is the connection line
                                let text = format!("  #{} {} {}", conn.id, conn.address, conn.state);
                                let is_closed = conn.state == "Closed";
                                execute!(
                                    stdout,
                                    cursor::MoveTo(servers_column_start, current_line),
                                    SetForegroundColor(self.palette.separator),
                                    Print("│ "),
                                    ResetColor,
                                )?;
                                if is_closed {
                                    execute!(stdout, SetForegroundColor(self.palette.dimmed))?;
                                }
                                execute!(stdout, Print(&text), ResetColor)?;
                                found = true;
                                break;
                            }

                            // Skip past the connection line
                            if content_line_idx == 0 {
                                break;
                            }
                            content_line_idx -= 1;

                            // Check for conversation sub-items for this connection
                            let conn_convs: Vec<_> = conversations
                                .iter()
                                .filter(|conv| {
                                    matches!(&conv.source,
                                        crate::state::app_state::ConversationSource::Network { server_id, connection_id }
                                        if server_id.as_u32().to_string() == server.id && connection_id.map(|id| id.to_string()) == Some(conn.id.to_string())
                                    )
                                })
                                .collect();

                            if content_line_idx < conn_convs.len() as u16 {
                                // This is a conversation sub-item
                                let conv = conn_convs[content_line_idx as usize];
                                let is_completed = conv.end_time.is_some();
                                execute!(
                                    stdout,
                                    cursor::MoveTo(servers_column_start, current_line),
                                    SetForegroundColor(self.palette.separator),
                                    Print("│ "),
                                    ResetColor,
                                )?;
                                if is_completed {
                                    execute!(stdout, SetForegroundColor(self.palette.dimmed))?;
                                }
                                execute!(stdout, Print(format!("    {}", conv.details)), ResetColor)?;
                                found = true;
                                break;
                            }
                            content_line_idx -= conn_convs.len() as u16;
                        }

                        if found {
                            break;
                        }

                        // Check for "... N more" line
                        if !expand_all && server_conns.len() > 10 {
                            if content_line_idx == 0 {
                                execute!(
                                    stdout,
                                    cursor::MoveTo(servers_column_start, current_line),
                                    SetForegroundColor(self.palette.separator),
                                    Print("│ "),
                                    ResetColor,
                                    SetForegroundColor(self.palette.dimmed),
                                    Print(format!("  ... ({} more)", server_conns.len() - 10)),
                                    ResetColor
                                )?;
                                break;
                            }
                            content_line_idx -= 1;
                        }
                    }
                }
            }
        }

        Ok(start_line + total_height)
    }

    /// Truncate text to fit within a given width
    fn truncate_to_width(&self, text: &str, max_width: u16) -> String {
        if text.len() <= max_width as usize {
            text.to_string()
        } else {
            let truncate_at = (max_width as usize).saturating_sub(3);
            format!("{}...", &text[..truncate_at])
        }
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
                    SetForegroundColor(self.palette.dimmed),
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
            SetForegroundColor(self.palette.separator),
            Print(&separator),
            ResetColor,
        )?;
        Ok(line + 1)
    }

    /// Render a separator line with ┴ join characters at column positions
    fn render_separator_with_joins(
        &self,
        stdout: &mut impl Write,
        line: u16,
        content: &FooterContent,
    ) -> Result<u16> {
        // First render the base separator line
        let separator = "─".repeat(self.terminal_width as usize);
        execute!(
            stdout,
            cursor::MoveTo(0, line),
            SetForegroundColor(self.palette.separator),
            Print(&separator),
            ResetColor,
        )?;

        // Add ┴ characters at column positions for Normal content
        if let FooterContent::Normal { servers, conversations, .. } = content {
            // Filter conversations for inputs column
            let input_convs: Vec<_> = conversations
                .iter()
                .filter(|c| matches!(&c.source, crate::state::app_state::ConversationSource::User | crate::state::app_state::ConversationSource::Scripting))
                .collect();

            // Add ┴ at inputs column position if inputs exist
            if !input_convs.is_empty() {
                execute!(
                    stdout,
                    cursor::MoveTo(INPUTS_LEFT_MARGIN, line),
                    SetForegroundColor(self.palette.separator),
                    Print("┴"),
                    ResetColor,
                )?;
            }

            // Add ┴ at servers column position if servers exist
            if !servers.is_empty() {
                let servers_column_start = INPUTS_LEFT_MARGIN + INPUTS_COLUMN_WIDTH + COLUMN_MARGIN;
                execute!(
                    stdout,
                    cursor::MoveTo(servers_column_start, line),
                    SetForegroundColor(self.palette.separator),
                    Print("┴"),
                    ResetColor,
                )?;
            }
        }

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
        let (web_status, web_color) = match self.connection_info.web_search_mode {
            WebSearchMode::On => ("ON", self.palette.success),
            WebSearchMode::Off => ("OFF", self.palette.failure),
            WebSearchMode::Ask => ("ASK", self.palette.ask),
        };

        // Determine script status and color based on scripting_env value
        // scripting_env contains the mode: "Off", "On", "Python", "JavaScript", "Go", "Perl"
        let (script_status, script_color) = if self.connection_info.scripting_env == "Off" {
            ("OFF", self.palette.failure)
        } else if self.connection_info.scripting_env.is_empty() {
            ("OFF", self.palette.failure)
        } else {
            (self.connection_info.scripting_env.as_str(), self.palette.success)
        };

        execute!(stdout, cursor::MoveTo(0, line))?;

        // Print each segment with appropriate coloring
        execute!(
            stdout,
            Print(" "),
            SetForegroundColor(self.palette.dimmed),
            Print("↓"),
            ResetColor,
            Print(format!("{}", self.packet_stats.packets_received)),
            SetForegroundColor(self.palette.dimmed),
            Print(" ↑"),
            ResetColor,
            Print(format!("{}", self.packet_stats.packets_sent)),
            SetForegroundColor(self.palette.dimmed),
            Print(" | Model:"),
            ResetColor,
            Print(format!("{}", &self.connection_info.model)),
            SetForegroundColor(self.palette.dimmed),
            Print(" | ^l Log:"),
            ResetColor,
            SetForegroundColor(self.log_level.color()),
            Print(format!("{}", self.log_level.as_str())),
            ResetColor,
            SetForegroundColor(self.palette.dimmed),
            Print(" | ^e Script:"),
            ResetColor,
            SetForegroundColor(script_color),
            Print(format!("{}", script_status)),
            ResetColor,
            SetForegroundColor(self.palette.dimmed),
            Print(" | ^w WebSearch:"),
            ResetColor,
            SetForegroundColor(web_color),
            Print(format!("{}", web_status)),
            ResetColor,
        )?;

        // Add privilege warnings if capabilities are unavailable
        if !self.system_capabilities.can_bind_privileged_ports {
            execute!(
                stdout,
                SetForegroundColor(self.palette.dimmed),
                Print(" |"),
                ResetColor,
                SetForegroundColor(self.palette.ask),
                Print(" Ports<1024 denied"),
                ResetColor,
            )?;
        }

        if !self.system_capabilities.has_raw_socket_access {
            execute!(
                stdout,
                SetForegroundColor(self.palette.dimmed),
                Print(" |"),
                ResetColor,
                SetForegroundColor(self.palette.ask),
                Print(" PCAP denied"),
                ResetColor,
            )?;
        }

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

    /// Render approval prompt when web search approval is pending
    fn render_approval_prompt(&self, stdout: &mut impl Write, start_line: u16, url: &str) -> Result<u16> {
        let mut current_line = start_line;

        // Parse URL to extract protocol, domain and path
        let (protocol, domain, path) = if url.starts_with("http://") || url.starts_with("https://") {
            // Parse as URL
            if let Ok(parsed_url) = url::Url::parse(url) {
                let protocol = parsed_url.scheme().to_string() + "://";
                let domain = parsed_url.host_str().unwrap_or("unknown").to_string();
                let path = parsed_url.path().to_string();
                (protocol, domain, path)
            } else {
                // Fallback: treat whole thing as domain
                (String::new(), url.to_string(), String::new())
            }
        } else {
            // Treat as search query
            (String::new(), "search".to_string(), url.to_string())
        };

        // Calculate available width for URL display
        // "Web Search Request: " = 20 chars
        // " | (Y)es | (N)o | (A)llow All" = 30 chars
        // Total prefix/suffix = 50 chars
        let available_width = self.terminal_width.saturating_sub(50) as usize;

        // Format the URL parts
        let protocol_part = protocol;
        let domain_part = domain;
        let path_part = if path.is_empty() {
            String::new()
        } else {
            // Calculate remaining width after protocol and domain
            let used_width = protocol_part.len() + domain_part.len();
            let remaining_width = available_width.saturating_sub(used_width);
            if path.len() <= remaining_width {
                path
            } else if remaining_width > 3 {
                format!("{}...", &path[..remaining_width.saturating_sub(3)])
            } else {
                String::new()
            }
        };

        // Render the approval prompt
        execute!(stdout, cursor::MoveTo(0, current_line))?;
        execute!(
            stdout,
            Print(" Web Search Request: "),
            // Protocol in grey (de-emphasized)
            SetForegroundColor(self.palette.dimmed),
            Print(&protocol_part),
            // Domain in normal color (stands out)
            SetForegroundColor(self.palette.normal),
            Print(&domain_part),
            // Path in grey (de-emphasized)
            SetForegroundColor(self.palette.dimmed),
            Print(&path_part),
            ResetColor,
            Print(" | "),
            SetForegroundColor(self.palette.success),
            Print("(Y)"),
            ResetColor,
            Print("es | "),
            SetForegroundColor(self.palette.failure),
            Print("(N)"),
            ResetColor,
            Print("o | "),
            SetForegroundColor(self.palette.info),
            Print("(A)"),
            ResetColor,
            Print("llow All"),
        )?;

        current_line += 1;
        Ok(current_line)
    }
}
