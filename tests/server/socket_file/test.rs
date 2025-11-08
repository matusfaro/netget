//! End-to-end Unix domain socket tests for NetGet
//!
//! These tests spawn the actual NetGet binary with socket file prompts
//! and validate the responses using UnixStream connections.
//!
//! Platform: Unix/Linux only

#![cfg(all(feature = "socket_file", unix))]

use super::super::super::helpers::{self, ServerConfig, E2EResult};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use std::time::Duration;

#[tokio::test]
async fn test_socket_echo() -> E2EResult<()> {
    println!("\n=== E2E Test: Socket File Echo Server ===");

    // PROMPT: Tell the LLM to echo back with ACK prefix
    let prompt = "Create socket file at /tmp/netget-test-echo.sock. When you receive any data, reply with 'ACK: ' followed by the received data";

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started with socket file");

    // Wait a bit for socket file to be created
    tokio::time::sleep(Duration::from_millis(500)).await;

    // VALIDATION: Send data and verify echo response
    println!("Connecting Unix socket client...");
    let mut stream = UnixStream::connect("/tmp/netget-test-echo.sock").await?;
    println!("✓ Unix socket client connected");

    // Send test data
    let test_message = "Hello, Socket!";
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

            println!("✓ Socket file echo test passed");
        }
        Ok(Ok(_)) => return Err("Connection closed without response".into()),
        Ok(Err(e)) => return Err(format!("Read error: {}", e).into()),
        Err(_) => return Err("Response timeout".into()),
    }

    server.stop().await?;

    // Cleanup socket file
    let _ = std::fs::remove_file("/tmp/netget-test-echo.sock");

    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_socket_ping_pong() -> E2EResult<()> {
    println!("\n=== E2E Test: Socket File PING/PONG ===");

    // PROMPT: Tell the LLM to respond to PING with PONG
    let prompt = "Create socket file at /tmp/netget-test-ping.sock. When you receive 'PING', respond with 'PONG\\n'";

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started with socket file");

    // Wait a bit for socket file to be created
    tokio::time::sleep(Duration::from_millis(500)).await;

    // VALIDATION: Verify PING/PONG
    println!("Connecting Unix socket client...");
    let mut stream = UnixStream::connect("/tmp/netget-test-ping.sock").await?;
    println!("✓ Unix socket client connected");

    // Send PING
    println!("Sending: PING");
    stream.write_all(b"PING").await?;
    stream.flush().await?;

    // Read PONG
    let mut buffer = vec![0u8; 1024];
    match tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buffer)).await {
        Ok(Ok(n)) if n > 0 => {
            let response = String::from_utf8_lossy(&buffer[..n]);
            println!("Received: {}", response.trim());
            assert!(
                response.contains("PONG"),
                "Expected PONG response, got: {}",
                response
            );
            println!("✓ PING/PONG test passed");
        }
        Ok(Ok(_)) => return Err("Connection closed without response".into()),
        Ok(Err(e)) => return Err(format!("Read error: {}", e).into()),
        Err(_) => return Err("Response timeout".into()),
    }

    server.stop().await?;

    // Cleanup socket file
    let _ = std::fs::remove_file("/tmp/netget-test-ping.sock");

    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_socket_line_protocol() -> E2EResult<()> {
    println!("\n=== E2E Test: Socket File Line Protocol ===");

    // PROMPT: Tell the LLM to respond to line-based commands
    let prompt = "Create socket file at /tmp/netget-test-line.sock. When you receive a line ending with \\n, respond with 'OK: <line>\\n'";

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started with socket file");

    // Wait a bit for socket file to be created
    tokio::time::sleep(Duration::from_millis(500)).await;

    // VALIDATION: Send line and verify response
    println!("Connecting Unix socket client...");
    let mut stream = UnixStream::connect("/tmp/netget-test-line.sock").await?;
    println!("✓ Unix socket client connected");

    // Send command line
    let command = "TEST COMMAND\n";
    println!("Sending: {}", command.trim());
    stream.write_all(command.as_bytes()).await?;
    stream.flush().await?;

    // Read response
    let mut buffer = vec![0u8; 1024];
    match tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buffer)).await {
        Ok(Ok(n)) if n > 0 => {
            let response = String::from_utf8_lossy(&buffer[..n]);
            println!("Received: {}", response.trim());

            assert!(
                response.starts_with("OK:"),
                "Expected OK: response, got: {}",
                response
            );
            assert!(
                response.contains("TEST COMMAND"),
                "Response should contain command, got: {}",
                response
            );
            println!("✓ Line protocol test passed");
        }
        Ok(Ok(_)) => return Err("Connection closed without response".into()),
        Ok(Err(e)) => return Err(format!("Read error: {}", e).into()),
        Err(_) => return Err("Response timeout".into()),
    }

    server.stop().await?;

    // Cleanup socket file
    let _ = std::fs::remove_file("/tmp/netget-test-line.sock");

    println!("=== Test passed ===\n");
    Ok(())
}
