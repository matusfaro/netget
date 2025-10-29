//! Rolling terminal TUI - output flows like tail -f with sticky footer
//!
//! This module implements the interactive TUI mode using a rolling terminal
//! approach where output naturally scrolls into the terminal's scrollback buffer,
//! while input and connection info remain sticky at the bottom.

use anyhow::Result;
use chrono::Local;
use crossterm::{
    cursor,
    event::{Event, EventStream, KeyCode, KeyModifiers},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal,
};
use futures::StreamExt;
use std::io::{stdout, Write};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{interval, Instant};
use tracing::{error, info};

use crate::events::{EventHandler, UserCommand};
use crate::llm::OllamaClient;
use crate::settings::Settings;
use crate::state::app_state::AppState;
use crate::ui::{app::LogLevel, App};

use super::input_state::InputState;
use super::sticky_footer::{ConnectionInfo, FooterContent, StickyFooter};

/// Run the interactive rolling TUI mode
pub async fn run_rolling_tui(
    state: AppState,
    mut app: App,
    mut event_handler: EventHandler,
    _llm_client: OllamaClient, // Kept for API compatibility
    settings: Settings,
    args: &super::Args,
) -> Result<()> {
    info!("Starting rolling TUI mode");

    // Override model if specified in args, otherwise use settings
    let effective_model = if let Some(model) = &args.model {
        model.clone()
    } else {
        settings.model.clone()
    };

    state.set_ollama_model(effective_model.clone()).await;
    app.connection_info.model = effective_model;

    // Setup terminal (raw mode only, no alternate screen)
    terminal::enable_raw_mode()?;

    // Get terminal size (use defaults if detection fails or returns 0, e.g., in PTY tests)
    let (width, height) = match terminal::size() {
        Ok((w, h)) if w > 0 && h > 0 => (w, h),
        _ => (80, 24), // Default to 80x24 if size detection fails or returns 0
    };

    // Create sticky footer
    let mut footer = StickyFooter::new(width, height)?;
    let scroll_height = footer.scroll_region_height();
    let footer_height = height.saturating_sub(scroll_height);

    // BEFORE setting scrolling region, push any existing terminal content up
    // by printing newlines. This makes room for the footer without overwriting content.
    // Move to actual bottom of terminal using a large line number that will clamp.
    // Note: terminal::size() may return wrong values in PTY tests, so we use ESC[9999;1H
    // which moves to line 9999 (clamped to actual terminal height) instead of relying on detected height
    print!("\x1b[9999;1H"); // CSI 9999;1 H - Move to line 9999, column 1 (clamps to actual terminal bottom)
    stdout().flush()?;

    // Print footer_height newlines to push existing content up
    for _ in 0..footer_height {
        execute!(stdout(), Print("\n"))?;
    }
    stdout().flush()?;

    // Now set up scrolling region (lines 1 to scroll_region_height)
    // This tells the terminal that only these lines should scroll, keeping footer fixed
    // DECSTBM: ESC[<top>;<bottom>r - Set scrolling region
    print!("\x1b[1;{}r", scroll_height);
    stdout().flush()?;

    footer.set_connection_info(ConnectionInfo {
        mode: app.connection_info.mode.clone(),
        protocol: app.connection_info.protocol.clone(),
        model: app.connection_info.model.clone(),
        local_addr: app.connection_info.local_addr.clone(),
    });
    footer.set_packet_stats(app.packet_stats.clone());
    footer.set_log_level(app.log_level);

    // Print welcome messages to scrolling region
    print_welcome_messages(&mut footer)?;

    // NOTE: No initial footer render here - the event loop will render it
    // after processing the "TUI initialized" message we send at line ~134

    // Create status channel for server messages
    let (status_tx, mut status_rx) = mpsc::unbounded_channel::<String>();

    // Wrap settings for sharing
    let _settings = Arc::new(Mutex::new(settings));

    // Create keyboard event stream
    let mut event_stream = EventStream::new();

    // Create tick interval for UI updates
    let mut tick_interval = interval(Duration::from_millis(100));

    // Cleanup configuration constants
    const CLEANUP_INTERVAL_SECS: u64 = 5;
    const SERVER_CLEANUP_TIMEOUT_SECS: u64 = 30;
    const CONNECTION_CLEANUP_TIMEOUT_SECS: u64 = 10;
    const CONNECTIONLESS_CLEANUP_TIMEOUT_SECS: u64 = 10;

    // Create cleanup interval
    let mut cleanup_interval = interval(Duration::from_secs(CLEANUP_INTERVAL_SECS));

    // Create test interval for debugging footer behavior (disabled for stable snapshots)
    // Set to a very long duration so it doesn't fire during tests
    let mut test_interval = tokio::time::interval_at(
        Instant::now() + Duration::from_secs(3600), // Start in 1 hour
        Duration::from_secs(3600) // Tick every hour
    );
    test_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Counter for test heartbeats
    let mut heartbeat_counter = 0u64;

    // Main event loop
    info!("Entering main event loop");

    // Send a test message immediately
    let _ = status_tx.send("[INFO] TUI initialized, event loop starting".to_string());

    loop {
        // Drain status messages from spawned tasks
        let mut ui_needs_update = false;
        while let Ok(msg) = status_rx.try_recv() {
            if msg == "__UPDATE_UI__" {
                // Special signal to update UI from state
                ui_needs_update = true;
            } else if msg.starts_with("__STATS_SENT__") {
                // Special signal to update sent bytes stats
                if let Ok(bytes) = msg.strip_prefix("__STATS_SENT__").unwrap().parse::<u64>() {
                    app.packet_stats.bytes_sent += bytes;
                    footer.set_packet_stats(app.packet_stats.clone());
                    footer.render(&mut stdout())?;
                }
            } else {
                // Filter messages by log level
                let should_show = if msg.starts_with("[ERROR]") {
                    true
                } else if msg.starts_with("[WARN]") {
                    app.log_level >= LogLevel::Warn
                } else if msg.starts_with("[INFO]") {
                    app.log_level >= LogLevel::Info
                } else if msg.starts_with("[DEBUG]") {
                    app.log_level >= LogLevel::Debug
                } else if msg.starts_with("[TRACE]") {
                    app.log_level >= LogLevel::Trace
                } else {
                    // Unprefixed messages always show
                    true
                };

                if should_show {
                    print_output_line(&msg, &mut footer)?;
                    ui_needs_update = true;
                }
            }
        }

        tokio::select! {
            // Keyboard events
            maybe_event = event_stream.next() => {
                match maybe_event {
                    Some(Ok(event)) => {
                        if handle_event(event, &mut app, &state, &mut event_handler, &status_tx, &mut footer).await? {
                            info!("Quit requested by user");
                            break; // Quit requested
                        }
                    }
                    Some(Err(e)) => {
                        error!("Keyboard event error: {}", e);
                    }
                    None => {
                        info!("Event stream ended unexpectedly");
                        break;
                    }
                }
            }

            // Periodic tick for UI updates
            _ = tick_interval.tick() => {
                // Just triggers potential updates
            }

            // Test timer - prints a message every second for debugging
            _ = test_interval.tick() => {
                let timestamp = Local::now().format("%H:%M:%S");
                let msg = format!("[DEBUG] Test heartbeat #{} at {}", heartbeat_counter, timestamp);
                info!("Test heartbeat firing: {}", msg);
                print_output_line(&msg, &mut footer)?;
                // Immediately re-render footer after printing
                footer.render(&mut stdout())?;
                heartbeat_counter += 1;
                ui_needs_update = false; // Already rendered
            }

            // Periodic cleanup of old servers and connections
            _ = cleanup_interval.tick() => {
                state.cleanup_old_servers(SERVER_CLEANUP_TIMEOUT_SECS).await;
                state.cleanup_closed_connections(CONNECTION_CLEANUP_TIMEOUT_SECS).await;
                state.cleanup_old_connections(CONNECTIONLESS_CLEANUP_TIMEOUT_SECS).await;
            }
        }

        // Update UI after handling events
        if ui_needs_update {
            update_ui_from_state(&mut app, &state, &mut footer).await;
            footer.render(&mut stdout())?;
        }
    }

    // Cleanup terminal
    // Reset scrolling region to full terminal (DECSTBM with no args)
    print!("\x1b[r");
    // Clear the sticky footer before exiting
    clear_sticky_footer(&footer)?;
    terminal::disable_raw_mode()?;
    println!(); // Final newline

    // Save command history before exiting
    let _ = app.save_history();
    info!("Rolling TUI mode exited");

    Ok(())
}

/// Print welcome messages to the scrolling region
fn print_welcome_messages(footer: &mut StickyFooter) -> Result<()> {
    let messages = vec![
        "NetGet - LLM-Controlled Network Application",
        "All protocol responses are generated by LLM",
        "",
        "Supported protocol stacks:",
        "  TCP (Beta): \"Pretend to be FTP server on port 2121; serve file accounts.csv with 'balance,0'\"",
        "  HTTP (Beta): \"Pretend to be a sassy HTTP server on port 8080 serving cooking recipes\"",
        "  SSH/SFTP (Beta): \"Pretent to be a shell via SSH on port 2222\"",
        "  DNS (Beta): \"DNS server on port 5252 and resolve everything to 1.2.3.4\"",
        "  NTP (Beta; root-only): \"pretend to be a ntp server on port 123\"",
        "  SNMP (Alpha): \"SNMP Port 8161 serve OID 1.3.6.1.2.1.1.1.0 (sysDescr) return 'NetGet SNMP Server v0.1'\"",
        "  IRC (Alpha): \"Start an IRC server\"",
        "  Telnet (Alpha): \"Start a telnet server on port 23 that echoes commands\"",
        "  SMTP (Alpha): \"Start an SMTP mail server on port 25\"",
        "  mDNS (Alpha): \"Advertise a web service via mDNS on port 8080\"",
        "  Ethernet (Alpha; root-only)",
        "  UDP (Alpha)",
        "  DHCP (Alpha)",
        "",
        "Other prompts:",
        "  ",
        "",
    ];

    for msg in messages {
        print_output_line(msg, footer)?;
    }

    Ok(())
}

/// Print a line to stdout (scrolls naturally within scroll region - no flickering!)
fn print_output_line(line: &str, footer: &mut StickyFooter) -> Result<()> {
    let mut stdout = stdout();

    // Move cursor to the LAST line of the scrolling region
    // The scrolling region is set to lines 1-scroll_region_height (1-indexed)
    // cursor::MoveTo uses 0-indexed coordinates, so last line is scroll_region_height - 1
    // When we print with \n, the scroll region will scroll naturally,
    // and the footer (outside the scroll region) will remain in place - no flickering!
    let scroll_height = footer.scroll_region_height();
    let last_scroll_line = scroll_height.saturating_sub(1); // 0-indexed

    // Position cursor at the last line of scroll region
    execute!(stdout, cursor::MoveTo(0, last_scroll_line))?;

    if line.starts_with("[ERROR]") {
        execute!(
            stdout,
            SetForegroundColor(Color::Red),
            Print("✗ "),
            ResetColor,
            Print(line.strip_prefix("[ERROR]").unwrap()),
        )?;
    } else if line.starts_with("[WARN]") {
        execute!(
            stdout,
            SetForegroundColor(Color::Yellow),
            Print("⚠ "),
            ResetColor,
            Print(line.strip_prefix("[WARN]").unwrap()),
        )?;
    } else if line.starts_with("[INFO]") {
        execute!(
            stdout,
            SetForegroundColor(Color::Blue),
            Print("● "),
            ResetColor,
            Print(line.strip_prefix("[INFO]").unwrap()),
        )?;
    } else if line.starts_with("[DEBUG]") {
        execute!(
            stdout,
            SetForegroundColor(Color::Cyan),
            Print("○ "),
            ResetColor,
            Print(line.strip_prefix("[DEBUG]").unwrap()),
        )?;
    } else if line.starts_with("[TRACE]") {
        execute!(
            stdout,
            SetForegroundColor(Color::DarkGrey),
            Print("· "),
            Print(line.strip_prefix("[TRACE]").unwrap()),
            ResetColor,
        )?;
    } else if line.starts_with("[USER]") {
        execute!(
            stdout,
            SetForegroundColor(Color::Green),
            Print("▶ "),
            ResetColor,
            Print(line.strip_prefix("[USER]").unwrap()),
        )?;
    } else if line.starts_with("[SERVER]") {
        execute!(
            stdout,
            SetForegroundColor(Color::Cyan),
            Print("◆ "),
            ResetColor,
            Print(line.strip_prefix("[SERVER]").unwrap()),
        )?;
    } else if line.starts_with("[CONN]") {
        execute!(
            stdout,
            SetForegroundColor(Color::Cyan),
            Print("◇ "),
            ResetColor,
            Print(line.strip_prefix("[CONN]").unwrap()),
        )?;
    } else {
        execute!(stdout, Print(line))?;
    }

    // Print newline - this will scroll the terminal up by one line
    execute!(stdout, Print("\n"))?;
    stdout.flush()?;

    // IMPORTANT: After printing a line, decrement the blank lines buffer
    // This line now occupies what was previously a blank line at the top
    footer.decrement_blank_lines_buffer();

    Ok(())
}

/// Clear the sticky footer area
fn clear_sticky_footer(footer: &StickyFooter) -> Result<()> {
    let mut stdout = stdout();
    let (_, height) = terminal::size()?;
    let footer_height = footer.calculate_footer_height();
    let footer_start = height.saturating_sub(footer_height);

    // Clear footer lines
    for line in footer_start..height {
        execute!(
            stdout,
            cursor::MoveTo(0, line),
            terminal::Clear(terminal::ClearType::CurrentLine),
        )?;
    }

    stdout.flush()?;
    Ok(())
}

/// Handle keyboard and other events
async fn handle_event(
    event: Event,
    app: &mut App,
    state: &AppState,
    event_handler: &mut EventHandler,
    status_tx: &mpsc::UnboundedSender<String>,
    footer: &mut StickyFooter,
) -> Result<bool> {
    match event {
        Event::Key(key) => {
            handle_key_event(key.code, key.modifiers, app, state, event_handler, status_tx, footer).await
        }
        Event::Resize(width, height) => {
            footer.handle_resize(width, height);
            footer.render(&mut stdout())?;
            Ok(false)
        }
        _ => Ok(false),
    }
}

/// Handle keyboard key events
async fn handle_key_event(
    key_code: KeyCode,
    modifiers: KeyModifiers,
    app: &mut App,
    state: &AppState,
    event_handler: &mut EventHandler,
    status_tx: &mpsc::UnboundedSender<String>,
    footer: &mut StickyFooter,
) -> Result<bool> {
    // Handle special keys first
    match key_code {
        // Ctrl+C to quit
        KeyCode::Char('c') | KeyCode::Char('C') if modifiers.contains(KeyModifiers::CONTROL) => {
            return Ok(true);
        }

        // Ctrl+N or Alt+N to insert newline
        KeyCode::Char('n') | KeyCode::Char('N') if modifiers.contains(KeyModifiers::CONTROL) || modifiers.contains(KeyModifiers::ALT) => {
            footer.input_mut().insert_newline();
            app.update_slash_suggestions(&footer.input().text());
            footer.render_input_only(&mut stdout())?;
            return Ok(false);
        }

        // Enter to submit (plain enter only, not with modifiers)
        KeyCode::Enter if !modifiers.contains(KeyModifiers::SHIFT) && !modifiers.contains(KeyModifiers::CONTROL) && !modifiers.contains(KeyModifiers::ALT) => {
            let text = footer.input().text();
            if !text.is_empty() {
                // Add to history
                app.add_to_history(text.clone());

                // Parse command
                let command = UserCommand::parse(&text);

                // IMPORTANT: For SetFooterStatus and TestOutput, we DON'T print the command echo
                // - SetFooterStatus: Avoids positioning issues during footer expansion/shrinking
                // - TestOutput: Direct scroll region manipulation makes the echo unnecessary
                let print_echo_before = !matches!(
                    command,
                    UserCommand::SetFooterStatus { .. } | UserCommand::TestOutput { .. }
                );

                if print_echo_before {
                    print_output_line(&format!("[USER] {}", text), footer)?;
                }

                // Handle command
                match command {
                    UserCommand::Status | UserCommand::ShowModel | UserCommand::ShowLogLevel => {
                        // Handle status/info commands
                        handle_status_command(&command, app, state, footer)?;
                    }
                    UserCommand::ChangeModel { model } => {
                        state.set_ollama_model(model.clone()).await;
                        app.connection_info.model = model.clone();
                        print_output_line(&format!("Model changed to: {}", model), footer)?;
                        update_ui_from_state(app, state, footer).await;
                        footer.render(&mut stdout())?;
                    }
                    UserCommand::ChangeLogLevel { level } => {
                        if let Some(log_level) = crate::ui::app::LogLevel::from_str(&level) {
                            app.set_log_level(log_level);
                            footer.set_log_level(log_level);
                            print_output_line(&format!("Log level set to: {}", log_level.as_str()), footer)?;
                            footer.render(&mut stdout())?;
                        } else {
                            print_output_line(&format!("Unknown log level: {}", level), footer)?;
                        }
                    }
                    UserCommand::TestOutput { count } => {
                        // Generate test output lines using print_output_line (scrolling mechanism)
                        // This ensures content is properly preserved during footer expansion/shrinking
                        for i in 1..=count {
                            print_output_line(&format!("Test line {} of {}", i, count), footer)?;
                        }

                        // Re-render footer
                        footer.render(&mut stdout())?;
                    }
                    UserCommand::SetFooterStatus { message } => {
                        use std::fs::OpenOptions;
                        use std::io::Write as IoWrite;

                        // Write debug info to file
                        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open("/tmp/netget_debug.log") {
                            let _ = writeln!(file, "[DEBUG] SetFooterStatus handler called with message: {:?}", message);
                        }

                        // Get current terminal dimensions from footer (terminal::size() returns 0 in PTY)
                        let term_width = footer.terminal_width();
                        let term_height = footer.terminal_height();

                        // Calculate old and new footer heights
                        let old_scroll_height = footer.scroll_region_height();
                        let old_footer_height = term_height.saturating_sub(old_scroll_height);
                        let old_footer_start = term_height.saturating_sub(old_footer_height);

                        // Set custom footer status message (this recalculates footer height)
                        footer.set_custom_status(message.clone());

                        let new_scroll_height = footer.scroll_region_height();
                        let new_footer_height = term_height.saturating_sub(new_scroll_height);

                        // Write footer height info to file
                        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open("/tmp/netget_debug.log") {
                            let _ = writeln!(file, "[DEBUG] Footer heights: old={}, new={}, term_height={}",
                                old_footer_height, new_footer_height, term_height);
                        }

                        // Handle footer size changes
                        if new_footer_height > old_footer_height {
                            // Footer is EXPANDING (e.g., 5 lines → 7 lines, increase by 2)
                            let lines_to_add = new_footer_height - old_footer_height;

                            // Try to consume from blank lines buffer first
                            let consumed = footer.consume_blank_lines_buffer(lines_to_add);
                            let lines_to_push = lines_to_add - consumed;

                            // Write debug info to file
                            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open("/tmp/netget_debug.log") {
                                let _ = writeln!(file, "[DEBUG-EXPAND] Footer expanding: old_height={}, new_height={}, lines_to_add={}, consumed={}, lines_to_push={}",
                                    old_footer_height, new_footer_height, lines_to_add, consumed, lines_to_push);
                            }

                            // If buffer didn't have enough space, push content up BEFORE changing scroll region
                            if lines_to_push > 0 {
                                // Move cursor to bottom of the OLD scroll region (0-indexed)
                                let last_old_scroll_line = old_scroll_height.saturating_sub(1);
                                execute!(stdout(), cursor::MoveTo(0, last_old_scroll_line))?;

                                // Print newlines to scroll content up within the OLD scroll region
                                // This preserves all content by scrolling it up before we shrink the region
                                for _ in 0..lines_to_push {
                                    execute!(stdout(), Print("\n"))?;
                                }
                                stdout().flush()?;
                            }

                            // NOW set the new (smaller) scrolling region
                            print!("\x1b[1;{}r", new_scroll_height);
                            stdout().flush()?;

                            // Footer.render() will clear and draw the footer area
                        } else if new_footer_height < old_footer_height {
                            // Footer is SHRINKING (e.g., 7 lines → 5 lines, decrease by 2)
                            let lines_to_remove = old_footer_height - new_footer_height;

                            // Add shrunk lines to blank lines buffer - they become available blank lines at top
                            footer.add_to_blank_lines_buffer(lines_to_remove);

                            // Write debug info to file
                            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open("/tmp/netget_debug.log") {
                                let _ = writeln!(file, "[DEBUG-SHRINK] Footer shrinking: lines_to_remove={}, buffer now={}",
                                    lines_to_remove, footer.blank_lines_buffer());
                            }

                            // Step 1: Clear the top N lines of the old footer (where N = lines_to_remove)
                            let blank_line = " ".repeat(term_width as usize);
                            for line_offset in 0..lines_to_remove {
                                execute!(
                                    stdout(),
                                    cursor::MoveTo(0, old_footer_start + line_offset),
                                    Print(&blank_line),
                                )?;
                            }
                            stdout().flush()?;

                            // Step 2: Update scrolling region to new height
                            print!("\x1b[1;{}r", new_scroll_height);
                            stdout().flush()?;
                        } else {
                            // Footer size UNCHANGED - no buffer manipulation needed
                            // Just log for debugging
                            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open("/tmp/netget_debug.log") {
                                let _ = writeln!(file, "[DEBUG-UNCHANGED] Footer size unchanged: height={}, buffer={}",
                                    new_footer_height, footer.blank_lines_buffer());
                            }
                        }

                        // Step 4 (all cases): Redraw the footer at the new position
                        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open("/tmp/netget_debug.log") {
                            let final_scroll_height = footer.scroll_region_height();
                            let final_footer_height = footer.terminal_height().saturating_sub(final_scroll_height);
                            let final_footer_start = footer.terminal_height().saturating_sub(final_footer_height);
                            let _ = writeln!(file, "[DEBUG] Before footer.render(): scroll_height={}, footer_height={}, footer_start={}",
                                final_scroll_height, final_footer_height, final_footer_start);
                        }
                        footer.render(&mut stdout())?;

                        // Command echo is suppressed for SetFooterStatus (see print_echo_before logic above)
                    }
                    UserCommand::Quit => {
                        return Ok(true);
                    }
                    UserCommand::UnknownSlashCommand { command } => {
                        print_output_line(&format!("Unknown command: {}", command), footer)?;
                    }
                    UserCommand::Interpret { input: llm_input } => {
                        // Spawn async task to process with LLM
                        let mut handler_clone = event_handler.clone();
                        let status_tx_clone = status_tx.clone();
                        tokio::spawn(async move {
                            let _ = handler_clone.handle_interpret_with_actions(llm_input, status_tx_clone, None).await;
                        });
                    }
                }

                // Clear input
                footer.input_mut().clear();
                app.update_slash_suggestions(&footer.input().text());
            }

            footer.render(&mut stdout())?;
            return Ok(false);
        }

        // Up arrow - command history navigation
        KeyCode::Up if footer.input().is_on_first_line() => {
            navigate_history_previous(app, footer);
            app.update_slash_suggestions(&footer.input().text());
            footer.render_input_only(&mut stdout())?;
            return Ok(false);
        }

        // Down arrow - command history navigation
        KeyCode::Down if footer.input().is_on_last_line() => {
            navigate_history_next(app, footer);
            app.update_slash_suggestions(&footer.input().text());
            footer.render_input_only(&mut stdout())?;
            return Ok(false);
        }

        // Ctrl+A - move to start of line
        KeyCode::Char('a') | KeyCode::Char('A') if modifiers.contains(KeyModifiers::CONTROL) => {
            footer.input_mut().move_to_start_of_line();
            footer.render_input_only(&mut stdout())?;
            return Ok(false);
        }

        // Ctrl+E - move to end of line
        KeyCode::Char('e') | KeyCode::Char('E') if modifiers.contains(KeyModifiers::CONTROL) => {
            footer.input_mut().move_to_end_of_line();
            footer.render_input_only(&mut stdout())?;
            return Ok(false);
        }

        // Ctrl+K - delete to end of line
        KeyCode::Char('k') | KeyCode::Char('K') if modifiers.contains(KeyModifiers::CONTROL) => {
            footer.input_mut().delete_to_end_of_line();
            app.update_slash_suggestions(&footer.input().text());
            footer.render_input_only(&mut stdout())?;
            return Ok(false);
        }

        // Ctrl+U - delete entire line
        KeyCode::Char('u') | KeyCode::Char('U') if modifiers.contains(KeyModifiers::CONTROL) => {
            footer.input_mut().delete_line();
            app.update_slash_suggestions(&footer.input().text());
            footer.render_input_only(&mut stdout())?;
            return Ok(false);
        }

        // Ctrl+W - delete word
        KeyCode::Char('w') | KeyCode::Char('W') if modifiers.contains(KeyModifiers::CONTROL) => {
            footer.input_mut().delete_word();
            app.update_slash_suggestions(&footer.input().text());
            footer.render_input_only(&mut stdout())?;
            return Ok(false);
        }

        // E key - toggle expand all (if not typing)
        KeyCode::Char('e') | KeyCode::Char('E') if !modifiers.contains(KeyModifiers::CONTROL) && footer.input().text().is_empty() => {
            app.toggle_expand_all();
            update_ui_from_state(app, state, footer).await;
            footer.render(&mut stdout())?;
            return Ok(false);
        }

        _ => {}
    }

    // Try to handle with InputState
    if footer.input_mut().handle_key(key_code, modifiers) {
        app.update_slash_suggestions(&footer.input().text());
        // Only update the input line, not the entire footer
        footer.render_input_only(&mut stdout())?;
        return Ok(false);
    }

    Ok(false)
}

/// Navigate to previous command in history
fn navigate_history_previous(app: &mut App, footer: &mut StickyFooter) {
    if app.command_history.is_empty() {
        return;
    }

    let input = footer.input_mut();
    match app.history_position {
        None => {
            // Starting history navigation - save current input
            let current = input.text();
            if !current.is_empty() {
                app.history_temp_input = Some(current);
            }
            // Go to most recent command
            let pos = app.command_history.len() - 1;
            app.history_position = Some(pos);
            *input = InputState::from_lines(
                app.command_history[pos]
                    .lines()
                    .map(|s| s.to_string())
                    .collect(),
            );
            input.move_to_top();
        }
        Some(pos) if pos > 0 => {
            // Go to older command
            let new_pos = pos - 1;
            app.history_position = Some(new_pos);
            *input = InputState::from_lines(
                app.command_history[new_pos]
                    .lines()
                    .map(|s| s.to_string())
                    .collect(),
            );
            input.move_to_top();
        }
        _ => {
            // Already at oldest command, do nothing
        }
    }
}

/// Navigate to next command in history
fn navigate_history_next(app: &mut App, footer: &mut StickyFooter) {
    let input = footer.input_mut();
    match app.history_position {
        Some(pos) if pos < app.command_history.len() - 1 => {
            // Go to newer command
            let new_pos = pos + 1;
            app.history_position = Some(new_pos);
            *input = InputState::from_lines(
                app.command_history[new_pos]
                    .lines()
                    .map(|s| s.to_string())
                    .collect(),
            );
            input.move_to_bottom();
        }
        Some(_) => {
            // At newest command, restore temp input or clear
            app.history_position = None;
            let temp = app.history_temp_input.take().unwrap_or_default();
            *input = InputState::from_lines(temp.lines().map(|s| s.to_string()).collect());
            input.move_to_bottom();
        }
        None => {
            // Not in history mode, do nothing
        }
    }
}

// Slash suggestion updates are now done directly via app.update_slash_suggestions()

/// Update UI with current application state
async fn update_ui_from_state(app: &mut App, state: &AppState, footer: &mut StickyFooter) {
    use crate::ui::app::{ConnectionDisplayInfo, ServerDisplayInfo};

    // Track old footer height BEFORE updating content
    let old_scroll_height = footer.scroll_region_height();
    let term_height = footer.terminal_height();
    let old_footer_height = term_height.saturating_sub(old_scroll_height);

    app.connection_info.mode = state.get_mode().await.to_string();
    app.connection_info.model = state.get_ollama_model().await;

    // Update server list
    let servers = state.get_all_servers().await;
    app.servers = servers
        .iter()
        .map(|s| ServerDisplayInfo {
            id: format!("#{}", s.id.as_u32()),
            protocol: s.base_stack.to_string(),
            port: s.port,
            status: s.status.to_string(),
            connections: s.connections.len(),
        })
        .collect();

    // Update connection list - collect into a temporary vec to avoid borrow issues
    let mut connections = Vec::new();
    for s in &servers {
        for conn in s.connections.values() {
            let network_conn_id = conn.id.to_string();
            let global_id = app.get_or_allocate_connection_id(network_conn_id);
            connections.push(ConnectionDisplayInfo {
                id: global_id,
                server_id: format!("#{}", s.id.as_u32()),
                address: conn.remote_addr.to_string(),
                state: match conn.status {
                    crate::state::server::ConnectionStatus::Active => "Active".to_string(),
                    crate::state::server::ConnectionStatus::Closed => "Closed".to_string(),
                },
            });
        }
    }
    app.connections = connections;

    // Update footer content (this recalculates scroll region)
    if app.slash_suggestions.is_empty() {
        footer.set_content(FooterContent::Normal {
            servers: app.servers.clone(),
            connections: app.connections.clone(),
            expand_all: app.expand_all_connections,
        });
    } else {
        footer.set_content(FooterContent::SlashCommands {
            suggestions: app.slash_suggestions.clone(),
        });
    }

    // Update connection info
    if let Some(first_server) = servers.first() {
        app.connection_info.protocol = first_server.base_stack.to_string();
        if let Some(addr) = first_server.local_addr {
            app.connection_info.local_addr = Some(addr.to_string());
        }
    }

    footer.set_connection_info(ConnectionInfo {
        mode: app.connection_info.mode.clone(),
        protocol: app.connection_info.protocol.clone(),
        model: app.connection_info.model.clone(),
        local_addr: app.connection_info.local_addr.clone(),
    });

    // CRITICAL: Handle footer size changes (expansion/shrinking)
    let new_scroll_height = footer.scroll_region_height();
    let new_footer_height = term_height.saturating_sub(new_scroll_height);

    if new_footer_height != old_footer_height {
        let term_width = footer.terminal_width();
        let old_footer_start = term_height.saturating_sub(old_footer_height);

        if new_footer_height > old_footer_height {
            // Footer is EXPANDING (e.g., connection added, causing footer to grow)
            let lines_to_add = new_footer_height - old_footer_height;

            // Try to consume from blank lines buffer first
            let consumed = footer.consume_blank_lines_buffer(lines_to_add);
            let lines_to_push = lines_to_add - consumed;

            // If buffer didn't have enough space, push content up BEFORE changing scroll region
            if lines_to_push > 0 {
                // Move cursor to bottom of the OLD scroll region (0-indexed)
                let last_old_scroll_line = old_scroll_height.saturating_sub(1);
                execute!(stdout(), cursor::MoveTo(0, last_old_scroll_line)).ok();

                // Print newlines to scroll content up within the OLD scroll region
                // This preserves all content by scrolling it up before we shrink the region
                for _ in 0..lines_to_push {
                    execute!(stdout(), Print("\n")).ok();
                }
                stdout().flush().ok();
            }

            // NOW set the new (smaller) scrolling region
            print!("\x1b[1;{}r", new_scroll_height);
            stdout().flush().ok();

            // Footer.render() will clear and draw the footer area

        } else if new_footer_height < old_footer_height {
            // Footer is SHRINKING (e.g., connection removed, causing footer to shrink)
            let lines_to_remove = old_footer_height - new_footer_height;

            // Add shrunk lines to blank lines buffer
            footer.add_to_blank_lines_buffer(lines_to_remove);

            // Clear the top N lines of the old footer
            let blank_line = " ".repeat(term_width as usize);
            for line_offset in 0..lines_to_remove {
                execute!(
                    stdout(),
                    cursor::MoveTo(0, old_footer_start + line_offset),
                    Print(&blank_line),
                ).ok();
            }
            stdout().flush().ok();

            // Update scrolling region to new height
            print!("\x1b[1;{}r", new_scroll_height);
            stdout().flush().ok();
        }
    }

    // NOTE: Callers are responsible for rendering the footer after this function
    // All call sites already do: update_ui_from_state() then footer.render()
}

/// Handle status/info commands
fn handle_status_command(
    command: &UserCommand,
    app: &App,
    _state: &AppState, // Reserved for future use
    footer: &mut StickyFooter,
) -> Result<()> {
    match command {
        UserCommand::Status => {
            print_output_line("=== Server Status ===", footer)?;
            if app.servers.is_empty() {
                print_output_line("No servers running", footer)?;
            } else {
                for server in &app.servers {
                    print_output_line(
                        &format!(
                            "Server {}: {} on port {} - {}",
                            server.id, server.protocol, server.port, server.status
                        ),
                        footer,
                    )?;
                }
            }
        }
        UserCommand::ShowModel => {
            print_output_line(&format!("Current model: {}", app.connection_info.model), footer)?;
        }
        UserCommand::ShowLogLevel => {
            print_output_line(
                &format!("Current log level: {}", app.log_level.as_str()),
                footer,
            )?;
        }
        _ => {}
    }
    Ok(())
}
