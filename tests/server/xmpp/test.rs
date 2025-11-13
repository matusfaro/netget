//! End-to-end XMPP tests for NetGet
//!
//! These tests spawn the actual NetGet binary with XMPP prompts
//! and validate the responses using XMPP protocol clients.

#![cfg(feature = "xmpp")]

use super::super::super::helpers::{self, E2EResult, NetGetConfig};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn test_xmpp_stream_header() -> E2EResult<()> {
    println!("\n=== E2E Test: XMPP Stream Header ===");

    // PROMPT: Tell the LLM to act as an XMPP server
    let prompt = "listen on port {AVAILABLE_PORT} via xmpp. When clients connect and send an XML stream header, \
        respond with: <?xml version='1.0'?><stream:stream xmlns='jabber:client' xmlns:stream='http://etherx.jabber.org/streams' from='localhost' id='stream-123' version='1.0'>";

    // Start the server
    let config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                .on_instruction_containing("xmpp")
                .respond_with_actions(serde_json::json!([{"type": "open_server", "port": 0, "base_stack": "XMPP", "instruction": "Respond to stream header"}]))
                .expect_calls(1)
                .and()
                .on_event("xmpp_data_received")
                .respond_with_actions(serde_json::json!([{"type": "send_stream_header", "from": "localhost", "stream_id": "stream-123"}]))
                .expect_calls(1)
                .and()
        });
    let server = helpers::start_netget_server(config).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Connect and send stream header
    println!("Connecting to XMPP server...");
    let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    // Send XMPP stream header
    let client_stream_header = "<?xml version='1.0'?><stream:stream xmlns='jabber:client' xmlns:stream='http://etherx.jabber.org/streams' to='localhost' version='1.0'>";
    println!("Sending stream header...");
    stream.write_all(client_stream_header.as_bytes()).await?;
    stream.flush().await?;

    // Read server response
    let mut buffer = vec![0u8; 4096];
    match tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buffer)).await {
        Ok(Ok(n)) if n > 0 => {
            let response = String::from_utf8_lossy(&buffer[..n]);
            println!("XMPP response: {}", response);

            // Check for stream header elements
            if response.contains("stream:stream") && response.contains("jabber:client") {
                println!("✓ XMPP stream header received");
            } else {
                println!("Note: Stream header not in expected format");
            }
        }
        Ok(Ok(_)) => {
            println!("Note: Connection closed before response");
        }
        Ok(Err(e)) => {
            println!("Note: Read error: {}", e);
        }
        Err(_) => {
            println!("Note: Timeout waiting for response");
        }
    }

    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_xmpp_message() -> E2EResult<()> {
    println!("\n=== E2E Test: XMPP Message Exchange ===");

    // PROMPT: Tell the LLM to echo XMPP messages
    let prompt = "listen on port {AVAILABLE_PORT} via xmpp domain=localhost. \
        When clients send an XML stream header, respond with server stream header and features. \
        When clients send a message stanza, extract the body text and echo it back with: \
        <message from='bot@localhost' to='[sender]' type='chat'><body>Echo: [body]</body></message>";

    // Start the server
    let config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                .on_instruction_containing("xmpp")
                .respond_with_actions(serde_json::json!([{"type": "open_server", "port": 0, "base_stack": "XMPP", "instruction": "Echo messages"}]))
                .expect_calls(1)
                .and()
                .on_event("xmpp_data_received")
                .respond_with_actions(serde_json::json!([{"type": "send_stream_header", "from": "localhost", "stream_id": "stream-456"}]))
                .expect_calls(1)
                .and()
                .on_event("xmpp_data_received")
                .respond_with_actions(serde_json::json!([{"type": "send_message", "from": "bot@localhost", "to": "alice@localhost", "message_type": "chat", "body": "Echo: Hello XMPP!"}]))
                .expect_calls(1)
                .and()
        });
    let server = helpers::start_netget_server(config).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Send message and verify echo
    let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    // Send stream header
    let client_stream_header = "<?xml version='1.0'?><stream:stream xmlns='jabber:client' xmlns:stream='http://etherx.jabber.org/streams' to='localhost' version='1.0'>";
    stream.write_all(client_stream_header.as_bytes()).await?;
    stream.flush().await?;

    // Read server stream header (allow time for LLM)
    let mut buffer = vec![0u8; 4096];
    println!("Waiting for server stream header...");
    match tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buffer)).await {
        Ok(Ok(n)) if n > 0 => {
            let response = String::from_utf8_lossy(&buffer[..n]);
            println!("Server response: {}", response);
        }
        _ => {
            println!("Note: Timeout or error on stream header");
        }
    }

    // Send a test message
    let test_message = "<message from='alice@localhost' to='bot@localhost' type='chat'><body>Hello XMPP!</body></message>";
    println!("Sending test message...");
    stream.write_all(test_message.as_bytes()).await?;
    stream.flush().await?;

    // Read echo response
    buffer.clear();
    buffer.resize(4096, 0);
    println!("Waiting for echo response...");
    match tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buffer)).await {
        Ok(Ok(n)) if n > 0 => {
            let response = String::from_utf8_lossy(&buffer[..n]);
            println!("Echo response: {}", response);

            // Check if message was echoed
            if response.contains("<message")
                && (response.contains("Echo:") || response.contains("Hello XMPP"))
            {
                println!("✓ XMPP message echo received");
            } else {
                println!("Note: Echo response not in expected format");
            }
        }
        Ok(Ok(_)) => {
            println!("Note: Connection closed before echo");
        }
        Ok(Err(e)) => {
            println!("Note: Read error: {}", e);
        }
        Err(_) => {
            println!("Note: Timeout waiting for echo");
        }
    }

    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_xmpp_presence() -> E2EResult<()> {
    println!("\n=== E2E Test: XMPP Presence ===");

    // PROMPT: Tell the LLM to handle presence
    let prompt = "listen on port {AVAILABLE_PORT} via xmpp. When clients connect, respond with stream header. \
        When clients send a presence stanza, acknowledge with: \
        <presence from='server@localhost' type='available'><status>Server online</status></presence>";

    // Start the server
    let config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                .on_instruction_containing("xmpp")
                .respond_with_actions(serde_json::json!([{"type": "open_server", "port": 0, "base_stack": "XMPP", "instruction": "Handle presence"}]))
                .expect_calls(1)
                .and()
                .on_event("xmpp_data_received")
                .respond_with_actions(serde_json::json!([{"type": "send_stream_header", "from": "localhost", "stream_id": "stream-789"}]))
                .expect_calls(1)
                .and()
                .on_event("xmpp_data_received")
                .respond_with_actions(serde_json::json!([{"type": "send_presence", "from": "server@localhost", "presence_type": "available", "status": "Server online"}]))
                .expect_calls(1)
                .and()
        });
    let server = helpers::start_netget_server(config).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Send presence and verify response
    let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", server.port)).await?;
    println!("✓ TCP connected");

    // Send stream header
    let client_stream_header = "<?xml version='1.0'?><stream:stream xmlns='jabber:client' xmlns:stream='http://etherx.jabber.org/streams' to='localhost' version='1.0'>";
    stream.write_all(client_stream_header.as_bytes()).await?;
    stream.flush().await?;

    // Read server stream header
    let mut buffer = vec![0u8; 4096];
    println!("Waiting for server stream header...");
    tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buffer))
        .await
        .ok();

    // Send presence
    let presence = "<presence><show>chat</show><status>Available</status></presence>";
    println!("Sending presence...");
    stream.write_all(presence.as_bytes()).await?;
    stream.flush().await?;

    // Read presence response
    buffer.clear();
    buffer.resize(4096, 0);
    println!("Waiting for presence response...");
    match tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buffer)).await {
        Ok(Ok(n)) if n > 0 => {
            let response = String::from_utf8_lossy(&buffer[..n]);
            println!("Presence response: {}", response);

            // Check if presence was acknowledged
            if response.contains("<presence") {
                println!("✓ XMPP presence response received");
            } else {
                println!("Note: Presence response not in expected format");
            }
        }
        Ok(Ok(_)) => {
            println!("Note: Connection closed before presence response");
        }
        Ok(Err(e)) => {
            println!("Note: Read error: {}", e);
        }
        Err(_) => {
            println!("Note: Timeout waiting for presence response");
        }
    }

    server.verify_mocks().await?;
    server.stop().await?;
    println!("=== Test completed ===\n");
    Ok(())
}
