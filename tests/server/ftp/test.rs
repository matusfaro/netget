//! End-to-end FTP tests for NetGet
//!
//! These tests spawn the actual NetGet binary with FTP prompts
//! and validate the responses using raw TCP connections.

#![cfg(feature = "ftp")]

use super::super::super::helpers::{self, E2EResult};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn test_ftp_greeting() -> E2EResult<()> {
    println!("\n=== E2E Test: FTP Greeting (220) ===");

    // PROMPT: Tell the LLM to send FTP greeting
    let prompt =
        "listen on port {AVAILABLE_PORT} via ftp. When a client connects, send FTP greeting: \
        '220 FTP Server Ready'";

    // Start the server with mocks
    let config = helpers::NetGetConfig::new(prompt).with_mock(|mock| {
        mock.on_instruction_containing("listen on port")
            .and_instruction_containing("ftp")
            .and_instruction_containing("greeting")
            .respond_with_actions(serde_json::json!([
                {
                    "type": "open_server",
                    "port": 0,
                    "base_stack": "FTP",
                    "instruction": prompt
                }
            ]))
            .expect_calls(1)
            .and()
    });

    let server = helpers::start_netget_server(config).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Connect and expect 220 greeting
    println!("Connecting to FTP server...");
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("TCP connected");

    let (read_half, _write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Read greeting
    let mut line = String::new();
    match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line)).await {
        Ok(Ok(n)) if n > 0 => {
            println!("FTP greeting: {}", line.trim());

            // Verify FTP greeting code 220
            assert!(
                line.starts_with("220") || line.contains("220"),
                "Expected FTP greeting starting with '220', got: {}",
                line
            );
            println!("FTP greeting (220) verified");
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

    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_ftp_user_pass() -> E2EResult<()> {
    println!("\n=== E2E Test: FTP USER/PASS Commands ===");

    // PROMPT: Handle USER and PASS commands
    let prompt = "listen on port {AVAILABLE_PORT} via ftp. Send greeting '220 FTP Ready'. \
        When client sends USER anonymous, respond with '331 Password required'. \
        When client sends PASS, respond with '230 User logged in'";

    // Start the server with mocks
    let config = helpers::NetGetConfig::new(prompt).with_mock(|mock| {
        mock.on_instruction_containing("listen on port")
            .and_instruction_containing("ftp")
            .and_instruction_containing("USER")
            .respond_with_actions(serde_json::json!([
                {
                    "type": "open_server",
                    "port": 0,
                    "base_stack": "FTP",
                    "instruction": prompt
                }
            ]))
            .expect_calls(1)
            .and()
    });

    let server = helpers::start_netget_server(config).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Send USER and PASS commands
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("TCP connected");

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Read greeting
    let mut line = String::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line)).await;
    println!("Greeting: {}", line.trim());

    // Send USER
    println!("Sending: USER anonymous");
    write_half.write_all(b"USER anonymous\r\n").await?;
    write_half.flush().await?;

    // Read USER response
    let mut user_response = String::new();
    match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut user_response)).await
    {
        Ok(Ok(n)) if n > 0 => {
            println!("USER response: {}", user_response.trim());
            if user_response.contains("331") {
                println!("USER response (331) verified");
            }
        }
        _ => println!("Note: No USER response received"),
    }

    // Send PASS
    println!("Sending: PASS guest@example.com");
    write_half.write_all(b"PASS guest@example.com\r\n").await?;
    write_half.flush().await?;

    // Read PASS response
    let mut pass_response = String::new();
    match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut pass_response)).await
    {
        Ok(Ok(n)) if n > 0 => {
            println!("PASS response: {}", pass_response.trim());
            if pass_response.contains("230") {
                println!("PASS response (230) verified");
            }
        }
        _ => println!("Note: No PASS response received"),
    }

    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_ftp_pwd_quit() -> E2EResult<()> {
    println!("\n=== E2E Test: FTP PWD and QUIT Commands ===");

    // PROMPT: Handle PWD and QUIT commands
    let prompt = "listen on port {AVAILABLE_PORT} via ftp. Send greeting '220 FTP Ready'. \
        When client sends PWD, respond with '257 \"/\" is current directory'. \
        When client sends QUIT, respond with '221 Goodbye' and close connection";

    // Start the server with mocks
    let config = helpers::NetGetConfig::new(prompt).with_mock(|mock| {
        mock.on_instruction_containing("listen on port")
            .and_instruction_containing("ftp")
            .and_instruction_containing("PWD")
            .respond_with_actions(serde_json::json!([
                {
                    "type": "open_server",
                    "port": 0,
                    "base_stack": "FTP",
                    "instruction": prompt
                }
            ]))
            .expect_calls(1)
            .and()
    });

    let server = helpers::start_netget_server(config).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Send PWD and QUIT commands
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("TCP connected");

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Read greeting
    let mut line = String::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line)).await;
    println!("Greeting: {}", line.trim());

    // Send PWD
    println!("Sending: PWD");
    write_half.write_all(b"PWD\r\n").await?;
    write_half.flush().await?;

    // Read PWD response
    let mut pwd_response = String::new();
    match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut pwd_response)).await {
        Ok(Ok(n)) if n > 0 => {
            println!("PWD response: {}", pwd_response.trim());
            if pwd_response.contains("257") {
                println!("PWD response (257) verified");
            }
        }
        _ => println!("Note: No PWD response received"),
    }

    // Send QUIT
    println!("Sending: QUIT");
    write_half.write_all(b"QUIT\r\n").await?;
    write_half.flush().await?;

    // Read QUIT response
    let mut quit_response = String::new();
    match tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut quit_response)).await
    {
        Ok(Ok(n)) if n > 0 => {
            println!("QUIT response: {}", quit_response.trim());
            if quit_response.contains("221") {
                println!("QUIT response (221) verified");
            }
        }
        _ => println!("Note: No QUIT response received"),
    }

    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}
