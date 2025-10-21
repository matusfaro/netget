//! NetGet - LLM-Controlled Network Application
//!
//! Main entry point and event loop

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::KeyCode,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::fs::OpenOptions;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, info, error, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use netget::events::{AppEvent, EventHandler, NetworkEvent, UserCommand};
use netget::llm::{LlmResponse, OllamaClient, PromptBuilder};
use netget::network::{ConnectionId, TcpServer};
use netget::state::app_state::AppState;
use netget::ui::{App, UiEvent};

/// Connection processing state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionStatus {
    /// Not processing, no queued data
    Idle,
    /// LLM is currently generating a response
    Processing,
    /// LLM requested to wait for more data before responding
    Accumulating,
}

/// Per-connection state for queueing and LLM processing
struct ConnectionState {
    /// Current processing status
    status: ConnectionStatus,
    /// Queue of data that arrived while LLM was processing
    queue: Vec<u8>,
}

impl ConnectionState {
    fn new() -> Self {
        Self {
            status: ConnectionStatus::Idle,
            queue: Vec::new(),
        }
    }
}

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

/// Process LLM response - parse JSON and handle legacy formats
fn process_llm_response(response: &str) -> LlmResponse {
    // Try to parse as structured response
    match LlmResponse::from_str(response) {
        Ok(mut parsed) => {
            // Unescape output if needed
            if let Some(output) = &parsed.output {
                if output.contains("\\n") || output.contains("\\r") || output.contains("\\t") {
                    warn!("LLM output contains escaped sequences, unescaping");
                    parsed.output = Some(
                        output
                            .replace("\\r\\n", "\r\n")
                            .replace("\\n", "\n")
                            .replace("\\r", "\r")
                            .replace("\\t", "\t")
                            .replace("\\\\", "\\")
                    );
                }
            }
            parsed
        }
        Err(e) => {
            error!("Failed to parse LLM response: {}", e);
            // Return default (no action)
            LlmResponse::default()
        }
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
    // Initialize terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create application state
    let state = AppState::new();

    // Create Ollama clients (one for event handler, one for main loop)
    let llm_for_handler = OllamaClient::default();
    let llm = OllamaClient::default();

    // Create UI app
    let mut app = App::new();

    // Show welcome message with history info
    app.add_message("NetGet - LLM-Controlled Network Application".to_string());
    app.add_message("All protocol responses are generated by LLM".to_string());
    if !app.command_history.is_empty() {
        app.add_message(format!("Loaded {} commands from history", app.command_history.len()));
    }
    app.add_message("".to_string());
    app.add_message("Example commands:".to_string());
    app.add_message("  listen on port 21 via ftp".to_string());
    app.add_message("  listen on port 80 via http".to_string());
    app.add_message("  listen on port 1234".to_string());
    app.add_message("  model deepseek-coder:latest".to_string());
    app.add_message("".to_string());
    app.add_message("Keybindings:".to_string());
    app.add_message("  Up/Down - Navigate command history".to_string());
    app.add_message("  PageUp/PageDown - Scroll output".to_string());
    app.add_message("  Ctrl+G - Jump to bottom".to_string());
    app.add_message("  Shift+Enter - Insert newline".to_string());
    app.add_message("  Ctrl+A/E - Start/End of line".to_string());
    app.add_message("  Ctrl+K/W/U - Delete to end/word/line".to_string());
    app.add_message("  Ctrl+C - Quit".to_string());
    app.add_message("".to_string());

    // Set initial model display
    app.connection_info.model = state.get_ollama_model().await;

    // Create event channels
    let (network_tx, mut network_rx) = mpsc::unbounded_channel::<NetworkEvent>();

    // Channel for status messages from spawned tasks back to UI
    let (status_tx, mut status_rx) = mpsc::unbounded_channel::<String>();

    // Shared connection storage (write halves only, read halves are in separate tasks)
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    type WriteHalfMap = Arc<Mutex<HashMap<ConnectionId, Arc<Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>>>>;
    let connections: WriteHalfMap = Arc::new(Mutex::new(HashMap::new()));

    // Per-connection state tracking for LLM processing and queueing
    type ConnectionStateMap = Arc<Mutex<HashMap<ConnectionId, ConnectionState>>>;
    let connection_states: ConnectionStateMap = Arc::new(Mutex::new(HashMap::new()));

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

    // Main event loop
    let result = 'main_loop: loop {
        // Render UI
        terminal.draw(|f| {
            let area = f.area();
            app.render(f, area);
        })?;

        // Poll for UI events (with timeout)
        if let Some(ui_event) = netget::ui::events::poll_event(Duration::from_millis(100))? {
            match ui_event {
                UiEvent::Key(key) => {
                    if key.code == KeyCode::Enter {
                        // Submit user input
                        let input = app.submit_input();
                        if !input.is_empty() {
                            app.add_status_message(format!("> {}", input));

                            // Parse user command
                            let command = UserCommand::parse(&input);

                            // Special handling for listen command
                            match command.clone() {
                                UserCommand::Listen { port, protocol: _ } => {
                                    // Handle listen command
                                    if let Err(e) = event_handler.handle_event(
                                        AppEvent::UserCommand(command),
                                        &mut app
                                    ).await {
                                        app.add_llm_message(format!("Error: {}", e));
                                        continue;
                                    }

                                    // Create a new TCP server for this listen command
                                    let mut tcp_server = TcpServer::new(network_tx.clone());

                                    // Start listening
                                    let addr: std::net::SocketAddr = format!("0.0.0.0:{}", port).parse()?;
                                    if let Err(e) = tcp_server.listen(addr).await {
                                        app.add_llm_message(format!("Failed to listen: {}", e));
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
                                    if event_handler.handle_event(
                                        AppEvent::UserCommand(command),
                                        &mut app
                                    ).await? {
                                        break 'main_loop Ok(());
                                    }
                                }
                            }
                        }
                    } else if netget::ui::events::handle_key_event(&mut app, key)? {
                        // Quit signal
                        break 'main_loop Ok(());
                    }
                }
                UiEvent::Resize(_, _) => {
                    // Terminal resized, will re-render on next iteration
                }
                UiEvent::Tick => {
                    // Periodic update
                }
            }
        }

        // Check for status messages from spawned tasks
        while let Ok(status_msg) = status_rx.try_recv() {
            app.add_status_message(status_msg);
        }

        // Check for network events and handle them with LLM
        while let Ok(net_event) = network_rx.try_recv() {
            match &net_event {
                NetworkEvent::Connected { connection_id, remote_addr } => {
                    app.add_status_message(format!("Connection {} from {}", connection_id, remote_addr));

                    // Initialize connection state
                    connection_states.lock().await.insert(*connection_id, ConnectionState::new());

                    // Call LLM for initial greeting
                    let model = state.get_ollama_model().await;
                    let prompt = PromptBuilder::build_connection_established_prompt(&state, *connection_id).await;

                    match llm.generate(&model, &prompt).await {
                        Ok(raw_response) => {
                            let response = process_llm_response(&raw_response);

                            // Log message if present
                            if let Some(msg) = &response.log_message {
                                info!("LLM: {}", msg);
                            }

                            // Send output if present
                            if let Some(output) = &response.output {
                                if let Some(write_half_arc) = connections.lock().await.get(connection_id) {
                                    use tokio::io::AsyncWriteExt;
                                    let mut write_half = write_half_arc.lock().await;
                                    if let Err(e) = write_half.write_all(output.as_bytes()).await {
                                        error!("Write error: {}", e);
                                    } else if let Err(e) = write_half.flush().await {
                                        error!("Flush error: {}", e);
                                    } else {
                                        let formatted = format_data(output.as_bytes(), 80);
                                        app.add_status_message(format!("→ Sent to {}: {}", connection_id, formatted));
                                    }
                                }
                            }

                            // Handle close connection
                            if response.close_connection {
                                if let Some(write_half_arc) = connections.lock().await.remove(connection_id) {
                                    drop(write_half_arc);
                                    app.add_status_message(format!("Closed connection {}", connection_id));
                                }
                                connection_states.lock().await.remove(connection_id);
                            }
                        }
                        Err(e) => {
                            error!("LLM error: {}", e);
                            app.add_llm_message(format!("LLM error: {}", e));
                        }
                    }
                }
                NetworkEvent::DataReceived { connection_id, data } => {
                    let formatted = format_data(&data, 80);
                    app.add_status_message(format!("← Recv from {}: {}", connection_id, formatted));

                    // Clone necessary data for the spawned task
                    let connection_id = *connection_id;
                    let data = data.clone();
                    let state_clone = state.clone();
                    let llm_clone = llm.clone();
                    let connections_clone = connections.clone();
                    let connection_states_clone = connection_states.clone();
                    let status_tx_clone = status_tx.clone();

                    // Spawn task to handle this data (enables concurrent processing per connection)
                    tokio::spawn(async move {
                        // Check connection processing state
                        let current_status = {
                            let mut states = connection_states_clone.lock().await;
                            let conn_state = states.entry(connection_id).or_insert_with(ConnectionState::new);
                            conn_state.status
                        };

                        match current_status {
                            ConnectionStatus::Processing => {
                                // LLM is already processing for this connection, queue the data
                                let mut states = connection_states_clone.lock().await;
                                let conn_state = states.get_mut(&connection_id).unwrap();
                                conn_state.queue.extend_from_slice(&data);
                                let msg = format!(
                                    "LLM busy, queued {} bytes for {} (queue: {} bytes)",
                                    data.len(),
                                    connection_id,
                                    conn_state.queue.len()
                                );
                                info!("{}", msg);
                                let _ = status_tx_clone.send(msg);
                            }
                            ConnectionStatus::Idle | ConnectionStatus::Accumulating => {
                                // Merge any queued data with new data
                                let mut data_to_process = {
                                    let mut states = connection_states_clone.lock().await;
                                    let conn_state = states.get_mut(&connection_id).unwrap();

                                    if conn_state.queue.is_empty() {
                                        data.to_vec()
                                    } else {
                                        // Append new data to queue and process all
                                        conn_state.queue.extend_from_slice(&data);
                                        let all_data = conn_state.queue.clone();
                                        conn_state.queue.clear();
                                        let msg = format!(
                                            "Processing accumulated {} bytes for {}",
                                            all_data.len(),
                                            connection_id
                                        );
                                        info!("{}", msg);
                                        let _ = status_tx_clone.send(msg);
                                        all_data
                                    }
                                };

                                // Process data with LLM in a loop (handling queued data)
                                loop {
                                    // Mark as processing
                                    {
                                        let mut states = connection_states_clone.lock().await;
                                        let conn_state = states.get_mut(&connection_id).unwrap();
                                        conn_state.status = ConnectionStatus::Processing;
                                    }

                                    // Call LLM
                                    let model = state_clone.get_ollama_model().await;
                                    let data_bytes = bytes::Bytes::copy_from_slice(&data_to_process);
                                    let prompt = PromptBuilder::build_data_received_prompt(&state_clone, connection_id, &data_bytes).await;

                                    let _ = status_tx_clone.send("Asking LLM for response...".to_string());

                                    match llm_clone.generate(&model, &prompt).await {
                                        Ok(raw_response) => {
                                            let response = process_llm_response(&raw_response);

                                            // Log message if present
                                            if let Some(msg) = &response.log_message {
                                                info!("LLM: {}", msg);
                                            }

                                            // Check for WAIT_FOR_MORE
                                            if response.wait_for_more {
                                                let mut states = connection_states_clone.lock().await;
                                                let conn_state = states.get_mut(&connection_id).unwrap();
                                                conn_state.status = ConnectionStatus::Accumulating;
                                                let msg = "LLM: Waiting for more data".to_string();
                                                info!("{}", msg);
                                                let _ = status_tx_clone.send(msg);
                                                break;
                                            }

                                            // Check for shutdown server
                                            if response.shutdown_server {
                                                info!("LLM requested server shutdown");
                                                let _ = status_tx_clone.send("LLM requested server shutdown".to_string());
                                                // TODO: Implement graceful shutdown
                                                // For now, just log it
                                                warn!("Server shutdown requested but not yet implemented");
                                            }

                                            // Send output if present
                                            if let Some(output) = &response.output {
                                                if let Some(write_half_arc) = connections_clone.lock().await.get(&connection_id) {
                                                    use tokio::io::AsyncWriteExt;
                                                    let mut write_half = write_half_arc.lock().await;
                                                    if let Err(e) = write_half.write_all(output.as_bytes()).await {
                                                        error!("Write error: {}", e);
                                                    } else if let Err(e) = write_half.flush().await {
                                                        error!("Flush error: {}", e);
                                                    } else {
                                                        let formatted = format_data(output.as_bytes(), 80);
                                                        let msg = format!("→ Sent to {}: {}", connection_id, formatted);
                                                        info!("{}", msg);
                                                        let _ = status_tx_clone.send(msg);
                                                    }
                                                }
                                            }

                                            // Check for CLOSE_CONNECTION
                                            if response.close_connection {
                                                if let Some(write_half_arc) = connections_clone.lock().await.remove(&connection_id) {
                                                    drop(write_half_arc);
                                                    let msg = format!("Closed connection {}", connection_id);
                                                    info!("{}", msg);
                                                    let _ = status_tx_clone.send(msg);
                                                }
                                                connection_states_clone.lock().await.remove(&connection_id);
                                                break;
                                            }

                                            // Check for queued data
                                            let queued_data = {
                                                let mut states = connection_states_clone.lock().await;
                                                let conn_state = states.get_mut(&connection_id).unwrap();

                                                if conn_state.queue.is_empty() {
                                                    conn_state.status = ConnectionStatus::Idle;
                                                    None
                                                } else {
                                                    let queued = conn_state.queue.clone();
                                                    conn_state.queue.clear();
                                                    let msg = format!(
                                                        "Processing {} bytes of queued data for {}",
                                                        queued.len(),
                                                        connection_id
                                                    );
                                                    info!("{}", msg);
                                                    let _ = status_tx_clone.send(msg);
                                                    Some(queued)
                                                }
                                            };

                                            if let Some(queued) = queued_data {
                                                data_to_process = queued;
                                                // Continue loop
                                            } else {
                                                break;
                                            }
                                        }
                                        Err(e) => {
                                            error!("LLM error: {}", e);

                                            // Go idle on error
                                            let mut states = connection_states_clone.lock().await;
                                            if let Some(conn_state) = states.get_mut(&connection_id) {
                                                conn_state.status = ConnectionStatus::Idle;
                                            }
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    });
                }
                _ => {
                    // Handle other events with EventHandler
                    if event_handler.handle_event(AppEvent::Network(net_event), &mut app).await? {
                        break 'main_loop Ok(());
                    }
                }
            }
        }

        // Update UI stats from state
        if let Some(conns) = state.get_all_connections().await.first() {
            app.packet_stats.packets_received = conns.packets_received;
            app.packet_stats.packets_sent = conns.packets_sent;
            app.packet_stats.bytes_received = conns.bytes_received;
            app.packet_stats.bytes_sent = conns.bytes_sent;
        }

        // Update model display
        app.connection_info.model = state.get_ollama_model().await;
    };

    // Save history before exiting
    if let Err(e) = app.save_history() {
        error!("Failed to save command history: {}", e);
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

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
