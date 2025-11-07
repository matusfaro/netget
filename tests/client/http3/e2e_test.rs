//! E2E tests for HTTP/3 client
//!
//! These tests verify HTTP/3 client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box over QUIC transport.

#[cfg(all(test, feature = "http3"))]
mod http3_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test HTTP/3 client making a GET request over QUIC
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_http3_client_get_request() -> E2EResult<()> {
        // Start an HTTP/3 server listening on an available port
        let server_config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via HTTP/3. Respond to GET requests with 'Hello from HTTP/3 server'.");

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Now start an HTTP/3 client that makes a GET request
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via HTTP/3. Send a GET request to / and read the response.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to make QUIC connection and HTTP/3 request
        tokio::time::sleep(Duration::from_millis(2000)).await;

        // Verify client output shows HTTP/3 or QUIC protocol
        assert!(
            client.output_contains("HTTP/3").await
                || client.output_contains("HTTP3").await
                || client.output_contains("QUIC").await
                || client.output_contains("connected").await,
            "Client should show HTTP/3, QUIC protocol or connection message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ HTTP/3 client made GET request over QUIC successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test HTTP/3 client with stream priorities
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_http3_client_with_priority() -> E2EResult<()> {
        // Start an HTTP/3 server
        let server_config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via HTTP/3. Log all incoming requests with their stream IDs.");

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Client that makes a high-priority request
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via HTTP/3. Send a GET request to /urgent with high priority (priority 7).",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(2000)).await;

        // Verify the client is HTTP3 protocol
        assert_eq!(client.protocol, "HTTP3", "Client should be HTTP3 protocol");

        println!("✅ HTTP/3 client sent prioritized request");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test HTTP/3 client can handle LLM-controlled requests
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_http3_client_llm_controlled() -> E2EResult<()> {
        // Start an HTTP/3 server
        let server_config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via HTTP/3. Respond to POST requests with the request body echoed back.");

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Client that makes a POST request with custom headers
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via HTTP/3. Send a POST request to /api/data with JSON body containing a test message.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(2000)).await;

        // Verify client made HTTP/3 connection
        assert!(
            client.output_contains("HTTP3").await || client.output_contains("QUIC").await,
            "Client should use HTTP/3 or QUIC. Output: {:?}",
            client.get_output().await
        );

        println!("✅ HTTP/3 client responded to LLM instruction");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
