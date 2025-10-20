//! Integration tests for FTP protocol
//!
//! These tests spin up the NetGet server with FTP configuration
//! and use a real FTP client to verify functionality.

use netget::events::{NetworkEvent, UserCommand};
use netget::llm::{OllamaClient, PromptBuilder};
use netget::network::{ConnectionId, TcpServer};
use netget::protocol::ProtocolType;
use netget::state::app_state::{AppState, Mode};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tokio::time::sleep;

/// Helper to get an available port from the OS
async fn get_available_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

/// Test helper to start FTP server
async fn start_ftp_server() -> (AppState, u16, tokio::task::JoinHandle<()>) {
    let state = AppState::new();

    // Set up as FTP server
    state.set_mode(Mode::Server).await;
    state.set_protocol_type(ProtocolType::Ftp).await;
    state.add_instruction(format!("Serve file data.txt with content: hello")).await;

    let (network_tx, mut network_rx) = mpsc::unbounded_channel::<NetworkEvent>();

    let llm = OllamaClient::default();

    // Shared connection storage (write halves only, read halves are in separate tasks)
    let connections: Arc<Mutex<HashMap<ConnectionId, Arc<Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    // Get an available port dynamically
    let port = get_available_port().await;

    // Create and bind TCP server
    let mut tcp_server = TcpServer::new(network_tx.clone());
    let listen_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
    tcp_server.listen(listen_addr).await.unwrap();

    let state_clone = state.clone();
    let network_tx_clone = network_tx.clone();

    let connections_for_accept = connections.clone();

    // Spawn accept connections loop
    tokio::spawn(async move {
        println!("Test server: Accept loop starting...");
        loop {
            println!("Test server: Waiting for connection...");
            match tcp_server.accept().await {
                Ok(Some((stream, remote_addr))) => {
                    let connection_id = ConnectionId::new();
                    println!("Test server: Accepted connection {} from {}", connection_id, remote_addr);

                    // Split stream into read and write halves to avoid deadlock
                    let (read_half, write_half) = tokio::io::split(stream);

                    // Store write half for sending data
                    let write_half_arc = Arc::new(Mutex::new(write_half));
                    connections_for_accept.lock().await.insert(connection_id, write_half_arc);

                    // Send connected event
                    let _ = network_tx_clone.send(NetworkEvent::Connected {
                        connection_id,
                        remote_addr,
                    });

                    // Spawn handler for reading from connection
                    let network_tx_inner = network_tx_clone.clone();
                    tokio::spawn(async move {
                        use tokio::io::AsyncReadExt;
                        let mut buffer = vec![0u8; 8192];
                        let mut read_half = read_half;

                        loop {
                            match read_half.read(&mut buffer).await {
                                Ok(0) => {
                                    println!("Test server: Connection {} closed", connection_id);
                                    let _ = network_tx_inner.send(NetworkEvent::Disconnected { connection_id });
                                    break;
                                }
                                Ok(n) => {
                                    use bytes::Bytes;
                                    let data = Bytes::copy_from_slice(&buffer[..n]);
                                    println!("Test server: Read {} bytes from {}", n, connection_id);
                                    let _ = network_tx_inner.send(NetworkEvent::DataReceived {
                                        connection_id,
                                        data,
                                    });
                                }
                                Err(e) => {
                                    eprintln!("Read error: {}", e);
                                    break;
                                }
                            }
                        }
                    });
                }
                Ok(None) => break,
                Err(e) => {
                    eprintln!("Accept error: {}", e);
                    break;
                }
            }
        }
    });

    let connections_for_events = connections.clone();
    let state_for_events = state.clone();

    // Spawn event processing loop - THIS IS CRITICAL!
    // This processes network events with the LLM
    let handle = tokio::spawn(async move {
        while let Some(net_event) = network_rx.recv().await {
            println!("Test server: Processing event: {:?}", net_event);

            match &net_event {
                NetworkEvent::Connected {
                    connection_id,
                    remote_addr: _,
                } => {
                    // Ask LLM for initial greeting
                    let model = state_for_events.get_ollama_model().await;
                    let prompt =
                        PromptBuilder::build_connection_established_prompt(&state_for_events, *connection_id).await;

                    match llm.generate(&model, &prompt).await {
                        Ok(response) => {
                            let response = response.trim();
                            if !response.is_empty() && response != "NO_RESPONSE" {
                                // Send response using shared connection
                                let stream_arc_opt = {
                                    let conns = connections_for_events.lock().await;
                                    conns.get(connection_id).cloned()
                                };

                                if let Some(stream_arc) = stream_arc_opt {
                                    use tokio::io::AsyncWriteExt;
                                    let mut stream = stream_arc.lock().await;
                                    if let Err(e) = stream.write_all(response.as_bytes()).await {
                                        eprintln!("Write error: {}", e);
                                    } else if let Err(e) = stream.flush().await {
                                        eprintln!("Flush error: {}", e);
                                    } else {
                                        println!("Sent {} bytes to {}", response.len(), connection_id);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("LLM error: {}", e);
                        }
                    }
                }
                NetworkEvent::DataReceived {
                    connection_id,
                    data,
                } => {
                    // Ask LLM how to respond
                    let model = state_for_events.get_ollama_model().await;
                    let prompt =
                        PromptBuilder::build_data_received_prompt(&state_for_events, *connection_id, data).await;

                    match llm.generate(&model, &prompt).await {
                        Ok(response) => {
                            let response = response.trim();
                            if !response.is_empty() && response != "NO_RESPONSE" {
                                // Send response
                                let stream_arc_opt = {
                                    let conns = connections_for_events.lock().await;
                                    conns.get(connection_id).cloned()
                                };

                                if let Some(stream_arc) = stream_arc_opt {
                                    use tokio::io::AsyncWriteExt;
                                    let mut stream = stream_arc.lock().await;
                                    if let Err(e) = stream.write_all(response.as_bytes()).await {
                                        eprintln!("Write error: {}", e);
                                    } else if let Err(e) = stream.flush().await {
                                        eprintln!("Flush error: {}", e);
                                    } else {
                                        println!("Sent {} bytes to {}", response.len(), connection_id);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("LLM error: {}", e);
                        }
                    }
                }
                _ => {}
            }
        }
    });

    // Give server time to start
    sleep(Duration::from_millis(500)).await;

    (state_clone, port, handle)
}

#[tokio::test]
async fn test_raw_tcp_connection() {
    println!("Starting raw TCP connection test...");

    // Start the server on a dynamically allocated port
    let (_state, test_port, server_handle) = start_ftp_server().await;

    println!("Server started on port {}", test_port);

    // Give server time to initialize
    sleep(Duration::from_millis(500)).await;

    // Try raw TCP connection
    println!("Connecting raw TCP client...");
    match tokio::net::TcpStream::connect(format!("127.0.0.1:{}", test_port)).await {
        Ok(mut stream) => {
            println!("Raw TCP client connected!");

            // Read welcome message
            let mut buf = vec![0u8; 1024];
            match tokio::time::timeout(
                Duration::from_secs(10),
                tokio::io::AsyncReadExt::read(&mut stream, &mut buf)
            ).await {
                Ok(Ok(n)) => {
                    println!("Received {} bytes: {}", n, String::from_utf8_lossy(&buf[..n]));
                }
                Ok(Err(e)) => println!("Read error: {}", e),
                Err(_) => println!("Read timeout"),
            }
        }
        Err(e) => {
            eprintln!("Failed to connect: {}", e);
            panic!("TCP connection failed");
        }
    }

    server_handle.abort();
    println!("Test completed!");
}

#[tokio::test]
async fn test_ftp_server_basic_commands() {
    println!("Starting FTP integration test...");

    // Start the server on a dynamically allocated port
    let (_state, test_port, server_handle) = start_ftp_server().await;

    println!("Server started on port {}", test_port);

    // Give server more time to initialize
    sleep(Duration::from_secs(1)).await;

    // Connect with FTP client
    println!("Connecting FTP client...");
    let ftp_result = suppaftp::FtpStream::connect(format!("127.0.0.1:{}", test_port));

    match ftp_result {
        Ok(mut ftp_stream) => {
            println!("FTP client connected!");

            // Try login
            println!("Attempting login...");
            match ftp_stream.login("anonymous", "test@example.com") {
                Ok(_) => println!("Login successful!"),
                Err(e) => println!("Login failed: {}", e),
            }

            // Try PWD command
            println!("Trying PWD...");
            match ftp_stream.pwd() {
                Ok(path) => println!("PWD returned: {}", path),
                Err(e) => println!("PWD failed: {}", e),
            }

            // Try TYPE command
            println!("Trying TYPE...");
            match ftp_stream.transfer_type(suppaftp::types::FileType::Binary) {
                Ok(_) => println!("TYPE command successful"),
                Err(e) => println!("TYPE failed: {}", e),
            }

            // Disconnect
            let _ = ftp_stream.quit();
            println!("FTP client disconnected");
        }
        Err(e) => {
            eprintln!("Failed to connect FTP client: {}", e);
            eprintln!("This test requires:");
            eprintln!("1. Ollama running (ollama serve)");
            eprintln!("2. A model installed (ollama pull llama3.2:latest)");
            panic!("FTP connection failed: {}", e);
        }
    }

    // Clean up
    server_handle.abort();
    println!("Test completed!");
}

#[tokio::test]
async fn test_ftp_server_file_retrieval() {
    println!("Starting FTP file retrieval test...");

    let (_state, test_port, server_handle) = start_ftp_server().await;

    println!("Server started on port {}", test_port);
    sleep(Duration::from_secs(1)).await;

    // Connect and try to list/retrieve file
    println!("Connecting FTP client...");
    match suppaftp::FtpStream::connect(format!("127.0.0.1:{}", test_port)) {
        Ok(mut ftp_stream) => {
            println!("FTP client connected!");

            // Login
            let _ = ftp_stream.login("anonymous", "test@example.com");

            // Try to list files
            println!("Listing files...");
            match ftp_stream.list(None) {
                Ok(files) => {
                    println!("Files listed:");
                    for file in &files {
                        println!("  - {}", file);
                    }

                    // Check if data.txt is in the list
                    assert!(
                        files.iter().any(|f| f.contains("data.txt")),
                        "data.txt should be in the file list"
                    );
                }
                Err(e) => println!("LIST failed: {}", e),
            }

            let _ = ftp_stream.quit();
        }
        Err(e) => {
            eprintln!("Failed to connect: {}", e);
            panic!("FTP connection failed");
        }
    }

    server_handle.abort();
    println!("File retrieval test completed!");
}

#[test]
fn test_user_command_parsing() {
    // Test command parsing
    let cmd1 = UserCommand::parse("listen on port 21 via ftp");
    match cmd1 {
        UserCommand::Listen { port, protocol } => {
            assert_eq!(port, 21);
            assert_eq!(protocol, ProtocolType::Ftp);
        }
        _ => panic!("Expected Listen command"),
    }

    let cmd2 = UserCommand::parse("listen on port 80 via http");
    match cmd2 {
        UserCommand::Listen { port, protocol } => {
            assert_eq!(port, 80);
            assert_eq!(protocol, ProtocolType::Http);
        }
        _ => panic!("Expected Listen command"),
    }

    let cmd3 = UserCommand::parse("close");
    match cmd3 {
        UserCommand::Close => {},
        _ => panic!("Expected Close command"),
    }

    let cmd4 = UserCommand::parse("status");
    match cmd4 {
        UserCommand::Status => {},
        _ => panic!("Expected Status command"),
    }

    let cmd5 = UserCommand::parse("model deepseek-coder:latest");
    match cmd5 {
        UserCommand::ChangeModel { model } => {
            assert_eq!(model, "deepseek-coder:latest");
        }
        _ => panic!("Expected ChangeModel command"),
    }

    let cmd6 = UserCommand::parse("model llama3.2:latest");
    match cmd6 {
        UserCommand::ChangeModel { model } => {
            assert_eq!(model, "llama3.2:latest");
        }
        _ => panic!("Expected ChangeModel command"),
    }
}
