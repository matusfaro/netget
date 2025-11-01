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
use tracing::{debug, error, info};

use crate::events::{EventHandler, UserCommand};
use crate::llm::OllamaClient;
use crate::settings::Settings;
use crate::state::app_state::AppState;
use crate::ui::{app::LogLevel, App};

use super::input_state::InputState;
use super::sticky_footer::{ConnectionInfo, FooterContent, StickyFooter};

/// Format scripting mode for display in status bar
/// Returns "LLM", "Python", or "JavaScript" based on selected mode
fn format_scripting_mode(mode: crate::state::app_state::ScriptingMode) -> String {
    mode.as_str().to_string()
}

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

    // Wrap settings in Arc<Mutex> for sharing with event handlers
    let settings = Arc::new(Mutex::new(settings));

    // Override model if specified in args, otherwise use settings
    let effective_model = if let Some(model) = &args.model {
        model.clone()
    } else {
        settings.lock().await.model.clone()
    };

    state.set_ollama_model(effective_model.clone()).await;
    app.connection_info.model = effective_model;

    // Load web search setting from settings file
    let web_search_mode = settings.lock().await.get_web_search_mode();
    state.set_web_search_mode(web_search_mode).await;

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

    // Create web approval channel for ASK mode
    let (web_approval_tx, mut web_approval_rx) = tokio::sync::mpsc::unbounded_channel();
    state.set_web_approval_channel(web_approval_tx).await;

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

    let scripting_mode = state.get_selected_scripting_mode().await;
    let scripting_status = format_scripting_mode(scripting_mode);
    let web_search_mode = state.get_web_search_mode().await;

    footer.set_connection_info(ConnectionInfo {
        model: app.connection_info.model.clone(),
        scripting_env: scripting_status,
        web_search_mode,
    });
    footer.set_packet_stats(app.packet_stats.clone());
    footer.set_log_level(app.log_level);

    // Print welcome messages to scrolling region
    print_welcome_messages(&mut footer)?;

    // Render footer initially to position cursor correctly
    // Without this, the cursor sits at the terminal default position until first keystroke
    footer.render(&mut stdout())?;

    // Create status channel for server messages
    let (status_tx, mut status_rx) = mpsc::unbounded_channel::<String>();

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

    loop {
        // Drain status messages from spawned tasks
        let mut ui_needs_update = false;
        while let Ok(msg) = status_rx.try_recv() {
            if msg == "__UPDATE_UI__" {
                // Special signal to update UI from state
                ui_needs_update = true;
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

        // Render footer immediately if messages were printed to reposition cursor
        // This ensures cursor is in the input field before select! blocks
        if ui_needs_update {
            update_ui_from_state(&mut app, &state, &mut footer).await;
            footer.render(&mut stdout())?;
            ui_needs_update = false; // Reset flag since we just rendered
        }

        tokio::select! {
            // Keyboard events
            maybe_event = event_stream.next() => {
                match maybe_event {
                    Some(Ok(event)) => {
                        if handle_event(event, &mut app, &state, &mut event_handler, &status_tx, &mut footer, settings.clone()).await? {
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

            // Web search approval requests
            Some(request) = web_approval_rx.recv() => {
                debug!("Received web approval request for: {}", request.url);

                // Store approval request in footer
                footer.pending_approval = Some(crate::cli::sticky_footer::PendingApproval {
                    url: request.url,
                    response_tx: request.response_tx,
                });

                // Re-render footer to show approval prompt
                footer.render(&mut stdout())?;
                ui_needs_update = false;
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
        "NetGet - LLM-Controlled Server",
        "",
        "Supported protocol stacks:",
        "  TCP (Beta): \"Pretend to be FTP server on port 2121; serve file accounts.csv with 'balance,0'\"",
        "  HTTP (Beta): \"Pretend to be a sassy HTTP server on port 8080 serving cooking recipes\"",
        "  SSH/SFTP (Beta): \"Pretent to be a shell via SSH on port 2222\"",
        "  DNS (Beta): \"DNS server on port 5252 and resolve everything to 1.2.3.4\"",
        "  DoT (Beta): \"Start a DNS-over-TLS server on port 853\"",
        "  DoH (Beta): \"Start a DNS-over-HTTPS server on port 443\"",
        "  NTP (Beta; root-only): \"pretend to be a ntp server on port 123\"",
        "  SNMP (Alpha): \"SNMP Port 8161 serve OID 1.3.6.1.2.1.1.1.0 (sysDescr) return 'NetGet SNMP Server v0.1'\"",
        "  IRC (Alpha): \"Start an IRC server\"",
        "  Telnet (Alpha): \"Start a telnet server on port 23 that echoes commands\"",
        "  SMTP (Alpha): \"Start an SMTP mail server on port 25\"",
        "  mDNS (Alpha): \"Advertise a web service via mDNS on port 8080\"",
        "  Ethernet (Alpha; root-only)",
        "  UDP (Alpha)",
        "  DHCP (Alpha)",
        "  NFS (Alpha)",
        "  SMB (Alpha): \"Start an SMB/CIFS file server on port 8445\"",
        "  LDAP (Alpha): \"Start an LDAP directory server on port 389\"",
        "  IMAP (Alpha): \"Start an IMAP mail server on port 143\" (or port 993 for IMAPS/TLS)",
        "  MySQL (Alpha)",
        "  WebDAV (Alpha)",
        "  IPP (Alpha)",
        "  PostgreSQL (Alpha)",
        "  Redis (Alpha)",
        "  Cassandra (Alpha): \"Start a Cassandra/CQL database server on port 9042\"",
        "  Elasticsearch (Alpha): \"Start an Elasticsearch server on port 9200\"",
        "  DynamoDB (Alpha): \"Start a DynamoDB-compatible server on port 8000\"",
        "  OpenAI API (Alpha): \"Start an OpenAI-compatible API server on port 11435\"",
        "  JSON-RPC (Alpha): \"Start a JSON-RPC 2.0 server on port 8000\"",
        "  HTTP Proxy (Alpha)",
        "  SOCKS5 (Alpha): \"Start a SOCKS5 proxy on port 1080 that asks before connecting\"",
        "  WireGuard VPN: \"Start a WireGuard VPN server on port 51820\"",
        "  OpenVPN (Alpha): \"Start an OpenVPN honeypot on port 1194\"",
        "  IPSec/IKEv2 (Alpha): \"Start an IPSec VPN honeypot on port 500\"",
        "  BGP (Alpha): \"Start a BGP routing server on port 179\"",
        "  MCP (Alpha): \"Start an MCP (Model Context Protocol) server on port 8000\"",
        "  gRPC (Alpha): \"Start a gRPC server on port 50051 with this schema: service UserService { rpc GetUser(UserId) returns (User); }\"",
        "  XML-RPC (Beta): \"Start an XML-RPC server on port 8080 with methods add(a,b) and greet(name)\"",
        "  Tor Directory (Alpha): \"Start a Tor directory mirror on port 9030 serving consensus and relay descriptors\"",
        "  Tor Relay (Beta): \"Start a Tor relay (OR protocol) on port 9001 with full circuit and crypto support\"",
        "  VNC (Alpha): \"Start a VNC server on port 5900 showing an ASCII art login screen\"",
        "  OpenAPI (Alpha): \"Start an OpenAPI server for a TODO API on port 8080\"",
        "  STUN (Alpha): \"Start a STUN server for NAT traversal on port 3478\"",
        "  TURN (Alpha): \"Start a TURN relay server on port 3478 with 10 minute allocations\"",
        "",
        "Features:",
        "  Scripting: LLM may produce on-the-fly code to reduce invoking LLM",
        "  Web Search: LLM may perform web searches (e.g. fetch protocol RFC)",
        "  Read file: Ask LLM to read a local file (e.g. load SQL schema, prompt)",
        "  Logging: LLM can log data into a file (e.g. web server access logs)",
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
    settings: Arc<Mutex<Settings>>,
) -> Result<bool> {
    match event {
        Event::Key(key) => {
            handle_key_event(key.code, key.modifiers, app, state, event_handler, status_tx, footer, settings).await
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
    settings: Arc<Mutex<Settings>>,
) -> Result<bool> {
    // Handle web approval prompt first (if active)
    if let Some(approval) = footer.pending_approval.take() {
        use crate::state::app_state::{WebApprovalResponse, WebSearchMode};

        match (key_code, modifiers) {
            (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
                // Ctrl-C during approval - deny and quit
                debug!("User pressed Ctrl-C during approval - denying and quitting");
                let _ = approval.response_tx.send(WebApprovalResponse::Deny);
                return Ok(true); // Signal quit
            }
            (KeyCode::Char('y'), _) | (KeyCode::Char('Y'), _) => {
                debug!("User approved web search");
                let _ = approval.response_tx.send(WebApprovalResponse::Allow);
                footer.render(&mut stdout())?;
                return Ok(false);
            }
            (KeyCode::Char('n'), _) | (KeyCode::Char('N'), _) => {
                debug!("User denied web search");
                let _ = approval.response_tx.send(WebApprovalResponse::Deny);
                footer.render(&mut stdout())?;
                return Ok(false);
            }
            (KeyCode::Char('a'), _) | (KeyCode::Char('A'), _) => {
                debug!("User chose always allow - switching to ON mode");

                // Switch mode to ON
                state.set_web_search_mode(WebSearchMode::On).await;

                // Save to settings
                if let Err(e) = settings.lock().await.set_web_search_mode(WebSearchMode::On) {
                    error!("Failed to save web search mode: {}", e);
                }

                // Send response
                let _ = approval.response_tx.send(WebApprovalResponse::AlwaysAllow);

                // Update UI
                update_ui_from_state(app, state, footer).await;
                footer.render(&mut stdout())?;
                return Ok(false);
            }
            _ => {
                // Any other key - restore the approval and ignore
                footer.pending_approval = Some(approval);
                return Ok(false);
            }
        }
    }

    // Handle special keys first
    match key_code {
        // Ctrl+C to quit
        KeyCode::Char('c') | KeyCode::Char('C') if modifiers.contains(KeyModifiers::CONTROL) => {
            return Ok(true);
        }

        // Ctrl+E to cycle scripting environment
        KeyCode::Char('e') | KeyCode::Char('E') if modifiers.contains(KeyModifiers::CONTROL) => {
            let (new_mode, switched) = state.cycle_scripting_mode().await;

            if switched {
                let message = match new_mode {
                    crate::state::app_state::ScriptingMode::Llm => {
                        "LLM will handle all requests directly"
                    }
                    crate::state::app_state::ScriptingMode::Python => {
                        "LLM will produce Python code to handle simple requests"
                    }
                    crate::state::app_state::ScriptingMode::JavaScript => {
                        "LLM will produce JavaScript code to handle simple requests"
                    }
                    crate::state::app_state::ScriptingMode::Go => {
                        "LLM will produce Go code to handle simple requests"
                    }
                    crate::state::app_state::ScriptingMode::Perl => {
                        "LLM will produce Perl code to handle simple requests"
                    }
                };
                print_output_line(message, footer)?;

                // Save the new scripting mode to settings
                let mode_str = new_mode.as_str().to_lowercase();
                if let Err(e) = settings.lock().await.set_scripting_mode(mode_str) {
                    error!("Failed to save scripting mode setting: {}", e);
                }

                update_ui_from_state(app, state, footer).await;
                footer.render(&mut stdout())?;
            } else {
                print_output_line("No other scripting environments available (only LLM)", footer)?;
            }

            return Ok(false);
        }

        // Ctrl+L to cycle log level
        KeyCode::Char('l') | KeyCode::Char('L') if modifiers.contains(KeyModifiers::CONTROL) => {
            let new_level = app.log_level.cycle();
            app.set_log_level(new_level);
            footer.set_log_level(new_level);
            print_output_line(&format!("Log level set to: {}", new_level.as_str()), footer)?;
            footer.render(&mut stdout())?;
            return Ok(false);
        }

        // Ctrl+W to cycle web search mode (ON -> ASK -> OFF -> ON)
        KeyCode::Char('w') | KeyCode::Char('W') if modifiers.contains(KeyModifiers::CONTROL) => {
            let new_mode = state.cycle_web_search_mode().await;
            let message = match new_mode {
                crate::state::app_state::WebSearchMode::On => "Web search: ON - LLM may perform web searches",
                crate::state::app_state::WebSearchMode::Ask => "Web search: ASK - LLM will request approval before searching",
                crate::state::app_state::WebSearchMode::Off => "Web search: OFF - LLM cannot perform web searches",
            };
            print_output_line(message, footer)?;

            // Save the new web search mode to settings
            if let Err(e) = settings.lock().await.set_web_search_mode(new_mode) {
                error!("Failed to save web search setting: {}", e);
            }

            update_ui_from_state(app, state, footer).await;
            footer.render(&mut stdout())?;
            return Ok(false);
        }

        // Ctrl+N or Alt+N to insert newline
        KeyCode::Char('n') | KeyCode::Char('N') if modifiers.contains(KeyModifiers::CONTROL) || modifiers.contains(KeyModifiers::ALT) => {
            footer.input_mut().insert_newline();
            update_slash_suggestions_and_render(app, footer, &mut stdout())?;
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

                // CRITICAL: Clear input and slash suggestions BEFORE executing command
                // This ensures the footer shrinks and scroll region is correct before we print output
                footer.input_mut().clear();
                app.update_slash_suggestions(&footer.input().text());

                // Update footer content (switch back to Normal mode since input is cleared)
                if app.slash_suggestions.is_empty() {
                    footer.set_content(FooterContent::Normal {
                        servers: app.servers.clone(),
                        connections: app.connections.clone(),
                        expand_all: app.expand_all_connections,
                    });
                }

                // Render footer now so scroll region is updated before command execution
                footer.render(&mut stdout())?;

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
                    UserCommand::Status | UserCommand::ShowModel | UserCommand::ShowLogLevel | UserCommand::ShowScriptingEnv | UserCommand::ShowWebSearch => {
                        // Handle status/info commands
                        handle_status_command(&command, app, state, event_handler, footer).await?;
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
                    UserCommand::TestAsk => {
                        // Test web search approval by triggering a search
                        use crate::llm::actions::tools::{execute_tool, ToolAction};

                        print_output_line("[INFO] Testing web search approval with DuckDuckGo...", footer)?;

                        // Get web search mode and approval channel
                        let web_search_mode = state.get_web_search_mode().await;
                        let approval_tx = state.get_web_approval_channel().await;

                        // Create a web search action for DuckDuckGo with a long path to test truncation
                        let action = ToolAction::WebSearch {
                            query: "https://duckduckgo.com/?q=test+search+query+with+very+long+parameters&ia=web&category=general&filters=none".to_string(),
                        };

                        // Execute the tool asynchronously (this will trigger approval prompt if in ASK mode)
                        let status_tx_clone = status_tx.clone();
                        tokio::spawn(async move {
                            let result = execute_tool(&action, approval_tx.as_ref(), web_search_mode).await;

                            // Send result to status channel
                            if result.success {
                                let _ = status_tx_clone.send("[INFO] Web search completed successfully".to_string());
                                // Truncate result if too long
                                let result_preview = if result.result.len() > 500 {
                                    format!("{}... (truncated)", &result.result[..500])
                                } else {
                                    result.result.clone()
                                };
                                let _ = status_tx_clone.send(format!("[DEBUG] Result preview: {}", result_preview));
                            } else {
                                let _ = status_tx_clone.send(format!("[ERROR] Web search failed: {}", result.result));
                            }
                        });
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
                    UserCommand::ShowDocs { protocol } => {
                        use crate::docs;

                        if let Some(protocol_name) = protocol {
                            // Show detailed docs for specific protocol
                            match docs::show_protocol_docs(&protocol_name) {
                                Ok(docs_text) => {
                                    for line in docs_text.lines() {
                                        print_output_line(line, footer)?;
                                    }
                                }
                                Err(err_msg) => {
                                    print_output_line(&err_msg, footer)?;
                                }
                            }
                        } else {
                            // List all protocols
                            let docs_text = docs::list_all_protocols();
                            for line in docs_text.lines() {
                                print_output_line(line, footer)?;
                            }
                        }

                        footer.render(&mut stdout())?;
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
                    UserCommand::ChangeScriptingEnv { env } => {
                        // Parse the scripting environment
                        let mode = match env.to_lowercase().as_str() {
                            "llm" => Some(crate::state::app_state::ScriptingMode::Llm),
                            "python" | "py" => Some(crate::state::app_state::ScriptingMode::Python),
                            "javascript" | "js" | "node" => Some(crate::state::app_state::ScriptingMode::JavaScript),
                            "go" | "golang" => Some(crate::state::app_state::ScriptingMode::Go),
                            "perl" => Some(crate::state::app_state::ScriptingMode::Perl),
                            _ => None,
                        };

                        if let Some(new_mode) = mode {
                            // Check if the environment is available
                            let scripting_env = state.get_scripting_env().await;
                            let available = match new_mode {
                                crate::state::app_state::ScriptingMode::Llm => true,
                                crate::state::app_state::ScriptingMode::Python => scripting_env.python.is_some(),
                                crate::state::app_state::ScriptingMode::JavaScript => scripting_env.javascript.is_some(),
                                crate::state::app_state::ScriptingMode::Go => scripting_env.go.is_some(),
                                crate::state::app_state::ScriptingMode::Perl => scripting_env.perl.is_some(),
                            };

                            if available {
                                state.set_selected_scripting_mode(new_mode).await;
                                let message = match new_mode {
                                    crate::state::app_state::ScriptingMode::Llm => {
                                        "Scripting environment set to: LLM (LLM will handle all requests directly)"
                                    }
                                    crate::state::app_state::ScriptingMode::Python => {
                                        "Scripting environment set to: Python (LLM will produce Python code)"
                                    }
                                    crate::state::app_state::ScriptingMode::JavaScript => {
                                        "Scripting environment set to: JavaScript (LLM will produce JavaScript code)"
                                    }
                                    crate::state::app_state::ScriptingMode::Go => {
                                        "Scripting environment set to: Go (LLM will produce Go code)"
                                    }
                                    crate::state::app_state::ScriptingMode::Perl => {
                                        "Scripting environment set to: Perl (LLM will produce Perl code)"
                                    }
                                };
                                print_output_line(message, footer)?;

                                // Save to settings
                                let mode_str = new_mode.as_str().to_lowercase();
                                if let Err(e) = settings.lock().await.set_scripting_mode(mode_str) {
                                    error!("Failed to save scripting mode setting: {}", e);
                                }

                                update_ui_from_state(app, state, footer).await;
                                footer.render(&mut stdout())?;
                            } else {
                                print_output_line(&format!("{} environment is not available on this system", new_mode), footer)?;
                            }
                        } else {
                            print_output_line(&format!("Unknown scripting environment: {}. Valid options: llm, python, javascript, go, perl", env), footer)?;
                        }
                    }
                    UserCommand::SetWebSearch { mode } => {
                        state.set_web_search_mode(mode).await;
                        let message = match mode {
                            crate::state::app_state::WebSearchMode::On => "Web search: ON",
                            crate::state::app_state::WebSearchMode::Ask => "Web search: ASK - will request approval",
                            crate::state::app_state::WebSearchMode::Off => "Web search: OFF",
                        };
                        print_output_line(message, footer)?;

                        // Save the new web search mode to settings
                        if let Err(e) = settings.lock().await.set_web_search_mode(mode) {
                            error!("Failed to save web search setting: {}", e);
                        }

                        update_ui_from_state(app, state, footer).await;
                        footer.render(&mut stdout())?;
                    }
                }
            }

            // Re-render footer after command execution (content may have changed)
            footer.render(&mut stdout())?;
            return Ok(false);
        }

        // Up arrow - command history navigation
        KeyCode::Up if footer.input().is_on_first_line() => {
            navigate_history_previous(app, footer);
            update_slash_suggestions_and_render(app, footer, &mut stdout())?;
            return Ok(false);
        }

        // Down arrow - command history navigation
        KeyCode::Down if footer.input().is_on_last_line() => {
            navigate_history_next(app, footer);
            update_slash_suggestions_and_render(app, footer, &mut stdout())?;
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
            update_slash_suggestions_and_render(app, footer, &mut stdout())?;
            return Ok(false);
        }

        // Ctrl+U - delete entire line
        KeyCode::Char('u') | KeyCode::Char('U') if modifiers.contains(KeyModifiers::CONTROL) => {
            footer.input_mut().delete_line();
            update_slash_suggestions_and_render(app, footer, &mut stdout())?;
            return Ok(false);
        }

        // Ctrl+W - delete word
        KeyCode::Char('w') | KeyCode::Char('W') if modifiers.contains(KeyModifiers::CONTROL) => {
            footer.input_mut().delete_word();
            update_slash_suggestions_and_render(app, footer, &mut stdout())?;
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
        update_slash_suggestions_and_render(app, footer, &mut stdout())?;
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

/// Update slash suggestions and render footer intelligently
/// Only re-renders full footer if suggestions actually changed
fn update_slash_suggestions_and_render(
    app: &mut App,
    footer: &mut StickyFooter,
    stdout: &mut impl Write,
) -> Result<()> {
    // Store old suggestions before updating
    let old_suggestions = app.slash_suggestions.clone();

    // Update suggestions based on current input
    app.update_slash_suggestions(&footer.input().text());

    // Check if suggestions actually changed
    if old_suggestions != app.slash_suggestions {
        // Update footer content based on new suggestions
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
        // Re-render entire footer (content changed)
        footer.render(stdout)?;
    } else {
        // Only re-render input line (suggestions unchanged)
        footer.render_input_only(stdout)?;
    }

    Ok(())
}

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
            protocol: s.protocol_name.clone(),
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
        app.connection_info.protocol = first_server.protocol_name.clone();
        if let Some(addr) = first_server.local_addr {
            app.connection_info.local_addr = Some(addr.to_string());
        }
    }

    let scripting_mode = state.get_selected_scripting_mode().await;
    let scripting_status = format_scripting_mode(scripting_mode);
    let web_search_mode = state.get_web_search_mode().await;

    footer.set_connection_info(ConnectionInfo {
        model: app.connection_info.model.clone(),
        scripting_env: scripting_status,
        web_search_mode,
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
async fn handle_status_command(
    command: &UserCommand,
    app: &App,
    state: &AppState,
    event_handler: &mut EventHandler,
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
            let current_model = state.get_ollama_model().await;
            print_output_line(&format!("Current model: {}", current_model), footer)?;
            print_output_line("", footer)?;
            print_output_line("Fetching available models...", footer)?;

            // Fetch model list from Ollama via event handler's LLM client
            match event_handler.list_models().await {
                Ok(models) => {
                    if models.is_empty() {
                        print_output_line("No models found. Please pull a model first.", footer)?;
                        print_output_line("Example: ollama pull llama3.2", footer)?;
                    } else {
                        print_output_line(&format!("Available models ({}):", models.len()), footer)?;
                        for model in &models {
                            if model == &current_model {
                                print_output_line(&format!("  * {} (current)", model), footer)?;
                            } else {
                                print_output_line(&format!("    {}", model), footer)?;
                            }
                        }
                        print_output_line("", footer)?;
                        print_output_line("To change model, use: /model <name>", footer)?;
                    }
                }
                Err(e) => {
                    print_output_line(&format!("Failed to fetch models: {}", e), footer)?;
                    print_output_line("Make sure Ollama is running.", footer)?;
                }
            }
        }
        UserCommand::ShowLogLevel => {
            print_output_line(
                &format!("Current log level: {}", app.log_level.as_str()),
                footer,
            )?;
        }
        UserCommand::ShowWebSearch => {
            let mode = state.get_web_search_mode().await;
            let status = match mode {
                crate::state::app_state::WebSearchMode::On => "ON (always allowed)",
                crate::state::app_state::WebSearchMode::Ask => "ASK (requires approval)",
                crate::state::app_state::WebSearchMode::Off => "OFF (disabled)",
            };
            print_output_line(&format!("Web search mode: {}", status), footer)?;
            print_output_line("", footer)?;
            print_output_line("To change, use: /web on, /web ask, or /web off", footer)?;
            print_output_line("Or press Ctrl+W to cycle through modes", footer)?;
        }
        _ => {}
    }
    Ok(())
}
