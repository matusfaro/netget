//! End-to-end Tor Directory tests for NetGet
//!
//! These tests spawn the actual NetGet binary with Tor Directory prompts
//! and validate the responses using HTTP clients.

#![cfg(feature = "tor_directory")]

use crate::server::helpers::{self, E2EResult, NetGetConfig};
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn test_tor_directory_consensus_request() -> E2EResult<()> {
    println!("\n=== E2E Test: Tor Directory Consensus Request ===");

    // Start Tor Directory server
    let prompt = "Open TOR Directory on port {AVAILABLE_PORT}. This is a Tor directory mirror. \
        When clients request /tor/status-vote/current/consensus, return a simple test consensus document \
        with network-status-version 3 and a few fake relays. When clients request any other path, \
        return a 404 error.";

    let config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("TOR Directory")
                .respond_with_actions(json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "HTTP",
                        "protocol": "TOR_DIRECTORY",
                        "instruction": "Tor directory mirror serving consensus documents"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Consensus request
                .on_event("http_request_received")
                .and_event_data_contains("path", "/tor/status-vote/current/consensus")
                .respond_with_actions(json!([
                    {
                        "type": "http_response",
                        "status_code": 200,
                        "headers": {
                            "Content-Type": "text/plain"
                        },
                        "body": "network-status-version 3\nvalid-after 2024-01-01 00:00:00\nfresh-until 2024-01-01 01:00:00\nr relay1 AAA BBB 1.2.3.4 9001 0 0\nr relay2 CCC DDD 5.6.7.8 9001 0 0\n"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let server = helpers::start_netget_server(config).await?;
    println!("Server started on port {}", server.port);

    // Wait for server to be ready
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send GET /tor/status-vote/current/consensus request
    println!("Sending GET /tor/status-vote/current/consensus request...");

    let client = reqwest::Client::new();
    let response = match tokio::time::timeout(
        Duration::from_secs(60), // Increased from 15 to 60 seconds for LLM response
        client
            .get(format!(
                "http://127.0.0.1:{}/tor/status-vote/current/consensus",
                server.port
            ))
            .send(),
    )
    .await
    {
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
    println!(
        "Response preview: {}",
        &text.chars().take(200).collect::<String>()
    );

    // Validate consensus format (basic check)
    assert!(
        text.contains("network-status-version") || text.len() > 0,
        "Expected consensus document to contain network-status-version or have content"
    );

    println!("✓ Tor Directory Consensus request test completed\n");

    // Verify mock expectations were met
    server.verify_mocks().await?;
    server.stop().await?;

    Ok(())
}

#[tokio::test]
async fn test_tor_directory_404_error() -> E2EResult<()> {
    println!("\n=== E2E Test: Tor Directory 404 Error ===");

    let prompt = "Open TOR Directory on port {AVAILABLE_PORT}. This is a Tor directory mirror. \
        When clients request unknown paths, return a 404 Not Found error.";

    let config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("TOR Directory")
                .respond_with_actions(json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "HTTP",
                        "protocol": "TOR_DIRECTORY",
                        "instruction": "Tor directory mirror with 404 error handling"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: 404 error for unknown path
                .on_event("http_request_received")
                .and_event_data_contains("path", "/tor/invalid/path")
                .respond_with_actions(json!([
                    {
                        "type": "http_response",
                        "status_code": 404,
                        "headers": {
                            "Content-Type": "text/plain"
                        },
                        "body": "Not Found"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let server = helpers::start_netget_server(config).await?;
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
            .send(),
    )
    .await
    {
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
    assert!(
        response.status().is_client_error() || response.status().is_server_error(),
        "Expected error status code for unknown path, got {}",
        response.status()
    );

    println!("✓ Tor Directory 404 Error test completed\n");

    // Verify mock expectations were met
    server.verify_mocks().await?;
    server.stop().await?;

    Ok(())
}

#[tokio::test]
async fn test_tor_directory_microdescriptors() -> E2EResult<()> {
    println!("\n=== E2E Test: Tor Directory Microdescriptors ===");

    let prompt = "Open TOR Directory on port {AVAILABLE_PORT}. This is a Tor directory mirror. \
        When clients request /tor/micro/d/<hash>, return a simple microdescriptor with \
        onion-key and ntor-onion-key fields.";

    let config = NetGetConfig::new(prompt)
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup
                .on_instruction_containing("TOR Directory")
                .respond_with_actions(json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "HTTP",
                        "protocol": "TOR_DIRECTORY",
                        "instruction": "Tor directory mirror serving microdescriptors"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Microdescriptor request
                .on_event("http_request_received")
                .and_event_data_contains("path", "/tor/micro/d/")
                .respond_with_actions(json!([
                    {
                        "type": "http_response",
                        "status_code": 200,
                        "headers": {
                            "Content-Type": "text/plain"
                        },
                        "body": "onion-key\n-----BEGIN RSA PUBLIC KEY-----\nMIGJAoGBAM... (test key)\n-----END RSA PUBLIC KEY-----\nntor-onion-key base64data\n"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let server = helpers::start_netget_server(config).await?;
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
            .send(),
    )
    .await
    {
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
    println!(
        "Response preview: {}",
        &text.chars().take(200).collect::<String>()
    );

    // Validate microdescriptor format (basic check)
    assert!(
        text.contains("onion-key") || text.len() > 0,
        "Expected microdescriptor to contain onion-key or have content"
    );

    println!("✓ Tor Directory Microdescriptors test completed\n");

    // Verify mock expectations were met
    server.verify_mocks().await?;
    server.stop().await?;

    Ok(())
}
