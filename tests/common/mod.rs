//! Common test utilities for integration tests
//!
//! This module provides helpers to set up NetGet servers for testing
//! in a simple, black-box manner using only prompts.

use bytes::Bytes;
use netget::events::{HttpResponse, NetworkEvent, UserCommand};
use netget::llm::{HttpLlmResponse, OllamaClient, PromptBuilder};
use netget::network::{ConnectionId, HttpServer, TcpServer};
use netget::protocol::BaseStack;
use netget::state::app_state::{AppState, Mode};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tokio::time::sleep;

/// Get an available port for testing
pub async fn get_available_port() -> u16 {
    use tokio::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

/// Simple black-box test helper: Start a server with a prompt
///
/// This function simulates what a user would do - just provide a prompt/command
/// and the system infers everything (mode, stack, protocol) from it.
///
/// # Arguments
/// * `prompt` - User prompt/command (e.g., "listen on port 21 via ftp. Serve file data.txt")
///
/// # Returns
/// * `(AppState, u16, JoinHandle)` - State, port, and event loop handle for cleanup
pub async fn start_server_with_prompt(
    prompt: &str,
) -> (AppState, u16, tokio::task::JoinHandle<()>) {
    let state = AppState::new();

    // Parse the command to infer configuration
    let cmd = UserCommand::parse(prompt);

    // Extract port and configuration from command
    let (port, base_stack, protocol) = match cmd {
        UserCommand::Listen {
            port,
            base_stack,
            protocol,
        } => (port, base_stack, protocol),
        _ => panic!("Prompt must contain a listen command"),
    };

    // Get a dynamic port if not specified (port 0 = auto-assign)
    let port = if port == 0 {
        get_available_port().await
    } else {
        port
    };

    // Set up state based on parsed command
    state.set_mode(Mode::Server).await;
    state.set_base_stack(base_stack).await;
    if base_stack == BaseStack::TcpRaw {
        state.set_protocol_type(protocol).await;
    }

    // Add the full prompt as instructions for the LLM
    state.add_instruction(prompt.to_string()).await;

    let (network_tx, mut network_rx) = mpsc::unbounded_channel::<NetworkEvent>();
    let llm = OllamaClient::default();

    let listen_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

    match base_stack {
        BaseStack::TcpRaw => {
            // Set up TCP server
            start_tcp_server(state.clone(), listen_addr, network_tx.clone(), llm).await
        }
        BaseStack::Http => {
            // Set up HTTP server
            start_http_server(state.clone(), listen_addr, network_tx.clone(), llm).await
        }
        BaseStack::DataLink => {
            panic!("DataLink stack tests not yet implemented - requires network interface access");
        }
    }

    // Spawn event processing loop
    let state_for_events = state.clone();
    let handle = tokio::spawn(async move {
        process_events(&state_for_events, &mut network_rx).await;
    });

    // Wait for server to be ready
    sleep(Duration::from_millis(500)).await;

    (state, port, handle)
}

/// Start TCP server and accept loop
async fn start_tcp_server(
    _state: AppState,
    listen_addr: SocketAddr,
    network_tx: mpsc::UnboundedSender<NetworkEvent>,
    _llm: OllamaClient,
) {
    // Shared connection storage (write halves only)
    let connections: Arc<
        Mutex<HashMap<ConnectionId, Arc<Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>>>,
    > = Arc::new(Mutex::new(HashMap::new()));

    // Create and bind TCP server
    let mut tcp_server = TcpServer::new(network_tx.clone());
    tcp_server.listen(listen_addr).await.unwrap();

    let network_tx_clone = network_tx.clone();
    let connections_for_accept = connections.clone();

    // Spawn accept connections loop
    tokio::spawn(async move {
        loop {
            match tcp_server.accept().await {
                Ok(Some((stream, remote_addr))) => {
                    let connection_id = ConnectionId::new();

                    // Split stream into read and write halves
                    let (read_half, write_half) = tokio::io::split(stream);
                    let write_half_arc = Arc::new(Mutex::new(write_half));
                    connections_for_accept
                        .lock()
                        .await
                        .insert(connection_id, write_half_arc);

                    // Send connected event
                    let _ = network_tx_clone.send(NetworkEvent::Connected {
                        connection_id,
                        remote_addr,
                    });

                    // Spawn reader task
                    let network_tx_inner = network_tx_clone.clone();
                    tokio::spawn(async move {
                        use tokio::io::AsyncReadExt;
                        let mut buffer = vec![0u8; 8192];
                        let mut read_half = read_half;

                        loop {
                            match read_half.read(&mut buffer).await {
                                Ok(0) => {
                                    let _ = network_tx_inner
                                        .send(NetworkEvent::Disconnected { connection_id });
                                    break;
                                }
                                Ok(n) => {
                                    let data = Bytes::copy_from_slice(&buffer[..n]);
                                    let _ = network_tx_inner.send(NetworkEvent::DataReceived {
                                        connection_id,
                                        data,
                                    });
                                }
                                Err(_) => break,
                            }
                        }
                    });
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
    });

    // Store connections for event processing
    // We'll need to pass this to the event processor somehow
    // For now, we'll handle this in the event processing loop
    CONNECTIONS.lock().await.replace(connections);
}

/// Start HTTP server and accept loop
async fn start_http_server(
    _state: AppState,
    listen_addr: SocketAddr,
    network_tx: mpsc::UnboundedSender<NetworkEvent>,
    _llm: OllamaClient,
) {
    let http_server = HttpServer::new(listen_addr, network_tx.clone())
        .await
        .expect("Failed to create HTTP server");

    tokio::spawn(async move {
        if let Err(e) = http_server.accept_loop().await {
            eprintln!("HTTP server error: {}", e);
        }
    });
}

/// Global connection storage for TCP tests
/// This is a workaround to share connections between server setup and event processing
static CONNECTIONS: tokio::sync::Mutex<
    Option<Arc<Mutex<HashMap<ConnectionId, Arc<Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>>>>>,
> = tokio::sync::Mutex::const_new(None);

/// Process network events with LLM
async fn process_events(state: &AppState, network_rx: &mut mpsc::UnboundedReceiver<NetworkEvent>) {
    let llm = OllamaClient::default();

    while let Some(event) = network_rx.recv().await {
        match event {
            NetworkEvent::Connected {
                connection_id,
                remote_addr: _,
            } => {
                // Ask LLM for initial greeting
                let model = state.get_ollama_model().await;
                let prompt =
                    PromptBuilder::build_connection_established_prompt(state, connection_id).await;

                if let Ok(response) = llm.generate(&model, &prompt).await {
                    let response = response.trim();
                    if !response.is_empty() && response != "NO_RESPONSE" {
                        if let Some(connections) = CONNECTIONS.lock().await.as_ref() {
                            if let Some(write_half_arc) = connections.lock().await.get(&connection_id)
                            {
                                use tokio::io::AsyncWriteExt;
                                let mut write_half = write_half_arc.lock().await;
                                let _ = write_half.write_all(response.as_bytes()).await;
                                let _ = write_half.flush().await;
                            }
                        }
                    }
                }
            }
            NetworkEvent::DataReceived {
                connection_id,
                data,
            } => {
                // Ask LLM how to respond
                let model = state.get_ollama_model().await;
                let prompt =
                    PromptBuilder::build_data_received_prompt(state, connection_id, &data).await;

                if let Ok(response) = llm.generate(&model, &prompt).await {
                    let response = response.trim();
                    if !response.is_empty() && response != "NO_RESPONSE" {
                        if let Some(connections) = CONNECTIONS.lock().await.as_ref() {
                            if let Some(write_half_arc) = connections.lock().await.get(&connection_id)
                            {
                                use tokio::io::AsyncWriteExt;
                                let mut write_half = write_half_arc.lock().await;
                                let _ = write_half.write_all(response.as_bytes()).await;
                                let _ = write_half.flush().await;
                            }
                        }
                    }
                }
            }
            NetworkEvent::HttpRequest {
                connection_id,
                method,
                uri,
                headers,
                body,
                response_tx,
            } => {
                // Ask LLM to generate HTTP response
                let model = state.get_ollama_model().await;
                let prompt = PromptBuilder::build_http_request_prompt(
                    state,
                    connection_id,
                    &method,
                    &uri,
                    &headers,
                    &body,
                )
                .await;

                match llm.generate(&model, &prompt).await {
                    Ok(raw_response) => match HttpLlmResponse::from_str(&raw_response) {
                        Ok(http_response) => {
                            let _ = response_tx.send(http_response.to_event_response());
                        }
                        Err(_) => {
                            // Send 500 error on parse failure
                            let _ = response_tx.send(HttpResponse {
                                status: 500,
                                headers: HashMap::new(),
                                body: Bytes::from("Internal Server Error"),
                            });
                        }
                    },
                    Err(_) => {
                        // Send 500 error on LLM failure
                        let _ = response_tx.send(HttpResponse {
                            status: 500,
                            headers: HashMap::new(),
                            body: Bytes::from("Internal Server Error"),
                        });
                    }
                }
            }
            _ => {}
        }
    }
}
