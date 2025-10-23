//! End-to-end TCP tests for NetGet
//!
//! These tests spawn the actual NetGet binary with TCP/FTP prompts
//! and validate the responses using real TCP/FTP clients.

#![cfg(feature = "e2e-tests")]

mod e2e;

use e2e::helpers::{self, ServerConfig, E2EResult};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::time::Duration;

#[tokio::test]
async fn test_ftp_server() -> E2EResult<()> {
    println!("\n=== E2E Test: FTP Server ===");

    // PROMPT: Tell the LLM to act as an FTP server
    let port = helpers::get_available_port().await?;
    let prompt = format!("listen on port {} via ftp. Serve file data.txt with content: hello world", port);

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started: {} stack on port {}", server.stack, server.port);

    // Give the server a moment to fully start
    tokio::time::sleep(Duration::from_secs(1)).await;

    // VALIDATION: Use real FTP client to verify behavior
    println!("Connecting FTP client...");
    match suppaftp::FtpStream::connect(format!("127.0.0.1:{}", server.port)) {
        Ok(mut ftp_stream) => {
            println!("✓ FTP client connected successfully");

            // Try login
            println!("Attempting login...");
            match ftp_stream.login("anonymous", "test@example.com") {
                Ok(_) => println!("✓ Login successful"),
                Err(e) => println!("✗ Login failed: {}", e),
            }

            // Try PWD command
            println!("Trying PWD...");
            match ftp_stream.pwd() {
                Ok(path) => println!("✓ PWD returned: {}", path),
                Err(e) => println!("✗ PWD failed: {}", e),
            }

            // Try TYPE command
            println!("Trying TYPE...");
            match ftp_stream.transfer_type(suppaftp::types::FileType::Binary) {
                Ok(_) => println!("✓ TYPE command successful"),
                Err(e) => println!("✗ TYPE failed: {}", e),
            }

            // Disconnect
            let _ = ftp_stream.quit();
            println!("✓ FTP test completed");
        }
        Err(e) => {
            return Err(format!(
                "FTP connection failed: {}. Ensure Ollama is running with a model installed.",
                e
            ).into());
        }
    }

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_simple_echo() -> E2EResult<()> {
    println!("\n=== E2E Test: Simple Echo Server ===");

    // PROMPT: Tell the LLM to echo back with ACK prefix
    let port = helpers::get_available_port().await?;
    let prompt = format!("listen on port {} via tcp. When you receive any data, reply with 'ACK: ' followed by the received data", port);

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_millis(500)).await;

    // VALIDATION: Send data and verify echo response
    println!("Connecting TCP client...");
    let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP client connected");

    // Send test data
    let test_message = "Hello, LLM!";
    println!("Sending: {}", test_message);
    stream.write_all(test_message.as_bytes()).await?;
    stream.flush().await?;

    // Read response with timeout
    let mut buffer = vec![0u8; 1024];
    match tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buffer)).await {
        Ok(Ok(n)) if n > 0 => {
            let response = String::from_utf8_lossy(&buffer[..n]);
            println!("Received: {}", response);

            // Verify response format
            assert!(
                response.contains("ACK"),
                "Response should contain 'ACK', got: {}",
                response
            );
            assert!(
                response.contains(test_message),
                "Response should echo the message, got: {}",
                response
            );

            println!("✓ Echo test passed");
        }
        Ok(Ok(_)) => return Err("Connection closed without response".into()),
        Ok(Err(e)) => return Err(format!("Read error: {}", e).into()),
        Err(_) => return Err("Response timeout".into()),
    }

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_custom_response() -> E2EResult<()> {
    println!("\n=== E2E Test: Custom Response Server ===");

    // PROMPT: Tell the LLM to send a specific response
    let port = helpers::get_available_port().await?;
    let prompt = format!("listen on port {} via tcp. When a client connects, send the greeting 'Welcome to the test server!\\r\\n'. When you receive 'PING', respond with 'PONG\\r\\n'", port);

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_millis(500)).await;

    // VALIDATION: Verify greeting and PING/PONG
    println!("Connecting TCP client...");
    let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP client connected");

    // Read greeting
    let mut buffer = vec![0u8; 1024];
    match tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buffer)).await {
        Ok(Ok(n)) if n > 0 => {
            let greeting = String::from_utf8_lossy(&buffer[..n]);
            println!("Received greeting: {}", greeting);
            assert!(
                greeting.contains("Welcome"),
                "Expected welcome message, got: {}",
                greeting
            );
            println!("✓ Greeting verified");
        }
        _ => return Err("Failed to receive greeting".into()),
    }

    // Send PING
    println!("Sending: PING");
    stream.write_all(b"PING").await?;
    stream.flush().await?;

    // Read PONG
    let mut buffer = vec![0u8; 1024];
    match tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buffer)).await {
        Ok(Ok(n)) if n > 0 => {
            let response = String::from_utf8_lossy(&buffer[..n]);
            println!("Received: {}", response);
            assert!(
                response.contains("PONG"),
                "Expected PONG response, got: {}",
                response
            );
            println!("✓ PING/PONG verified");
        }
        _ => return Err("Failed to receive PONG".into()),
    }

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}
