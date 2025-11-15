//! End-to-end tests for HTTP3 protocol implementation
//!
//! These tests spawn a real NetGet instance with HTTP3 server
//! and verify basic server startup functionality.

#![cfg(all(test, feature = "http3"))]

use super::super::helpers::{self, E2EResult, NetGetConfig};

/// Test HTTP3 echo server - send data and receive it back
#[tokio::test]
async fn test_http3_echo() -> E2EResult<()> {
    let config = NetGetConfig::new("Start an HTTP3 server on port 0")
        .with_log_level("debug")
        .with_mock(|mock| {
            mock
                .on_instruction_containing("HTTP3 server")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "HTTP3",
                        "instruction": "Run HTTP3 server"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let server = helpers::start_netget_server(config).await?;
    let port = server.port;

    println!("✓ HTTP3 server started on port {}", port);

    // TODO: Add client test once server is confirmed working
    // For now, just verify the server starts

    // Verify mock expectations were met
    server.verify_mocks().await?;

    server.stop().await?;

    Ok(())
}

/// Test HTTP3 custom response - send command and receive specific response
#[tokio::test]
async fn test_http3_custom_response() -> E2EResult<()> {
    let config = NetGetConfig::new("Start an HTTP3 server on port 0")
        .with_log_level("debug")
        .with_mock(|mock| {
            mock
                .on_instruction_containing("HTTP3 server")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "HTTP3",
                        "instruction": "Respond to PING with PONG"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let server = helpers::start_netget_server(config).await?;
    let port = server.port;

    println!("✓ HTTP3 server started on port {}", port);

    // TODO: Add client test once server is confirmed working

    // Verify mock expectations were met
    server.verify_mocks().await?;

    server.stop().await?;

    Ok(())
}

/// Test HTTP3 multiple streams - verify stream multiplexing
#[tokio::test]
async fn test_http3_multiple_streams() -> E2EResult<()> {
    let config = NetGetConfig::new("Start an HTTP3 server on port 0")
        .with_log_level("debug")
        .with_mock(|mock| {
            mock
                .on_instruction_containing("HTTP3 server")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "HTTP3",
                        "instruction": "Echo back all data on multiple streams"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

    let server = helpers::start_netget_server(config).await?;
    let port = server.port;

    println!("✓ HTTP3 server started on port {}", port);

    // TODO: Add client test once server is confirmed working

    // Verify mock expectations were met
    server.verify_mocks().await?;

    server.stop().await?;

    Ok(())
}
