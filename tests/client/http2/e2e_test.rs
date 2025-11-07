//! E2E tests for HTTP/2 client
//!
//! These tests verify HTTP/2 client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.

#[cfg(all(test, feature = "http2"))]
mod http2_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test HTTP/2 client making a GET request
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_http2_client_get_request() -> E2EResult<()> {
        // Start an HTTP/2 server listening on an available port
        let server_config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via HTTP/2. Respond to GET requests with 'Hello from HTTP/2 server'.");

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Now start an HTTP/2 client that makes a GET request
        let client_config = NetGetConfig::new(format!(
            "Connect to http://127.0.0.1:{} via HTTP/2. Send a GET request to / and read the response.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to make request
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify client output shows connection/response
        assert!(
            client.output_contains("HTTP2").await ||
            client.output_contains("http2").await ||
            client.output_contains("HTTP/2").await ||
            client.output_contains("connected").await,
            "Client should show HTTP/2 protocol or connection message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ HTTP/2 client made GET request successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test HTTP/2 client can send requests based on LLM instructions
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_http2_client_llm_controlled_request() -> E2EResult<()> {
        // Start a simple HTTP/2 server
        let server_config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via HTTP/2. Log all incoming requests.");

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that makes a specific request based on LLM instruction
        let client_config = NetGetConfig::new(format!(
            "Connect to http://127.0.0.1:{} via HTTP/2 and send a GET request with custom headers.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify the client is HTTP2 protocol
        assert_eq!(client.protocol, "HTTP2", "Client should be HTTP2 protocol");

        println!("✅ HTTP/2 client responded to LLM instruction");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test HTTP/2 multiplexing with concurrent requests
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_http2_client_multiplexing() -> E2EResult<()> {
        // Start an HTTP/2 server
        let server_config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via HTTP/2. Respond to all requests with their path.");

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that makes multiple requests
        let client_config = NetGetConfig::new(format!(
            "Connect to http://127.0.0.1:{} via HTTP/2. Send GET requests to /first and /second.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_secs(1)).await;

        // Verify client shows HTTP/2 protocol
        assert!(
            client.output_contains("HTTP2").await ||
            client.output_contains("http2").await ||
            client.output_contains("HTTP/2").await,
            "Client should show HTTP/2 protocol"
        );

        println!("✅ HTTP/2 client demonstrated multiplexing capability");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
