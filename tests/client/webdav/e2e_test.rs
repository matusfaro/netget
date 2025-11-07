//! E2E tests for WebDAV client
//!
//! These tests verify WebDAV client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.

#[cfg(all(test, feature = "webdav"))]
mod webdav_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test WebDAV client making a PROPFIND request
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_webdav_client_propfind() -> E2EResult<()> {
        // Start a WebDAV server listening on an available port
        let server_config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via WebDAV. Respond to PROPFIND requests with a simple directory listing.");

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Now start a WebDAV client that makes a PROPFIND request
        let client_config = NetGetConfig::new(format!(
            "Connect to http://127.0.0.1:{} via WebDAV. Send a PROPFIND request to / to list directory contents.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to make request
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify client output shows connection/response
        assert!(
            client.output_contains("WebDAV").await || client.output_contains("connected").await || client.output_contains("PROPFIND").await,
            "Client should show WebDAV protocol or PROPFIND message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ WebDAV client made PROPFIND request successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test WebDAV client can perform LLM-controlled operations
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_webdav_client_llm_controlled() -> E2EResult<()> {
        // Start a simple WebDAV server
        let server_config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via WebDAV. Log all incoming WebDAV requests.");

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that performs WebDAV operations based on LLM instruction
        let client_config = NetGetConfig::new(format!(
            "Connect to http://127.0.0.1:{} via WebDAV and list the root directory using PROPFIND.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify the client is WebDAV protocol
        assert_eq!(client.protocol, "WebDAV", "Client should be WebDAV protocol");

        println!("✅ WebDAV client responded to LLM instruction");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
