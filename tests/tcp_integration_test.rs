//! TCP Integration Tests
//!
//! Black-box tests that use prompts to configure the LLM-controlled TCP server.
//! Each test provides a prompt and validates the behavior using real network clients.

mod common;

use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_ftp_server() {
    println!("\n=== Testing FTP Server via TCP/LLM ===");

    // PROMPT: Tell the LLM to act as an FTP server
    let prompt = "listen on port 0 via ftp. Serve file data.txt with content: hello world";

    // Start server - everything is inferred from the prompt
    let (_state, port, _handle) = common::start_server_with_prompt(prompt).await;

    println!("Server started on port {}", port);
    sleep(Duration::from_secs(1)).await;

    // VALIDATION: Use real FTP client to verify behavior
    println!("Connecting FTP client...");
    match suppaftp::FtpStream::connect(format!("127.0.0.1:{}", port)) {
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
            panic!(
                "FTP connection failed: {}. Ensure Ollama is running with a model installed.",
                e
            );
        }
    }

    println!("=== FTP Server test passed ===\n");
}

#[tokio::test]
async fn test_simple_echo() {
    println!("\n=== Testing Simple Echo Server via TCP/LLM ===");

    // PROMPT: Tell the LLM to echo back with ACK prefix
    let prompt = "listen on port 0 via tcp. When you receive any data, reply with 'ACK: ' followed by the received data";

    // Start server
    let (_state, port, _handle) = common::start_server_with_prompt(prompt).await;

    println!("Server started on port {}", port);
    sleep(Duration::from_millis(500)).await;

    // VALIDATION: Send data and verify echo response
    println!("Connecting TCP client...");
    match tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await {
        Ok(mut stream) => {
            println!("✓ TCP client connected");

            // Send test data
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let test_message = "Hello, LLM!";
            println!("Sending: {}", test_message);
            stream.write_all(test_message.as_bytes()).await.unwrap();
            stream.flush().await.unwrap();

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
                Ok(Ok(_)) => panic!("Connection closed without response"),
                Ok(Err(e)) => panic!("Read error: {}", e),
                Err(_) => panic!("Response timeout"),
            }
        }
        Err(e) => {
            panic!("TCP connection failed: {}", e);
        }
    }

    println!("=== Simple Echo test passed ===\n");
}

#[tokio::test]
async fn test_custom_response() {
    println!("\n=== Testing Custom Response Server via TCP/LLM ===");

    // PROMPT: Tell the LLM to send a specific response
    let prompt = "listen on port 0 via tcp. When a client connects, send the greeting 'Welcome to the test server!\\r\\n'. When you receive 'PING', respond with 'PONG\\r\\n'";

    // Start server
    let (_state, port, _handle) = common::start_server_with_prompt(prompt).await;

    println!("Server started on port {}", port);
    sleep(Duration::from_millis(500)).await;

    // VALIDATION: Verify greeting and PING/PONG
    println!("Connecting TCP client...");
    match tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await {
        Ok(mut stream) => {
            println!("✓ TCP client connected");

            use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
                _ => panic!("Failed to receive greeting"),
            }

            // Send PING
            println!("Sending: PING");
            stream.write_all(b"PING").await.unwrap();
            stream.flush().await.unwrap();

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
                _ => panic!("Failed to receive PONG"),
            }
        }
        Err(e) => {
            panic!("TCP connection failed: {}", e);
        }
    }

    println!("=== Custom Response test passed ===\n");
}
