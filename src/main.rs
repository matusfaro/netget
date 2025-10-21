//! NetGet - LLM-Controlled Network Application
//!
//! Main entry point and event loop

use anyhow::Result;
use clap::Parser;
use crossterm::event::{Event, EventStream, KeyCode, KeyModifiers};
use crossterm::terminal::{enable_raw_mode, disable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use futures::{StreamExt, select, FutureExt};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::fs::OpenOptions;
use std::io;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{debug, info, error, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use netget::events::{AppEvent, EventHandler, NetworkEvent, UserCommand};
use netget::llm::{OllamaClient, PromptBuilder};
use netget::network::{ConnectionId, TcpServer};
use netget::state::app_state::AppState;
use netget::ui::{App, layout};

/// NetGet - LLM-Controlled Network Application
#[derive(Parser, Debug)]
#[command(name = "netget")]
#[command(about = "LLM-controlled network protocol server", long_about = None)]
struct Args {
    /// Command to execute immediately (e.g., "listen on port 21 via ftp")
    #[arg(value_name = "COMMAND")]
    command: Option<String>,

    /// Enable debug logging to netget.log
    #[arg(short, long)]
    debug: bool,
}

/// Format bytes for display - as text if printable, otherwise as hex
/// Truncates long messages
fn format_data(data: &[u8], max_len: usize) -> String {
    // Check if data is printable ASCII/UTF-8
    let is_text = data.iter().all(|&b| {
        b == b'\n' || b == b'\r' || b == b'\t' || (b >= 32 && b < 127)
    });

    if is_text {
        // Try to display as UTF-8 text
        match std::str::from_utf8(data) {
            Ok(text) => {
                let display_text = text.replace('\r', "\\r").replace('\n', "\\n").replace('\t', "\\t");
                if display_text.len() > max_len {
                    format!("{}... ({} bytes)", &display_text[..max_len], data.len())
                } else {
                    format!("{} ({} bytes)", display_text, data.len())
                }
            }
            Err(_) => format_as_hex(data, max_len),
        }
    } else {
        format_as_hex(data, max_len)
    }
}

/// Format bytes as hexadecimal
fn format_as_hex(data: &[u8], max_len: usize) -> String {
    let hex_chars = max_len / 3; // Each byte is "XX " (3 chars)
    let bytes_to_show = hex_chars.min(data.len());

    let hex: String = data.iter()
        .take(bytes_to_show)
        .map(|b| format!("{:02x} ", b))
        .collect();

    if data.len() > bytes_to_show {
        format!("{}... ({} bytes, hex)", hex.trim(), data.len())
    } else {
        format!("{} ({} bytes, hex)", hex.trim(), data.len())
    }
}

/// Process LLM response to handle common issues
/// - Strips b"..." wrapping if present
/// - Unescapes common escape sequences if needed
fn process_llm_response(response: &str) -> String {
    let trimmed = response.trim();

    // Check if wrapped in b"..." or just "..."
    let unwrapped = if trimmed.starts_with("b\"") && trimmed.ends_with('"') {
        warn!("LLM returned debug format b\"...\", unwrapping");
        &trimmed[2..trimmed.len()-1]
    } else if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() > 1 {
        &trimmed[1..trimmed.len()-1]
    } else {
        trimmed
    };

    // Unescape common sequences if they appear to be escaped
    // Only do this if we see literal \n or \r (not actual newlines)
    if unwrapped.contains("\\n") || unwrapped.contains("\\r") || unwrapped.contains("\\t") {
        warn!("LLM returned escaped sequences, unescaping");
        unwrapped
            .replace("\\r\\n", "\r\n")
            .replace("\\n", "\n")
            .replace("\\r", "\r")
            .replace("\\t", "\t")
            .replace("\\\\", "\\")
    } else {
        unwrapped.to_string()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let args = Args::parse();

    // Initialize logging only if --debug flag is set
    if args.debug {
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open("netget.log")?;

        tracing_subscriber::registry()
            .with(fmt::layer().with_writer(log_file).with_ansi(false))
            .with(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
            .init();

        info!("NetGet starting with debug logging enabled...");
    } else {
        // Initialize a no-op subscriber to avoid panics when logging
        // Filter set to ERROR level by default (effectively disabling most logs)
        tracing_subscriber::registry()
            .with(EnvFilter::new("error"))
            .init();
    }

    // Run the application
    let result = run_app(args.command).await;

    // Clean up
    if let Err(e) = result {
        if args.debug {
            error!("Application error: {}", e);
        }
        std::process::exit(1);
    }

    Ok(())
}

async fn run_app(initial_command: Option<String>) -> Result<()> {
    // Create application state
    let state = AppState::new();

    // Create Ollama clients (one for event handler, one for main loop)
    let llm_for_handler = OllamaClient::default();
    let llm = OllamaClient::default();

    // Create app state and load history
    let mut app = App::new();

    // Set initial model display
    app.connection_info.model = state.get_ollama_model().await;

    // Add welcome messages to output
    app.add_message("NetGet - LLM-Controlled Network Application".to_string());
    app.add_message("All protocol responses are generated by LLM".to_string());
    app.add_message("".to_string());

    if !app.command_history.is_empty() {
        app.add_message(format!("Loaded {} commands from history", app.command_history.len()));
        app.add_message("".to_string());
    }

    app.add_message("Example commands:".to_string());
    app.add_message("  listen on port 21 via ftp".to_string());
    app.add_message("  listen on port 80 via http".to_string());
    app.add_message("  model deepseek-coder:latest".to_string());
    app.add_message("".to_string());
    app.add_message("Keybindings:".to_string());
    app.add_message("  Ctrl+C - Quit".to_string());
    app.add_message("  Tab - Switch focus between Input/Output panels".to_string());
    app.add_message("  When Input focused:".to_string());
    app.add_message("    Up/Down arrows - Command history".to_string());
    app.add_message("    Enter - Submit command".to_string());
    app.add_message("  When Output focused:".to_string());
    app.add_message("    Up/Down or j/k - Scroll output (1 line)".to_string());
    app.add_message("    PageUp/PageDown - Scroll output (10 lines)".to_string());
    app.add_message("    Home/End - Jump to top/bottom".to_string());
    app.add_message("".to_string());

    // Create event channels
    let (network_tx, network_rx) = mpsc::unbounded_channel::<NetworkEvent>();

    // Shared connection storage (write halves only, read halves are in separate tasks)
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    type WriteHalfMap = Arc<Mutex<HashMap<ConnectionId, Arc<Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>>>>;
    let connections: WriteHalfMap = Arc::new(Mutex::new(HashMap::new()));

    // Create event handler
    let mut event_handler = EventHandler::new(state.clone(), llm_for_handler);

    // Process initial command if provided
    if let Some(cmd) = initial_command {
        app.add_status_message(format!("> {}", cmd));

        // Parse and execute the command
        let command = UserCommand::parse(&cmd);

        match command.clone() {
            UserCommand::Listen { port, protocol: _ } => {
                // Handle listen command
                if let Err(e) = event_handler.handle_event(
                    AppEvent::UserCommand(command),
                    &mut app
                ).await {
                    app.add_llm_message(format!("Error: {}", e));
                } else {
                    // Create a new TCP server for this listen command
                    let mut tcp_server = TcpServer::new(network_tx.clone());

                    // Start listening
                    let addr: std::net::SocketAddr = format!("0.0.0.0:{}", port).parse()?;
                    if let Err(e) = tcp_server.listen(addr).await {
                        app.add_llm_message(format!("Failed to listen: {}", e));
                    } else {
                        let local_addr = tcp_server.local_addr()
                            .map(|a| a.to_string())
                            .unwrap_or_default();

                        app.connection_info.local_addr = Some(local_addr);
                        app.connection_info.state = "Listening".to_string();

                        // Spawn task to accept connections
                        let network_tx_clone = network_tx.clone();
                        let connections_clone = connections.clone();

                        tokio::spawn(async move {
                            loop {
                                match tcp_server.accept().await {
                                    Ok(Some((stream, remote_addr))) => {
                                        let connection_id = ConnectionId::new();
                                        info!("Accepted connection {} from {}", connection_id, remote_addr);

                                        // Split stream into read and write halves to avoid deadlock
                                        let (read_half, write_half) = tokio::io::split(stream);

                                        // Store write half in shared HashMap
                                        let write_half_arc = Arc::new(Mutex::new(write_half));
                                        connections_clone.lock().await.insert(connection_id, write_half_arc);

                                        // Send connected event
                                        let _ = network_tx_clone.send(NetworkEvent::Connected {
                                            connection_id,
                                            remote_addr,
                                        });

                                        // Spawn read task for this connection
                                        let network_tx_inner = network_tx_clone.clone();
                                        tokio::spawn(async move {
                                            use tokio::io::AsyncReadExt;
                                            let mut buffer = vec![0u8; 8192];
                                            let mut read_half = read_half;

                                            loop {
                                                match read_half.read(&mut buffer).await {
                                                    Ok(0) => {
                                                        info!("Connection {} closed", connection_id);
                                                        let _ = network_tx_inner.send(NetworkEvent::Disconnected { connection_id });
                                                        break;
                                                    }
                                                    Ok(n) => {
                                                        use bytes::Bytes;
                                                        let data = Bytes::copy_from_slice(&buffer[..n]);
                                                        debug!("Received {} bytes from {}", n, connection_id);
                                                        let _ = network_tx_inner.send(NetworkEvent::DataReceived {
                                                            connection_id,
                                                            data,
                                                        });
                                                    }
                                                    Err(e) => {
                                                        error!("Read error on {}: {}", connection_id, e);
                                                        let _ = network_tx_inner.send(NetworkEvent::Error {
                                                            connection_id: Some(connection_id),
                                                            error: e.to_string(),
                                                        });
                                                        break;
                                                    }
                                                }
                                            }
                                        });
                                    }
                                    Ok(None) => break,
                                    Err(e) => {
                                        error!("Accept error: {}", e);
                                        break;
                                    }
                                }
                            }
                        });
                    }
                }
            }
            _ => {
                // Handle other commands
                if let Err(e) = event_handler.handle_event(
                    AppEvent::UserCommand(command),
                    &mut app
                ).await {
                    app.add_llm_message(format!("Error: {}", e));
                }
            }
        }
    }

    // Wrap app in Arc<Mutex> for sharing between tasks
    let app_clone = Arc::new(Mutex::new(app));

    // Spawn background task to handle network events immediately (with LLM processing)
    let network_rx_bg = network_rx;
    let app_for_task = app_clone.clone();
    let connections_for_task = connections.clone();
    let state_for_task = state.clone();
    let llm_for_task = llm.clone();

    tokio::spawn(async move {
        let mut network_rx = network_rx_bg;
        loop {
            if let Some(net_event) = network_rx.recv().await {
                match &net_event {
                    NetworkEvent::Listening { addr } => {
                        let mut app = app_for_task.lock().await;
                        app.add_message(format!("✓ Listening on {}", addr));
                        app.connection_info.local_addr = Some(addr.to_string());
                        app.connection_info.state = "Listening".to_string();
                        app.connection_info.model = state_for_task.get_ollama_model().await;
                    }
                    NetworkEvent::Connected { connection_id, remote_addr } => {
                        {
                            let mut app = app_for_task.lock().await;
                            app.add_message(format!("✓ Connection {} from {}", connection_id, remote_addr));
                            app.connection_info.remote_addr = Some(remote_addr.to_string());
                            app.connection_info.state = "Connected".to_string();
                        }

                        // Call LLM for initial greeting
                        let model = state_for_task.get_ollama_model().await;
                        let prompt = PromptBuilder::build_connection_established_prompt(&state_for_task, *connection_id).await;

                        match llm_for_task.generate(&model, &prompt).await {
                            Ok(response) => {
                                let response = process_llm_response(&response);
                                if !response.is_empty() && response != "NO_RESPONSE" {
                                    if let Some(write_half_arc) = connections_for_task.lock().await.get(connection_id) {
                                        use tokio::io::AsyncWriteExt;
                                        let mut write_half = write_half_arc.lock().await;
                                        if let Err(e) = write_half.write_all(response.as_bytes()).await {
                                            error!("Write error: {}", e);
                                        } else if let Err(e) = write_half.flush().await {
                                            error!("Flush error: {}", e);
                                        } else {
                                            let formatted = format_data(response.as_bytes(), 80);
                                            let mut app = app_for_task.lock().await;
                                            app.add_message(format!("→ Sent to {}: {}", connection_id, formatted));
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("LLM error: {}", e);
                                let mut app = app_for_task.lock().await;
                                app.add_message(format!("✗ LLM error: {}", e));
                            }
                        }
                    }
                    NetworkEvent::DataReceived { connection_id, data } => {
                        {
                            let formatted = format_data(data, 80);
                            let mut app = app_for_task.lock().await;
                            app.add_message(format!("← Recv from {}: {}", connection_id, formatted));
                        }

                        // Call LLM for response
                        let model = state_for_task.get_ollama_model().await;
                        let prompt = PromptBuilder::build_data_received_prompt(&state_for_task, *connection_id, data).await;

                        match llm_for_task.generate(&model, &prompt).await {
                            Ok(response) => {
                                let response = process_llm_response(&response);
                                if response == "CLOSE_CONNECTION" {
                                    if let Some(write_half_arc) = connections_for_task.lock().await.remove(connection_id) {
                                        drop(write_half_arc);
                                        let mut app = app_for_task.lock().await;
                                        app.add_message(format!("✗ Closed connection {}", connection_id));
                                    }
                                } else if !response.is_empty() && response != "NO_RESPONSE" {
                                    if let Some(write_half_arc) = connections_for_task.lock().await.get(connection_id) {
                                        use tokio::io::AsyncWriteExt;
                                        let mut write_half = write_half_arc.lock().await;
                                        if let Err(e) = write_half.write_all(response.as_bytes()).await {
                                            error!("Write error: {}", e);
                                        } else if let Err(e) = write_half.flush().await {
                                            error!("Flush error: {}", e);
                                        } else {
                                            let formatted = format_data(response.as_bytes(), 80);
                                            let mut app = app_for_task.lock().await;
                                            app.add_message(format!("→ Sent to {}: {}", connection_id, formatted));
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("LLM error: {}", e);
                                let mut app = app_for_task.lock().await;
                                app.add_message(format!("✗ LLM error: {}", e));
                            }
                        }
                    }
                    NetworkEvent::DataSent { connection_id, data } => {
                        let formatted = format_data(data, 80);
                        let mut app = app_for_task.lock().await;
                        app.add_message(format!("→ Sent to {}: {}", connection_id, formatted));
                    }
                    NetworkEvent::Disconnected { connection_id } => {
                        let mut app = app_for_task.lock().await;
                        app.add_message(format!("✗ Connection {} closed", connection_id));
                        app.connection_info.remote_addr = None;
                        app.connection_info.state = "Idle".to_string();
                    }
                    NetworkEvent::Error { connection_id, error } => {
                        let mut app = app_for_task.lock().await;
                        if let Some(id) = connection_id {
                            app.add_message(format!("✗ Error on {}: {}", id, error));
                        } else {
                            app.add_message(format!("✗ Error: {}", error));
                        }
                    }
                }
            }
        }
    });

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create async event stream
    let mut reader = EventStream::new();

    // Timer to trigger redraws
    let mut redraw_interval = interval(Duration::from_millis(100));

    // Main event loop with ratatui
    let result = 'main_loop: loop {
        // Draw the UI
        {
            let app = app_clone.lock().await;
            terminal.draw(|f| layout::render(f, &app))?;
        }

        select! {
            // Periodic redraw to pick up background task updates
            _ = redraw_interval.tick().fuse() => {
                // Just loop to trigger redraw above
            }

            // Handle keyboard events
            maybe_event = reader.next().fuse() => {
                match maybe_event {
                    Some(Ok(Event::Key(key))) => {
                        let mut app = app_clone.lock().await;

                        match key.code {
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                break 'main_loop Ok(());
                            }
                            KeyCode::Tab => {
                                app.toggle_focus();
                            }
                            // Input-only commands (only work when input is focused)
                            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) && app.is_input_focused() => {
                                app.move_cursor_start();
                            }
                            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) && app.is_input_focused() => {
                                app.move_cursor_end();
                            }
                            KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) && app.is_input_focused() => {
                                app.delete_to_end();
                            }
                            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) && app.is_input_focused() => {
                                app.delete_word();
                            }
                            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) && app.is_input_focused() => {
                                app.clear_input();
                            }
                            // Vim-style j/k navigation in output mode (must come before generic Char pattern)
                            KeyCode::Char('j') if app.is_output_focused() => {
                                app.scroll_down(1);
                            }
                            KeyCode::Char('k') if app.is_output_focused() => {
                                app.scroll_up(1);
                            }
                            // Generic character input (only when input is focused)
                            KeyCode::Char(c) => {
                                if app.is_input_focused() {
                                    app.enter_char(c);
                                }
                            }
                            KeyCode::Backspace if app.is_input_focused() => {
                                app.delete_char();
                            }
                            KeyCode::Left if app.is_input_focused() => {
                                app.move_cursor_left();
                            }
                            KeyCode::Right if app.is_input_focused() => {
                                app.move_cursor_right();
                            }
                            KeyCode::Up => {
                                if app.is_input_focused() {
                                    app.history_previous();
                                } else {
                                    app.scroll_up(1);
                                }
                            }
                            KeyCode::Down => {
                                if app.is_input_focused() {
                                    app.history_next();
                                } else {
                                    app.scroll_down(1);
                                }
                            }
                            KeyCode::PageUp => {
                                app.scroll_up(10);
                            }
                            KeyCode::PageDown => {
                                app.scroll_down(10);
                            }
                            KeyCode::Home => {
                                if app.is_input_focused() {
                                    app.move_cursor_start();
                                } else {
                                    app.scroll_to_top();
                                }
                            }
                            KeyCode::End => {
                                if app.is_input_focused() {
                                    app.move_cursor_end();
                                } else {
                                    app.scroll_to_bottom();
                                }
                            }
                            KeyCode::Enter if app.is_input_focused() => {
                                // Auto-scroll to bottom on new command
                                app.scroll_to_bottom();

                                let input = app.submit_input();

                                if !input.trim().is_empty() {
                                    app.add_message(format!("> {}", input));

                                    // Parse user command
                                    let command = UserCommand::parse(&input);

                                    // Special handling for listen command
                                    match command.clone() {
                                        UserCommand::Listen { port, protocol: _ } => {
                                            // Handle listen command through event handler
                                            if let Err(e) = event_handler.handle_event(
                                                AppEvent::UserCommand(command),
                                                &mut app
                                            ).await {
                                                app.add_message(format!("✗ Error: {}", e));
                                                drop(app); // Release lock before continuing
                                                continue;
                                            }

                                            app.add_message(format!("Starting server on port {}...", port));

                                            // Create a new TCP server for this listen command
                                            let mut tcp_server = TcpServer::new(network_tx.clone());

                                            // Start listening
                                            let addr: std::net::SocketAddr = format!("0.0.0.0:{}", port).parse().unwrap();
                                            if let Err(e) = tcp_server.listen(addr).await {
                                                app.add_message(format!("✗ Failed to listen: {}", e));
                                                drop(app); // Release lock before continuing
                                                continue;
                                            }

                                            let local_addr = tcp_server.local_addr()
                                                .map(|a| a.to_string())
                                                .unwrap_or_default();

                                            app.connection_info.local_addr = Some(local_addr);
                                            app.connection_info.state = "Listening".to_string();

                                            // Spawn task to accept connections
                                            let network_tx_clone = network_tx.clone();
                                            let connections_clone = connections.clone();

                                            drop(app); // Release lock before spawning

                                            tokio::spawn(async move {
                                                loop {
                                                    match tcp_server.accept().await {
                                                        Ok(Some((stream, remote_addr))) => {
                                                            let connection_id = ConnectionId::new();
                                                            info!("Accepted connection {} from {}", connection_id, remote_addr);

                                                            // Split stream into read and write halves to avoid deadlock
                                                            let (read_half, write_half) = tokio::io::split(stream);

                                                            // Store write half in shared HashMap
                                                            let write_half_arc = Arc::new(Mutex::new(write_half));
                                                            connections_clone.lock().await.insert(connection_id, write_half_arc);

                                                            // Send connected event
                                                            let _ = network_tx_clone.send(NetworkEvent::Connected {
                                                                connection_id,
                                                                remote_addr,
                                                            });

                                                            // Spawn read task for this connection
                                                            let network_tx_inner = network_tx_clone.clone();
                                                            tokio::spawn(async move {
                                                                use tokio::io::AsyncReadExt;
                                                                let mut buffer = vec![0u8; 8192];
                                                                let mut read_half = read_half;

                                                                loop {
                                                                    match read_half.read(&mut buffer).await {
                                                                        Ok(0) => {
                                                                            info!("Connection {} closed", connection_id);
                                                                            let _ = network_tx_inner.send(NetworkEvent::Disconnected { connection_id });
                                                                            break;
                                                                        }
                                                                        Ok(n) => {
                                                                            use bytes::Bytes;
                                                                            let data = Bytes::copy_from_slice(&buffer[..n]);
                                                                            debug!("Received {} bytes from {}", n, connection_id);
                                                                            let _ = network_tx_inner.send(NetworkEvent::DataReceived {
                                                                                connection_id,
                                                                                data,
                                                                            });
                                                                        }
                                                                        Err(e) => {
                                                                            error!("Read error on {}: {}", connection_id, e);
                                                                            let _ = network_tx_inner.send(NetworkEvent::Error {
                                                                                connection_id: Some(connection_id),
                                                                                error: e.to_string(),
                                                                            });
                                                                            break;
                                                                        }
                                                                    }
                                                                }
                                                            });
                                                        }
                                                        Ok(None) => break,
                                                        Err(e) => {
                                                            error!("Accept error: {}", e);
                                                            break;
                                                        }
                                                    }
                                                }
                                            });
                                        }
                                        _ => {
                                            // Handle other commands
                                            if let Ok(should_quit) = event_handler.handle_event(
                                                AppEvent::UserCommand(command),
                                                &mut app
                                            ).await {
                                                if should_quit {
                                                    drop(app); // Release lock
                                                    break 'main_loop Ok(());
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    Some(Err(e)) => {
                        let mut app = app_clone.lock().await;
                        app.add_message(format!("✗ Error: {}", e));
                    }
                    None => break 'main_loop Ok(()),
                    _ => {} // Ignore other events (mouse, focus, paste, resize)
                }
            }
        }
    };

    // Cleanup terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // Save history before exiting
    let app = app_clone.lock().await;
    if let Err(e) = app.save_history() {
        error!("Failed to save command history: {}", e);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_text_data() {
        let data = b"USER anonymous\r\n";
        let formatted = format_data(data, 100);
        println!("Formatted: {}", formatted);
        assert!(formatted.contains("USER anonymous\\r\\n"));
        assert!(formatted.contains("16 bytes")); // "USER anonymous\r\n" is 16 bytes
    }

    #[test]
    fn test_format_binary_data() {
        let data = b"\x00\x01\x02\xFF\xFE";
        let formatted = format_data(data, 100);
        assert!(formatted.contains("00 01 02 ff fe"));
        assert!(formatted.contains("5 bytes"));
        assert!(formatted.contains("hex"));
    }

    #[test]
    fn test_format_truncates_long_text() {
        let data = b"A very long message that should be truncated when displayed in the UI";
        let formatted = format_data(data, 30);
        assert!(formatted.contains("..."));
        assert!(formatted.contains("69 bytes"));
    }

    #[test]
    fn test_format_truncates_long_hex() {
        let data = [0xFF; 100]; // 100 bytes of 0xFF
        let formatted = format_data(&data, 30);
        assert!(formatted.contains("..."));
        assert!(formatted.contains("100 bytes"));
        assert!(formatted.contains("hex"));
    }

    #[test]
    fn test_format_empty_data() {
        let data = b"";
        let formatted = format_data(data, 100);
        assert!(formatted.contains("0 bytes"));
    }
}
