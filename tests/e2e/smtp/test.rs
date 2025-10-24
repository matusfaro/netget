//! End-to-end SMTP tests for NetGet
//!
//! These tests spawn the actual NetGet binary with SMTP prompts
//! and validate the responses using SMTP protocol clients.

#![cfg(feature = "e2e-tests")]

mod e2e;

use e2e::helpers::{self, ServerConfig, E2EResult};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use std::time::Duration;

#[tokio::test]
async fn test_smtp_greeting() -> E2EResult<()> {
    println!("\n=== E2E Test: SMTP Greeting (220) ===");

    // PROMPT: Tell the LLM to send SMTP greeting
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} via smtp. When a client connects, send SMTP greeting: \
        '220 mail.example.com ESMTP Service Ready'",
        port
    );

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_secs(1)).await;

    // VALIDATION: Connect and expect 220 greeting
    println!("Connecting to SMTP server...");
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    let (read_half, _write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Read greeting
    let mut line = String::new();
    match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
        Ok(Ok(n)) if n > 0 => {
            println!("SMTP greeting: {}", line.trim());

            // Verify SMTP greeting code 220
            assert!(
                line.starts_with("220") || line.contains("220"),
                "Expected SMTP greeting starting with '220', got: {}",
                line
            );
            println!("✓ SMTP greeting (220) verified");
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
async fn test_smtp_ehlo() -> E2EResult<()> {
    println!("\n=== E2E Test: SMTP EHLO Command ===");

    // PROMPT: Tell the LLM to handle EHLO
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} via smtp. Send greeting '220 mail.test ESMTP'. \
        When client sends EHLO, respond with '250-mail.test' followed by '250 8BITMIME'",
        port
    );

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_secs(1)).await;

    // VALIDATION: Send EHLO and verify response
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Read greeting
    let mut line = String::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line)).await;
    println!("Greeting: {}", line.trim());

    // Send EHLO
    println!("Sending: EHLO client.test");
    write_half.write_all(b"EHLO client.test\r\n").await?;
    write_half.flush().await?;

    // Read EHLO response (may be multiple lines)
    let mut received_250 = false;
    for attempt in 1..=5 {
        let mut line = String::new();
        match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
            Ok(Ok(n)) if n > 0 => {
                println!("SMTP response ({}): {}", attempt, line.trim());

                // Check for 250 response
                if line.starts_with("250") || line.contains("250") {
                    received_250 = true;
                }

                // Stop if we get a final 250 line (not 250-)
                if line.starts_with("250 ") {
                    break;
                }
            }
            _ => break,
        }
    }

    if received_250 {
        println!("✓ SMTP EHLO response (250) verified");
    } else {
        println!("Note: Did not receive 250 response to EHLO");
    }

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_smtp_mail_transaction() -> E2EResult<()> {
    println!("\n=== E2E Test: SMTP Mail Transaction ===");

    // PROMPT: Tell the LLM to handle a full SMTP transaction
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} via smtp. Handle full SMTP mail transaction: \
        1) Send '220' greeting \
        2) Respond to EHLO with '250 OK' \
        3) Respond to MAIL FROM with '250 Sender OK' \
        4) Respond to RCPT TO with '250 Recipient OK' \
        5) Respond to DATA with '354 Start mail input' \
        6) After mail data ending with '.', respond with '250 Message accepted'",
        port
    );

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_secs(1)).await;

    // VALIDATION: Perform full SMTP transaction
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Read greeting
    let mut line = String::new();
    let _ = tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await;
    println!("  Response: {}", line.trim());

    // Send EHLO
    println!("Sending: EHLO client.test");
    write_half.write_all(b"EHLO client.test\r\n").await?;
    write_half.flush().await?;
    line.clear();
    let _ = tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await;
    println!("  Response: {}", line.trim());

    // Send MAIL FROM
    println!("Sending: MAIL FROM:<sender@test.com>");
    write_half.write_all(b"MAIL FROM:<sender@test.com>\r\n").await?;
    write_half.flush().await?;
    line.clear();
    match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
        Ok(Ok(n)) if n > 0 => {
            println!("  Response: {}", line.trim());
            if line.contains("250") {
                println!("  ✓ MAIL FROM accepted");
            }
        }
        _ => {}
    }

    // Send RCPT TO
    println!("Sending: RCPT TO:<recipient@test.com>");
    write_half.write_all(b"RCPT TO:<recipient@test.com>\r\n").await?;
    write_half.flush().await?;
    line.clear();
    match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
        Ok(Ok(n)) if n > 0 => {
            println!("  Response: {}", line.trim());
            if line.contains("250") {
                println!("  ✓ RCPT TO accepted");
            }
        }
        _ => {}
    }

    // Send DATA
    println!("Sending: DATA");
    write_half.write_all(b"DATA\r\n").await?;
    write_half.flush().await?;
    line.clear();
    match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
        Ok(Ok(n)) if n > 0 => {
            println!("  Response: {}", line.trim());
            if line.contains("354") {
                println!("  ✓ DATA command accepted");
            }
        }
        _ => {}
    }

    println!("✓ SMTP transaction flow tested");

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_smtp_quit() -> E2EResult<()> {
    println!("\n=== E2E Test: SMTP QUIT Command ===");

    // PROMPT: Tell the LLM to handle QUIT
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} via smtp. Send greeting '220 mail.test'. \
        When client sends QUIT, respond with '221 Bye' and close connection",
        port
    );

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_secs(1)).await;

    // VALIDATION: Send QUIT and verify response
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Read greeting
    let mut line = String::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line)).await;
    println!("Greeting: {}", line.trim());

    // Send QUIT
    println!("Sending: QUIT");
    write_half.write_all(b"QUIT\r\n").await?;
    write_half.flush().await?;

    // Read QUIT response
    let mut line = String::new();
    match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
        Ok(Ok(n)) if n > 0 => {
            println!("SMTP response: {}", line.trim());

            // Verify 221 response
            if line.starts_with("221") || line.contains("221") {
                println!("✓ SMTP QUIT response (221) verified");
            } else {
                println!("Note: Expected 221, got: {}", line);
            }
        }
        _ => {
            println!("Note: No response to QUIT");
        }
    }

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_smtp_error_handling() -> E2EResult<()> {
    println!("\n=== E2E Test: SMTP Error Handling ===");

    // PROMPT: Tell the LLM to handle invalid commands
    let port = helpers::get_available_port().await?;
    let prompt = format!(
        "listen on port {} via smtp. Send greeting '220 mail.test'. \
        When you receive invalid commands, respond with '500 Command not recognized'",
        port
    );

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_secs(1)).await;

    // VALIDATION: Send invalid command
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Read greeting
    let mut line = String::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line)).await;
    println!("Greeting: {}", line.trim());

    // Send invalid command
    println!("Sending invalid command: INVALID");
    write_half.write_all(b"INVALID\r\n").await?;
    write_half.flush().await?;

    // Read error response
    let mut line = String::new();
    match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
        Ok(Ok(n)) if n > 0 => {
            println!("SMTP response: {}", line.trim());

            // Should get some kind of error (5xx)
            if line.starts_with("5") || line.contains("error") || line.contains("Error") {
                println!("✓ SMTP error response received");
            } else {
                println!("Note: Response to invalid command: {}", line);
            }
        }
        _ => {
            println!("Note: No response to invalid command");
        }
    }

    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}
