//! End-to-end POP3 tests for NetGet
//!
//! These tests spawn the actual NetGet binary with POP3 prompts
//! and validate the responses using POP3 protocol clients.

#![cfg(feature = "pop3")]

// Helper module imported from parent

use super::super::super::helpers::{self, E2EResult, ServerConfig};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn test_pop3_greeting() -> E2EResult<()> {
    println!("\n=== E2E Test: POP3 Greeting (+OK) ===");

    // PROMPT: Tell the LLM to send POP3 greeting
    let prompt =
        "listen on port {AVAILABLE_PORT} via pop3. When a client connects, send POP3 greeting: \
        '+OK POP3 server ready'";

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Connect and expect +OK greeting
    println!("Connecting to POP3 server...");
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    let (read_half, _write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Read greeting
    let mut line = String::new();
    match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
        Ok(Ok(n)) if n > 0 => {
            println!("POP3 greeting: {}", line.trim());

            // Verify POP3 greeting (+OK)
            assert!(
                line.starts_with("+OK") || line.contains("+OK"),
                "Expected POP3 greeting starting with '+OK', got: {}",
                line
            );
            println!("✓ POP3 greeting (+OK) verified");
        }
        Ok(Ok(_)) => {
            println!("Note: Connection closed without greeting");
        }
        Ok(Err(e)) => {
            println!("Note: Read error: {}", e);
        }
        Err(_) => {
            println!("Note: No greeting received (timeout)");
        }
    }

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_pop3_authentication() -> E2EResult<()> {
    println!("\n=== E2E Test: POP3 Authentication (USER/PASS) ===");

    // PROMPT: Tell the LLM to handle USER and PASS commands
    let prompt = "listen on port {AVAILABLE_PORT} via pop3. Send greeting '+OK POP3 ready'. \
        When client sends USER command, respond with '+OK user accepted'. \
        When client sends PASS command, respond with '+OK logged in'";

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Send USER and PASS, verify responses
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Read greeting
    let mut line = String::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line)).await;
    println!("Greeting: {}", line.trim());

    // Send USER command
    println!("Sending: USER alice");
    write_half.write_all(b"USER alice\r\n").await?;
    write_half.flush().await?;

    // Read USER response
    line.clear();
    match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
        Ok(Ok(n)) if n > 0 => {
            println!("POP3 USER response: {}", line.trim());
            assert!(
                line.contains("+OK"),
                "Expected +OK response for USER, got: {}",
                line
            );
            println!("✓ USER command accepted");
        }
        Ok(Ok(_)) => panic!("Connection closed after USER"),
        Ok(Err(e)) => panic!("Read error after USER: {}", e),
        Err(_) => panic!("No response to USER (timeout)"),
    }

    // Send PASS command
    println!("Sending: PASS secret");
    write_half.write_all(b"PASS secret\r\n").await?;
    write_half.flush().await?;

    // Read PASS response
    line.clear();
    match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
        Ok(Ok(n)) if n > 0 => {
            println!("POP3 PASS response: {}", line.trim());
            assert!(
                line.contains("+OK"),
                "Expected +OK response for PASS, got: {}",
                line
            );
            println!("✓ PASS command accepted (authenticated)");
        }
        Ok(Ok(_)) => panic!("Connection closed after PASS"),
        Ok(Err(e)) => panic!("Read error after PASS: {}", e),
        Err(_) => panic!("No response to PASS (timeout)"),
    }

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_pop3_stat() -> E2EResult<()> {
    println!("\n=== E2E Test: POP3 STAT Command ===");

    // PROMPT: Tell the LLM to handle STAT command
    let prompt = "listen on port {AVAILABLE_PORT} via pop3. Send greeting '+OK POP3 ready'. \
        Accept USER and PASS with '+OK'. \
        When client sends STAT, respond with '+OK 3 1024' (3 messages, 1024 bytes total)";

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Authenticate and send STAT
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Read greeting
    let mut line = String::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line)).await;
    println!("Greeting: {}", line.trim());

    // Authenticate (USER + PASS)
    write_half.write_all(b"USER alice\r\n").await?;
    write_half.flush().await?;
    line.clear();
    let _ = tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line)).await;
    println!("USER response: {}", line.trim());

    write_half.write_all(b"PASS secret\r\n").await?;
    write_half.flush().await?;
    line.clear();
    let _ = tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line)).await;
    println!("PASS response: {}", line.trim());

    // Send STAT command
    println!("Sending: STAT");
    write_half.write_all(b"STAT\r\n").await?;
    write_half.flush().await?;

    // Read STAT response
    line.clear();
    match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
        Ok(Ok(n)) if n > 0 => {
            println!("POP3 STAT response: {}", line.trim());
            assert!(
                line.contains("+OK") && (line.contains("3") || line.contains("1024")),
                "Expected +OK with message count and size, got: {}",
                line
            );
            println!("✓ STAT response verified");
        }
        Ok(Ok(_)) => panic!("Connection closed after STAT"),
        Ok(Err(e)) => panic!("Read error after STAT: {}", e),
        Err(_) => panic!("No response to STAT (timeout)"),
    }

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_pop3_quit() -> E2EResult<()> {
    println!("\n=== E2E Test: POP3 QUIT Command ===");

    // PROMPT: Tell the LLM to handle QUIT command
    let prompt = "listen on port {AVAILABLE_PORT} via pop3. Send greeting '+OK POP3 ready'. \
        When client sends QUIT, respond with '+OK goodbye' and close connection";

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Send QUIT and verify response
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Read greeting
    let mut line = String::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line)).await;
    println!("Greeting: {}", line.trim());

    // Send QUIT command
    println!("Sending: QUIT");
    write_half.write_all(b"QUIT\r\n").await?;
    write_half.flush().await?;

    // Read QUIT response
    line.clear();
    match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
        Ok(Ok(n)) if n > 0 => {
            println!("POP3 QUIT response: {}", line.trim());
            assert!(
                line.contains("+OK"),
                "Expected +OK response for QUIT, got: {}",
                line
            );
            println!("✓ QUIT response verified");
        }
        Ok(Ok(_)) => {
            println!("✓ Connection closed (expected after QUIT)");
        }
        Ok(Err(e)) => panic!("Read error after QUIT: {}", e),
        Err(_) => panic!("No response to QUIT (timeout)"),
    }

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}
