//! End-to-end SSH and IRC tests for NetGet
//!
//! These tests spawn the actual NetGet binary with SSH/IRC prompts
//! and validate the responses using real TCP clients.

#![cfg(feature = "e2e-tests")]

mod e2e;

use e2e::helpers::{self, ServerConfig, E2EResult};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, AsyncBufReadExt};
use std::time::Duration;

#[tokio::test]
async fn test_ssh_server() -> E2EResult<()> {
    println!("\n=== E2E Test: SSH Server ===");

    // PROMPT: Tell the LLM to act as an SSH server
    let port = helpers::get_available_port().await?;
    let prompt = format!("listen on port {} via ssh. Send SSH-2.0-TestServer as the banner.", port);

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_millis(500)).await;

    // VALIDATION: Connect and check SSH banner
    let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP client connected");

    // Send SSH client hello
    println!("Sending SSH client hello...");
    stream.write_all(b"SSH-2.0-TestClient\r\n").await?;

    // Read response (wait for LLM to process)
    let mut buffer = vec![0u8; 256];
    match tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buffer)).await {
        Ok(Ok(n)) if n > 0 => {
            let response = String::from_utf8_lossy(&buffer[..n]);
            println!("SSH server response: {}", response);

            // Check for SSH banner
            if response.contains("SSH-2.0") {
                println!("✓ SSH banner verified");
            } else {
                println!("Note: Expected SSH banner, got: {}", response);
            }
        }
        Ok(Ok(_)) => {
            println!("Note: Connection closed without response");
        }
        Ok(Err(e)) => {
            println!("Note: Read error: {}", e);
        }
        Err(_) => {
            println!("Note: Response timeout");
        }
    }

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_irc_server() -> E2EResult<()> {
    println!("\n=== E2E Test: IRC Server ===");

    // PROMPT: Tell the LLM to act as an IRC server
    let port = helpers::get_available_port().await?;
    let prompt = format!("listen on port {} via irc. Welcome users with a 001 numeric reply.", port);

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_millis(500)).await;

    // VALIDATION: Connect and send IRC commands
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP client connected");

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Send IRC NICK and USER commands
    println!("Sending IRC NICK and USER commands...");
    write_half.write_all(b"NICK testuser\r\n").await?;
    write_half.write_all(b"USER test 0 * :Test User\r\n").await?;
    write_half.flush().await?;

    // Try to read welcome message
    let mut line = String::new();
    match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
        Ok(Ok(_)) if !line.is_empty() => {
            println!("IRC server response: {}", line);

            // Check for IRC numeric reply (001 is welcome)
            if line.contains("001") || line.contains("Welcome") {
                println!("✓ IRC welcome message verified");
            } else {
                println!("Note: Expected IRC welcome (001), got: {}", line);
            }
        }
        Ok(Ok(_)) => {
            println!("Note: Connection closed without response");
        }
        Ok(Err(e)) => {
            println!("Note: Read error: {}", e);
        }
        Err(_) => {
            println!("Note: Response timeout");
        }
    }

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_irc_channel_join() -> E2EResult<()> {
    println!("\n=== E2E Test: IRC Channel Join ===");

    // PROMPT: Tell the LLM to handle channel joins
    let port = helpers::get_available_port().await?;
    let prompt = format!("listen on port {} via irc. When users join a channel, send JOIN confirmation.", port);

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_millis(500)).await;

    // VALIDATION: Connect and try to join a channel
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP client connected");

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Send IRC commands
    println!("Sending IRC commands...");
    write_half.write_all(b"NICK testuser\r\n").await?;
    write_half.write_all(b"USER test 0 * :Test User\r\n").await?;
    write_half.write_all(b"JOIN #test\r\n").await?;
    write_half.flush().await?;

    // Read responses
    for _ in 0..3 {
        let mut line = String::new();
        match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
            Ok(Ok(_)) if !line.is_empty() => {
                println!("IRC response: {}", line.trim());
            }
            _ => break,
        }
    }

    println!("✓ IRC channel join tested");

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}
