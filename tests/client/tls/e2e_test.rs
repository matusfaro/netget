//! E2E tests for TLS client
//!
//! These tests verify TLS client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.
//! Test strategy: Use netget binary to start TLS server + TLS client, < 10 LLM calls total.

#[cfg(all(test, feature = "tls"))]
mod tls_client_tests {
    use crate::helpers::*;
    use ::netget::logging::patterns;
    use std::time::Duration;

    /// Test TLS client connection to NetGet TLS server with self-signed certificates
    /// LLM calls: 4 (server startup, client startup, server data received, client connected)
    #[tokio::test]
    async fn test_tls_client_connect_to_server() -> E2EResult<()> {
        // Start a TLS server listening on an available port with mocks
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via TLS. Accept connections and echo received data back.",
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Server startup (user command)
                .on_instruction_containing("Listen on port")
                .and_instruction_containing("TLS")
                .and_instruction_containing("echo")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_server",
                        "port": 0,
                        "base_stack": "TLS",
                        "instruction": "Echo server - respond with exactly what is received"
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Server receives encrypted data (tls_data_received event)
                .on_event("tls_data_received")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_tls_data",
                        "data": "HELLO_TLS" // UTF-8 string (echoed back)
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let server = start_netget_server(server_config).await?;

        // Wait for TLS server to be listening
        server
            .wait_for_pattern("TLS server (action-based) listening on", Duration::from_secs(5))
            .await?;

        // Now start a TLS client that connects to this server with mocks
        // IMPORTANT: accept_invalid_certs: true because server uses self-signed cert
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via TLS (accept invalid certificates). Send 'HELLO_TLS' and wait for response.",
            server.port
        ))
        .with_mock(|mock| {
            mock
                // Mock 1: Client startup (user command)
                .on_instruction_containing("Connect to")
                .and_instruction_containing("TLS")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": format!("127.0.0.1:{}", server.port),
                        "protocol": "TLS",
                        "instruction": "Send HELLO_TLS and wait for echo",
                        "startup_params": {
                            "accept_invalid_certs": true
                        }
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Client connected after TLS handshake (tls_client_connected event)
                .on_event("tls_client_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_tls_data",
                        "data": "HELLO_TLS" // UTF-8 string
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let client = start_netget_client(client_config).await?;

        // Wait for TLS handshake and data exchange
        client
            .wait_for_patterns(
                &[
                    "TLS handshake complete",           // TLS handshake succeeded
                    patterns::TLS_CLIENT_CONNECTED,     // Client connected
                    patterns::TLS_CLIENT_SENT,          // Client sent HELLO_TLS
                ],
                Duration::from_secs(10), // Longer timeout for TLS handshake
            )
            .await?;

        // Wait for server to receive and process the encrypted data
        server
            .wait_for_pattern("TLS server received data", Duration::from_secs(5))
            .await?;

        println!("✅ TLS client connected to server with TLS handshake and sent data successfully");

        // Verify mock expectations were met
        server.verify_mocks().await?;
        client.verify_mocks().await?;

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test TLS client with certificate validation (should succeed with valid certs)
    /// This test connects to a public HTTPS server to verify real certificate validation
    /// LLM calls: 2 (client startup, client connected)
    #[tokio::test]
    async fn test_tls_client_certificate_validation() -> E2EResult<()> {
        // Connect to example.com:443 with full certificate validation
        let client_config = NetGetConfig::new(
            "Connect to example.com:443 via TLS and send an HTTP request for /",
        )
        .with_mock(|mock| {
            mock
                // Mock 1: Client startup (user command)
                .on_instruction_containing("example.com:443")
                .and_instruction_containing("TLS")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": "example.com:443",
                        "protocol": "TLS",
                        "instruction": "Send HTTP GET request",
                        "startup_params": {
                            "accept_invalid_certs": false  // Full validation
                        }
                    }
                ]))
                .expect_calls(1)
                .and()
                // Mock 2: Client connected (tls_client_connected event)
                .on_event("tls_client_connected")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "send_tls_data",
                        "data": "GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n"
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let client = start_netget_client(client_config).await?;

        // Wait for TLS connection with valid certificate
        client
            .wait_for_patterns(
                &[
                    "TLS handshake complete",       // Handshake succeeded
                    patterns::TLS_CLIENT_CONNECTED, // Client connected
                ],
                Duration::from_secs(15), // Longer timeout for network
            )
            .await?;

        println!("✅ TLS client successfully validated example.com certificate");

        // Verify mock expectations
        client.verify_mocks().await?;

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test TLS client rejects invalid certificates when validation is enabled
    /// LLM calls: 1 (client startup - connection should fail before connected event)
    #[tokio::test]
    async fn test_tls_client_rejects_self_signed_cert() -> E2EResult<()> {
        // Start a TLS server with self-signed certificate
        let server_config =
            NetGetConfig::new("Listen on port {AVAILABLE_PORT} via TLS. Log connections.")
            .with_mock(|mock| {
                mock
                    .on_instruction_containing("TLS")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "TLS",
                            "instruction": "Log connections"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let server = start_netget_server(server_config).await?;

        // Wait for server listening
        server
            .wait_for_pattern("TLS server (action-based) listening on", Duration::from_secs(5))
            .await?;

        // Try to connect with certificate validation enabled (should fail)
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via TLS with certificate validation",
            server.port
        ))
        .with_mock(|mock| {
            mock
                .on_instruction_containing("TLS")
                .respond_with_actions(serde_json::json!([
                    {
                        "type": "open_client",
                        "remote_addr": format!("127.0.0.1:{}", server.port),
                        "protocol": "TLS",
                        "instruction": "Try to connect",
                        "startup_params": {
                            "accept_invalid_certs": false  // Require valid cert
                        }
                    }
                ]))
                .expect_calls(1)
                .and()
        });

        let client = start_netget_client(client_config).await?;

        // Wait for handshake failure (certificate validation error)
        // We expect the connection to fail, not succeed
        let result = client
            .wait_for_pattern("TLS handshake failed", Duration::from_secs(10))
            .await;

        // The handshake should fail
        assert!(
            result.is_ok(),
            "Expected TLS handshake to fail due to self-signed certificate"
        );

        println!("✅ TLS client correctly rejected self-signed certificate");

        // Verify mock expectations
        client.verify_mocks().await?;

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
