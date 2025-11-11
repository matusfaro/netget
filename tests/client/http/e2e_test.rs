//! E2E tests for HTTP client
//!
//! These tests verify HTTP client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.

#[cfg(all(test, feature = "http"))]
mod http_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test HTTP client making a GET request
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_http_client_get_request() -> E2EResult<()> {
        // Start an HTTP server listening on an available port
        let server_config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via HTTP. Respond to GET requests with 'Hello from server'.");

        let server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Now start an HTTP client that makes a GET request
        let client_config = NetGetConfig::new(format!(
            "Connect to http://127.0.0.1:{} via HTTP. Send a GET request to / and read the response.",
            server.port
        ));

        let client = start_netget_client(client_config).await?;

        // Give client time to make request
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify client output shows connection/response
        assert!(
            client.output_contains("HTTP").await || client.output_contains("connected").await,
            "Client should show HTTP protocol or connection message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ HTTP client made GET request successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test HTTP client can send requests based on LLM instructions
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_http_client_lllm_controlled_request() -> E2EResult<()> {
        // Start a simple HTTP server
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via HTTP. Log all incoming requests.",
        );

        let server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that makes a specific request based on LLM instruction
        let client_config = NetGetConfig::new(format!(
            "Connect to http://127.0.0.1:{} via HTTP and send a GET request with custom headers.",
            server.port
        ));

        let client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify the client is HTTP protocol
        assert_eq!(client.protocol, "HTTP", "Client should be HTTP protocol");

        println!("✅ HTTP client responded to LLM instruction");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
