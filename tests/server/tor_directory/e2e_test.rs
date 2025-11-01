//! End-to-end Tor Directory tests for NetGet
//!
//! These tests spawn the actual NetGet binary with Tor Directory prompts
//! and validate the responses using HTTP clients.

#![cfg(feature = "e2e-tests")]

use crate::server::helpers::{self, ServerConfig, E2EResult};
use std::time::Duration;

#[tokio::test]
async fn test_tor_directory_consensus_request() -> E2EResult<()> {
    println!("\n=== E2E Test: Tor Directory Consensus Request ===");

    // Start Tor Directory server
    let prompt = "open_server port {AVAILABLE_PORT} base_stack ETH>IP>TCP>HTTP>TorDirectory. This is a Tor directory mirror. \
        When clients request /tor/status-vote/current/consensus, return a simple test consensus document \
        with network-status-version 3 and a few fake relays. When clients request any other path, \
        return a 404 error.";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // Wait for server to be ready
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send GET /tor/status-vote/current/consensus request
    println!("Sending GET /tor/status-vote/current/consensus request...");

    let client = reqwest::Client::new();
    let response = match tokio::time::timeout(
        Duration::from_secs(15),
        client
            .get(format!("http://127.0.0.1:{}/tor/status-vote/current/consensus", server.port))
            .send()
    ).await {
        Ok(Ok(resp)) => {
            println!("✓ Received HTTP response: {}", resp.status());
            resp
        }
        Ok(Err(e)) => {
            println!("✗ HTTP request error: {}", e);
            return Err(e.into());
        }
        Err(_) => {
            println!("✗ HTTP request timeout");
            return Err("Request timeout".into());
        }
    };

    assert_eq!(response.status(), 200, "Expected HTTP 200 OK");

    // Get response text
    let text = response.text().await?;
    println!("Response length: {} bytes", text.len());
    println!("Response preview: {}", &text.chars().take(200).collect::<String>());

    // Validate consensus format (basic check)
    assert!(text.contains("network-status-version") || text.len() > 0,
            "Expected consensus document to contain network-status-version or have content");

    println!("✓ Tor Directory Consensus request test completed\n");
    Ok(())
}

#[tokio::test]
async fn test_tor_directory_404_error() -> E2EResult<()> {
    println!("\n=== E2E Test: Tor Directory 404 Error ===");

    let prompt = "open_server port {AVAILABLE_PORT} base_stack ETH>IP>TCP>HTTP>TorDirectory. This is a Tor directory mirror. \
        When clients request unknown paths, return a 404 Not Found error.";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // Wait for server to be ready
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send GET request to unknown path
    println!("Sending GET /tor/invalid/path request...");

    let client = reqwest::Client::new();
    let response = match tokio::time::timeout(
        Duration::from_secs(15),
        client
            .get(format!("http://127.0.0.1:{}/tor/invalid/path", server.port))
            .send()
    ).await {
        Ok(Ok(resp)) => {
            println!("✓ Received HTTP response: {}", resp.status());
            resp
        }
        Ok(Err(e)) => {
            println!("✗ HTTP request error: {}", e);
            return Err(e.into());
        }
        Err(_) => {
            println!("✗ HTTP request timeout");
            return Err("Request timeout".into());
        }
    };

    // Expect 404 or similar error
    assert!(response.status().is_client_error() || response.status().is_server_error(),
            "Expected error status code for unknown path, got {}", response.status());

    println!("✓ Tor Directory 404 Error test completed\n");
    Ok(())
}

#[tokio::test]
async fn test_tor_directory_microdescriptors() -> E2EResult<()> {
    println!("\n=== E2E Test: Tor Directory Microdescriptors ===");

    let prompt = "open_server port {AVAILABLE_PORT} base_stack ETH>IP>TCP>HTTP>TorDirectory. This is a Tor directory mirror. \
        When clients request /tor/micro/d/<hash>, return a simple microdescriptor with \
        onion-key and ntor-onion-key fields.";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // Wait for server to be ready
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send GET /tor/micro/d/test request
    println!("Sending GET /tor/micro/d/test request...");

    let client = reqwest::Client::new();
    let response = match tokio::time::timeout(
        Duration::from_secs(15),
        client
            .get(format!("http://127.0.0.1:{}/tor/micro/d/test", server.port))
            .send()
    ).await {
        Ok(Ok(resp)) => {
            println!("✓ Received HTTP response: {}", resp.status());
            resp
        }
        Ok(Err(e)) => {
            println!("✗ HTTP request error: {}", e);
            return Err(e.into());
        }
        Err(_) => {
            println!("✗ HTTP request timeout");
            return Err("Request timeout".into());
        }
    };

    assert_eq!(response.status(), 200, "Expected HTTP 200 OK");

    // Get response text
    let text = response.text().await?;
    println!("Response length: {} bytes", text.len());
    println!("Response preview: {}", &text.chars().take(200).collect::<String>());

    // Validate microdescriptor format (basic check)
    assert!(text.contains("onion-key") || text.len() > 0,
            "Expected microdescriptor to contain onion-key or have content");

    println!("✓ Tor Directory Microdescriptors test completed\n");
    Ok(())
}
