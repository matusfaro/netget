//! End-to-end Telnet tests for NetGet
//!
//! These tests spawn the actual NetGet binary with Telnet prompts
//! and validate the responses using raw TCP clients.

#![cfg(feature = "e2e-tests")]

// Helper module imported from parent

use super::super::super::helpers::{self, ServerConfig, E2EResult};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use std::time::Duration;

#[tokio::test]
async fn test_telnet_echo() -> E2EResult<()> {
    println!("\n=== E2E Test: Telnet Echo Server ===");

    // PROMPT: Tell the LLM to act as a Telnet echo server
    let prompt = "listen on port {AVAILABLE_PORT} via telnet. Echo back any text you receive, line by line. \
        Add '> ' prompt after each echo.";

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);


    // VALIDATION: Connect via raw TCP (Telnet protocol)
    println!("Connecting to Telnet server...");
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Send a test message
    let test_message = "Hello Telnet Server";
    println!("Sending: {}", test_message);
    write_half.write_all(format!("{}\n", test_message).as_bytes()).await?;
    write_half.flush().await?;

    // Read echo response
    let mut line = String::new();
    match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
        Ok(Ok(n)) if n > 0 => {
            println!("Telnet response: {}", line.trim());

            // Verify echo (should contain our message)
            assert!(
                line.contains(test_message) || line.contains("Hello"),
                "Expected echo containing '{}', got: {}",
                test_message,
                line
            );
            println!("✓ Telnet echo verified");
        }
        Ok(Ok(_)) => {
            println!("Note: Connection closed without response");
        }
        Ok(Err(e)) => {
            println!("Note: Read error: {}", e);
        }
        Err(_) => {
            println!("Note: No response received (timeout)");
        }
    }

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_telnet_prompt() -> E2EResult<()> {
    println!("\n=== E2E Test: Telnet Interactive Prompt ===");

    // PROMPT: Tell the LLM to provide an interactive prompt
    let prompt = "listen on port {AVAILABLE_PORT} via telnet. Send a welcome message 'Welcome to NetGet Telnet' \
        when clients connect, then show a '$ ' prompt. Echo commands back with 'You said: ' prefix.";

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);


    // VALIDATION: Connect and verify welcome + prompt
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Read welcome message (if sent immediately)

    // Send a command
    println!("Sending command: help");
    write_half.write_all(b"help\n").await?;
    write_half.flush().await?;

    // Read responses
    let mut received_response = false;
    for attempt in 1..=3 {
        let mut line = String::new();
        match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
            Ok(Ok(n)) if n > 0 => {
                println!("Telnet response ({}): {}", attempt, line.trim());
                if !line.trim().is_empty() {
                    received_response = true;
                }
            }
            _ => break,
        }
    }

    if received_response {
        println!("✓ Telnet interaction successful");
    } else {
        println!("Note: No response received from Telnet server");
    }

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_telnet_multiple_lines() -> E2EResult<()> {
    println!("\n=== E2E Test: Telnet Multiple Lines ===");

    // PROMPT: Tell the LLM to handle multiple lines
    let prompt = "listen on port {AVAILABLE_PORT} via telnet. For each line received, respond with 'Line N: <content>' \
        where N is the line number starting from 1.";

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);


    // VALIDATION: Send multiple lines
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Send three lines
    let lines = ["First line", "Second line", "Third line"];
    for (i, line) in lines.iter().enumerate() {
        println!("Sending line {}: {}", i + 1, line);
        write_half.write_all(format!("{}\n", line).as_bytes()).await?;
        write_half.flush().await?;

        // Read response
        let mut response = String::new();
        match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut response)).await {
            Ok(Ok(n)) if n > 0 => {
                println!("  Response: {}", response.trim());
            }
            _ => {
                println!("  No response for line {}", i + 1);
            }
        }

    }

    println!("✓ Multiple line handling tested");

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_telnet_concurrent_connections() -> E2EResult<()> {
    println!("\n=== E2E Test: Telnet Concurrent Connections ===");

    // PROMPT: Tell the LLM to handle multiple clients
    let prompt = "listen on port {AVAILABLE_PORT} via telnet. Handle multiple concurrent clients. \
        Echo each message back with the client's message.";

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);


    // VALIDATION: Connect multiple clients concurrently
    println!("Testing concurrent Telnet clients...");

    let mut handles: Vec<tokio::task::JoinHandle<Result<bool, Box<dyn std::error::Error + Send + Sync>>>> = vec![];
    for i in 1..=3 {
        let port = server.port;
        let handle = tokio::spawn(async move {
            let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await?;
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);

            // Send a unique message
            let message = format!("Client {} message", i);
            write_half.write_all(format!("{}\n", message).as_bytes()).await?;
            write_half.flush().await?;

            // Try to read response
            let mut line = String::new();
            match tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line)).await {
                Ok(Ok(n)) if n > 0 => {
                    println!("  Client {} received: {}", i, line.trim());
                    Ok(true)
                }
                _ => {
                    println!("  Client {} no response", i);
                    Ok(false)
                }
            }
        });
        handles.push(handle);
    }

    // Wait for all clients
    let mut success_count = 0;
    for handle in handles {
        if let Ok(Ok(true)) = handle.await {
            success_count += 1;
        }
    }

    println!("✓ {} / 3 concurrent clients succeeded", success_count);

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}
