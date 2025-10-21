//! Non-interactive mode execution
//!
//! This module handles execution when NetGet runs without the TUI,
//! processing a single prompt and outputting results to stdout/stderr.

use anyhow::Result;
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tokio::time::timeout;
use tracing::{error, info, warn};

use crate::events::{AppEvent, NetworkEvent, UserCommand};
use crate::llm::{CommandAction, LlmResponse, OllamaClient, PromptBuilder};
use crate::network::ConnectionId;
use crate::protocol::BaseStack;
use crate::settings::Settings;
use crate::state::app_state::{AppState, Mode};

use super::server_startup;

/// Run NetGet in non-interactive mode with the given prompt
pub async fn run_non_interactive(
    prompt: String,
    args: &super::Args,
    settings: Settings,
) -> Result<()> {
    println!("Starting NetGet in non-interactive mode...");
    println!("Prompt: {}", prompt);

    // Create application state
    let state = AppState::new();

    // Override model if specified in args
    if let Some(model) = &args.model {
        state.set_ollama_model(model.clone()).await;
        println!("Using model: {}", model);
    } else if !settings.model.is_empty() {
        state.set_ollama_model(settings.model.clone()).await;
        println!("Using model from settings: {}", settings.model);
    } else {
        println!("Using default model: {}", state.get_ollama_model().await);
    }

    // Parse the command
    let command = UserCommand::parse(&prompt);

    // Process based on command type
    match command {
        UserCommand::Interpret { input } => {
            // Use LLM to interpret and execute
            let llm = OllamaClient::default();
            let model = state.get_ollama_model().await;
            let prompt = PromptBuilder::build_command_interpretation_prompt(&state, &input).await;

            println!("Sending prompt to LLM for interpretation...");
            match llm.generate_command_interpretation(&model, &prompt).await {
                Ok(interpretation) => {
                    // Display message from LLM
                    if let Some(msg) = &interpretation.message {
                        println!("LLM: {}", msg);
                    }

                    // Execute actions
                    for action in interpretation.actions {
                        match action {
                            CommandAction::UpdateInstruction { instruction } => {
                                println!("Setting instruction: {}", instruction);
                                state.set_instruction(instruction).await;
                            }
                            CommandAction::OpenServer {
                                port,
                                base_stack: stack_str,
                                send_banner,
                                initial_memory,
                            } => {
                                let stack = crate::protocol::BaseStack::from_str(&stack_str)
                                    .unwrap_or(crate::protocol::BaseStack::TcpRaw);
                                state.set_mode(Mode::Server).await;
                                state.set_base_stack(stack).await;
                                state.set_port(port).await;
                                state.set_send_banner(send_banner).await;

                                // Set initial memory if provided
                                if let Some(mem) = initial_memory {
                                    state.set_memory(mem).await;
                                }

                                println!("Starting {} server on port {}...", stack, port);
                                println!("Listen address: {}", args.listen_addr);

                                // Run the server
                                return run_server(&state, llm).await;
                            }
                            CommandAction::ShowMessage { message } => {
                                println!("{}", message);
                            }
                            CommandAction::ChangeModel { model: new_model } => {
                                println!("Switching model to: {}", new_model);
                                state.set_ollama_model(new_model).await;
                            }
                            _ => {
                                // Ignore other actions in non-interactive mode
                            }
                        }
                    }

                    // If no server was started, we're done
                    if state.get_mode().await != Mode::Server {
                        println!("Command executed successfully.");
                    }
                }
                Err(e) => {
                    error!("LLM interpretation failed: {}", e);
                    return Err(e);
                }
            }
        }
        _ => {
            // Slash commands not supported in non-interactive mode
            println!("Slash commands are not supported in non-interactive mode.");
            println!("Please provide a natural language prompt.");
            return Err(anyhow::anyhow!("Unsupported command type"));
        }
    }

    Ok(())
}

/// Run a server in non-interactive mode
async fn run_server(state: &AppState, llm: OllamaClient) -> Result<()> {
    // Create event channels
    let (network_tx, mut network_rx) = mpsc::unbounded_channel();
    let (status_tx, mut status_rx) = mpsc::unbounded_channel::<String>();

    // Connection management
    type WriteHalfMap = Arc<
        Mutex<
            HashMap<
                ConnectionId,
                Arc<Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>,
            >,
        >,
    >;
    let connections: WriteHalfMap = Arc::new(Mutex::new(HashMap::new()));
    let cancellation_tokens = Arc::new(Mutex::new(HashMap::new()));

    // Connection state for queueing
    let connection_states: Arc<Mutex<HashMap<ConnectionId, ConnectionState>>> =
        Arc::new(Mutex::new(HashMap::new()));

    // Start the server
    server_startup::check_and_start_server(
        state,
        &network_tx,
        &connections,
        &cancellation_tokens,
        &status_tx,
    )
    .await?;

    println!("Server is running. Press Ctrl+C to stop.");
    println!("Waiting for connections...\n");

    // Set up Ctrl+C handler
    let shutdown = Arc::new(Mutex::new(false));
    let shutdown_clone = shutdown.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        let mut shutdown = shutdown_clone.lock().await;
        *shutdown = true;
    });

    // Main event loop
    loop {
        // Check for shutdown
        if *shutdown.lock().await {
            println!("\nShutting down server...");
            break;
        }

        // Process status messages
        while let Ok(msg) = status_rx.try_recv() {
            println!("[STATUS] {}", msg);
        }

        // Process network events with timeout to allow checking shutdown
        match timeout(Duration::from_millis(100), network_rx.recv()).await {
            Ok(Some(event)) => {
                process_network_event(
                    event,
                    state,
                    &llm,
                    &connections,
                    &connection_states,
                    &network_tx,
                    &status_tx,
                )
                .await;
            }
            Ok(None) => {
                // Channel closed
                break;
            }
            Err(_) => {
                // Timeout - continue to check shutdown
                continue;
            }
        }
    }

    println!("Server stopped.");
    Ok(())
}

/// Connection processing state for LLM queueing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionStatus {
    Idle,
    Processing,
    Accumulating,
}

/// Per-connection state for queueing and LLM processing
struct ConnectionState {
    status: ConnectionStatus,
    queue: Vec<u8>,
    memory: String,
}

impl ConnectionState {
    fn new() -> Self {
        Self {
            status: ConnectionStatus::Idle,
            queue: Vec::new(),
            memory: String::new(),
        }
    }
}

/// Process a network event in non-interactive mode
async fn process_network_event(
    event: NetworkEvent,
    state: &AppState,
    llm: &OllamaClient,
    connections: &Arc<Mutex<HashMap<ConnectionId, Arc<Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>>>>,
    connection_states: &Arc<Mutex<HashMap<ConnectionId, ConnectionState>>>,
    network_tx: &mpsc::UnboundedSender<NetworkEvent>,
    status_tx: &mpsc::UnboundedSender<String>,
) {
    match event {
        NetworkEvent::Connected {
            connection_id,
            remote_addr,
        } => {
            println!(
                "[CONNECTED] Connection {} from {}",
                connection_id, remote_addr
            );

            // Initialize connection state
            connection_states
                .lock()
                .await
                .insert(connection_id, ConnectionState::new());

            // Generate initial response if needed
            if state.get_send_banner().await {
                let model = state.get_ollama_model().await;
                let prompt =
                    PromptBuilder::build_connection_established_prompt(state, connection_id, "").await;

                match llm.generate_llm_response(&model, &prompt).await {
                    Ok(response) => {
                        if let Some(output) = response.output {
                            if !output.is_empty() {
                                println!("→ Sending initial response to connection {}", connection_id);
                                send_response(connections, connection_id, output.as_bytes()).await;
                            }
                        }

                        if response.close_connection {
                            println!("→ Closing connection {} (LLM requested)", connection_id);
                            close_connection(connections, connection_states, connection_id).await;
                        }
                    }
                    Err(e) => {
                        error!("Failed to generate initial response: {}", e);
                    }
                }
            }
        }

        NetworkEvent::DataReceived {
            connection_id,
            data,
        } => {
            // Display received data
            match String::from_utf8(data.to_vec()) {
                Ok(text) => {
                    let display = if text.len() > 200 {
                        format!("{}... ({} bytes)", &text[..200], text.len())
                    } else {
                        text.clone()
                    };
                    println!("← Received from connection {}: {}", connection_id, display);
                }
                Err(_) => {
                    println!(
                        "← Received {} bytes of binary data from connection {}",
                        data.len(),
                        connection_id
                    );
                }
            }

            // Process with LLM (spawn task for async processing)
            let state = state.clone();
            let llm = llm.clone();
            let connections = connections.clone();
            let connection_states = connection_states.clone();
            let status_tx = status_tx.clone();

            tokio::spawn(async move {
                process_data_with_llm(
                    connection_id,
                    data.to_vec(),
                    &state,
                    &llm,
                    &connections,
                    &connection_states,
                    &status_tx,
                )
                .await;
            });
        }

        NetworkEvent::Disconnected {
            connection_id,
        } => {
            println!("[DISCONNECTED] Connection {}", connection_id);

            // Clean up connection state
            connections.lock().await.remove(&connection_id);
            connection_states.lock().await.remove(&connection_id);
        }

        NetworkEvent::HttpRequest {
            connection_id,
            method,
            uri,
            headers,
            body,
            response_tx,
        } => {
            println!("[HTTP] {} {} from connection {}", method, uri, connection_id);

            // Generate HTTP response with LLM
            let model = state.get_ollama_model().await;
            let prompt = PromptBuilder::build_http_request_prompt(
                state, connection_id, &method, &uri, &headers, &body, "",
            )
            .await;

            match llm.generate_http_response(&model, &prompt).await {
                Ok(response) => {
                    println!(
                        "→ Sending HTTP {} response to connection {}",
                        response.status, connection_id
                    );

                    let _ = response_tx.send(crate::events::HttpResponse {
                        status: response.status,
                        headers: response.headers,
                        body: Bytes::from(response.body),
                    });
                }
                Err(e) => {
                    error!("Failed to generate HTTP response: {}", e);
                    // Send a 500 error response
                    let _ = response_tx.send(crate::events::HttpResponse {
                        status: 500,
                        headers: HashMap::new(),
                        body: Bytes::from("Internal Server Error"),
                    });
                }
            }
        }

        _ => {
            // Other events we might not handle in non-interactive mode
            info!("Unhandled event: {:?}", event);
        }
    }
}

/// Process data with LLM, handling queueing and accumulation
async fn process_data_with_llm(
    connection_id: ConnectionId,
    mut data: Vec<u8>,
    state: &AppState,
    llm: &OllamaClient,
    connections: &Arc<Mutex<HashMap<ConnectionId, Arc<Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>>>>,
    connection_states: &Arc<Mutex<HashMap<ConnectionId, ConnectionState>>>,
    status_tx: &mpsc::UnboundedSender<String>,
) {
    // Check and update connection state
    let mut states = connection_states.lock().await;
    let conn_state = states.entry(connection_id).or_insert_with(ConnectionState::new);

    // Check if we're already processing
    if conn_state.status == ConnectionStatus::Processing {
        // Queue the data
        conn_state.queue.extend_from_slice(&data);
        let _ = status_tx.send(format!(
            "Queued {} bytes for connection {} (LLM is processing)",
            data.len(),
            connection_id
        ));
        return;
    }

    // If accumulating, merge with new data
    if conn_state.status == ConnectionStatus::Accumulating {
        conn_state.queue.extend_from_slice(&data);
        data = conn_state.queue.clone();
        conn_state.queue.clear();
    }

    // Mark as processing
    conn_state.status = ConnectionStatus::Processing;
    let memory = conn_state.memory.clone();
    drop(states); // Release lock while calling LLM

    // Loop to handle queued data
    loop {
        // Generate LLM response
        let model = state.get_ollama_model().await;
        let prompt = PromptBuilder::build_data_received_prompt(
            state,
            connection_id,
            &Bytes::from(data.clone()),
            &memory,
        )
        .await;

        match llm.generate_llm_response(&model, &prompt).await {
            Ok(response) => {
                // Handle response
                if let Some(output) = &response.output {
                    if !output.is_empty() {
                        println!("→ Sending response to connection {}", connection_id);
                        send_response(connections, connection_id, output.as_bytes()).await;
                    }
                }

                // Update memory if provided
                let mut states = connection_states.lock().await;
                if let Some(conn_state) = states.get_mut(&connection_id) {
                    if let Some(new_memory) = &response.set_connection_memory {
                        conn_state.memory = new_memory.clone();
                    } else if let Some(append_memory) = &response.append_connection_memory {
                        if !conn_state.memory.is_empty() {
                            conn_state.memory.push('\n');
                        }
                        conn_state.memory.push_str(append_memory);
                    }

                    // Handle special flags
                    if response.wait_for_more {
                        conn_state.status = ConnectionStatus::Accumulating;
                        conn_state.queue.clear();
                        let _ = status_tx.send(format!(
                            "Waiting for more data from connection {} (LLM requested)",
                            connection_id
                        ));
                        return;
                    }

                    if response.close_connection {
                        drop(states);
                        println!("→ Closing connection {} (LLM requested)", connection_id);
                        close_connection(connections, connection_states, connection_id).await;
                        return;
                    }

                    // Check for queued data
                    if !conn_state.queue.is_empty() {
                        data = conn_state.queue.clone();
                        conn_state.queue.clear();
                        let _ = status_tx.send(format!(
                            "Processing {} queued bytes for connection {}",
                            data.len(),
                            connection_id
                        ));
                        drop(states);
                        // Loop to process queued data
                        continue;
                    }

                    // No more data, go idle
                    conn_state.status = ConnectionStatus::Idle;
                }
                break;
            }
            Err(e) => {
                error!("LLM error for connection {}: {}", connection_id, e);

                // Reset to idle on error
                let mut states = connection_states.lock().await;
                if let Some(conn_state) = states.get_mut(&connection_id) {
                    conn_state.status = ConnectionStatus::Idle;
                }
                break;
            }
        }
    }
}

/// Send a response to a connection
async fn send_response(
    connections: &Arc<Mutex<HashMap<ConnectionId, Arc<Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>>>>,
    connection_id: ConnectionId,
    data: &[u8],
) {
    use tokio::io::AsyncWriteExt;

    if let Some(write_half_arc) = connections.lock().await.get(&connection_id) {
        if let Ok(mut write_half) = write_half_arc.try_lock() {
            if let Err(e) = write_half.write_all(data).await {
                error!("Failed to send response to connection {}: {}", connection_id, e);
            } else if let Err(e) = write_half.flush().await {
                error!("Failed to flush response to connection {}: {}", connection_id, e);
            }
        } else {
            warn!("Connection {} write half is locked", connection_id);
        }
    } else {
        warn!("Connection {} not found in connection map", connection_id);
    }
}

/// Close a connection
async fn close_connection(
    connections: &Arc<Mutex<HashMap<ConnectionId, Arc<Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>>>>,
    connection_states: &Arc<Mutex<HashMap<ConnectionId, ConnectionState>>>,
    connection_id: ConnectionId,
) {
    connections.lock().await.remove(&connection_id);
    connection_states.lock().await.remove(&connection_id);
}