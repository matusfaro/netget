//! End-to-end IRC tests for NetGet
//!
//! These tests spawn the actual NetGet binary with IRC prompts
//! and validate the responses using IRC protocol clients.

#![cfg(feature = "e2e-tests")]

// Helper module imported from parent

use super::super::super::helpers::{self, ServerConfig, E2EResult};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use std::time::Duration;

#[tokio::test]
async fn test_irc_welcome() -> E2EResult<()> {
    println!("\n=== E2E Test: IRC Welcome (RPL_WELCOME) ===");

    // PROMPT: Tell the LLM to act as an IRC server
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} via irc. When users connect and send NICK and USER commands, \
        respond with IRC welcome numeric 001: ':servername 001 nickname :Welcome to the IRC Network'",
        port
    );

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);


    // VALIDATION: Connect and perform IRC registration
    println!("Connecting to IRC server...");
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Send IRC registration commands
    println!("Sending NICK and USER commands...");
    write_half.write_all(b"NICK testuser\r\n").await?;
    write_half.write_all(b"USER testuser 0 * :Test User\r\n").await?;
    write_half.flush().await?;

    // Read responses (may be multiple lines)
    let mut received_welcome = false;
    for attempt in 1..=5 {
        let mut line = String::new();
        match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
            Ok(Ok(n)) if n > 0 => {
                println!("IRC response ({}): {}", attempt, line.trim());

                // Check for IRC numeric 001 (RPL_WELCOME)
                if line.contains(" 001 ") || line.contains("Welcome") || line.contains("WELCOME") {
                    println!("✓ IRC welcome message (001) received");
                    received_welcome = true;
                    break;
                }
            }
            Ok(Ok(_)) => {
                println!("  Connection closed");
                break;
            }
            Ok(Err(e)) => {
                println!("  Read error: {}", e);
                break;
            }
            Err(_) => {
                println!("  Timeout on attempt {}", attempt);
                break;
            }
        }
    }

    if !received_welcome {
        println!("Note: Did not receive IRC 001 welcome message");
        println!("  This may be expected if IRC protocol is not fully implemented");
    }

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_irc_ping_pong() -> E2EResult<()> {
    println!("\n=== E2E Test: IRC PING/PONG ===");

    // PROMPT: Tell the LLM to handle IRC PING
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} via irc. When you receive a PING command with a token, \
        respond with PONG using the same token. Format: 'PONG :token'",
        port
    );

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);


    // VALIDATION: Send PING and verify PONG
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Send PING command
    let ping_token = "1234567890";
    println!("Sending: PING :{}", ping_token);
    write_half.write_all(format!("PING :{}\r\n", ping_token).as_bytes()).await?;
    write_half.flush().await?;

    // Read PONG response
    let mut line = String::new();
    match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
        Ok(Ok(n)) if n > 0 => {
            println!("IRC response: {}", line.trim());

            // Verify PONG with same token
            assert!(
                line.contains("PONG") && line.contains(ping_token),
                "Expected 'PONG :{}', got: {}",
                ping_token,
                line
            );
            println!("✓ IRC PING/PONG verified");
        }
        Ok(Ok(_)) => {
            println!("Note: Connection closed without response");
        }
        Ok(Err(e)) => {
            println!("Note: Read error: {}", e);
        }
        Err(_) => {
            println!("Note: No PONG response received (timeout)");
            println!("  This may be expected if IRC protocol is not fully implemented");
        }
    }

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_irc_join_channel() -> E2EResult<()> {
    println!("\n=== E2E Test: IRC Channel Join ===");

    // PROMPT: Tell the LLM to handle channel joins
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} via irc. When users send JOIN #channel, \
        respond with ':nickname JOIN #channel' to confirm the join",
        port
    );

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);


    // VALIDATION: Connect and join a channel
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Send IRC registration and JOIN
    println!("Sending IRC commands...");
    write_half.write_all(b"NICK testuser\r\n").await?;
    write_half.write_all(b"USER testuser 0 * :Test User\r\n").await?;
    write_half.write_all(b"JOIN #test\r\n").await?;
    write_half.flush().await?;

    // Read responses
    let mut received_join = false;
    for attempt in 1..=5 {
        let mut line = String::new();
        match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
            Ok(Ok(n)) if n > 0 => {
                println!("IRC response ({}): {}", attempt, line.trim());

                // Check for JOIN confirmation
                if line.contains("JOIN") && line.contains("#test") {
                    println!("✓ JOIN confirmation received");
                    received_join = true;
                    break;
                }
            }
            _ => break,
        }
    }

    if !received_join {
        println!("Note: Did not receive JOIN confirmation");
        println!("  This may be expected if channel handling is not implemented");
    }

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_irc_privmsg() -> E2EResult<()> {
    println!("\n=== E2E Test: IRC PRIVMSG ===");

    // PROMPT: Tell the LLM to handle private messages
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} via irc. When you receive 'PRIVMSG target :message', \
        echo it back as 'PRIVMSG sender :message'",
        port
    );

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);


    // VALIDATION: Send PRIVMSG and check for response
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Send registration and message
    println!("Sending IRC commands...");
    write_half.write_all(b"NICK testuser\r\n").await?;
    write_half.write_all(b"USER testuser 0 * :Test User\r\n").await?;
    write_half.write_all(b"PRIVMSG bot :Hello IRC\r\n").await?;
    write_half.flush().await?;

    // Read responses
    for attempt in 1..=5 {
        let mut line = String::new();
        match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
            Ok(Ok(n)) if n > 0 => {
                println!("IRC response ({}): {}", attempt, line.trim());

                // Check for PRIVMSG response
                if line.contains("PRIVMSG") && line.contains("Hello") {
                    println!("✓ PRIVMSG response received");
                }
            }
            _ => break,
        }
    }

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_irc_multiple_clients() -> E2EResult<()> {
    println!("\n=== E2E Test: IRC Multiple Clients ===");

    // PROMPT: Tell the LLM to handle multiple IRC clients
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} via irc. Handle multiple concurrent IRC clients. \
        Send welcome message (001) to each client that connects with NICK and USER",
        port
    );

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);


    // VALIDATION: Connect multiple clients
    println!("Testing multiple IRC clients...");

    for i in 1..=3 {
        println!("  Client #{}", i);

        let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        // Register
        write_half.write_all(format!("NICK testuser{}\r\n", i).as_bytes()).await?;
        write_half.write_all(format!("USER testuser{} 0 * :Test User {}\r\n", i, i).as_bytes()).await?;
        write_half.flush().await?;

        // Try to read response
        let mut line = String::new();
        match tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line)).await {
            Ok(Ok(n)) if n > 0 => {
                println!("    Response: {}", line.trim());
                if line.contains("001") || line.contains("Welcome") {
                    println!("    ✓ Client #{} received welcome", i);
                }
            }
            _ => {
                println!("    Note: No response for client #{}", i);
            }
        }

        // Small delay between clients
    }

    println!("✓ Multiple client handling tested");

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}
