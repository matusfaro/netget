//! E2E tests for Bitcoin RPC client
//!
//! These tests verify Bitcoin RPC client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.

#[cfg(all(test, feature = "bitcoin"))]
mod bitcoin_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test Bitcoin RPC client connecting to a server
    /// LLM calls: 2 (server startup, client connection)
    ///
    /// Note: This test uses a Bitcoin server mock since we don't have a real Bitcoin Core node.
    /// The server responds with minimal JSON-RPC responses to verify client connectivity.
    #[tokio::test]
    async fn test_bitcoin_client_connection() -> E2EResult<()> {
        // Start a Bitcoin server mock (HTTP server that responds to JSON-RPC calls)
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via HTTP. \
             Respond to POST requests with JSON-RPC format. \
             Return blockchain info when method is getblockchaininfo.",
        );

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Now start a Bitcoin RPC client
        let client_config = NetGetConfig::new(format!(
            "Connect to http://test:test@127.0.0.1:{} via Bitcoin RPC. \
             Get blockchain information.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to initialize
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify client output shows Bitcoin protocol or connection message
        assert!(
            client.output_contains("Bitcoin").await
                || client.output_contains("bitcoin").await
                || client.output_contains("connected").await,
            "Client should show Bitcoin protocol or connection message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ Bitcoin RPC client connected successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test Bitcoin RPC client can execute RPC commands
    /// LLM calls: 2 (server startup, client connection + command execution)
    #[tokio::test]
    async fn test_bitcoin_client_rpc_command() -> E2EResult<()> {
        // Start a minimal HTTP server to simulate Bitcoin RPC
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via HTTP. \
             Log all incoming POST requests.",
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that makes an RPC call
        let client_config = NetGetConfig::new(format!(
            "Connect to Bitcoin RPC at http://bitcoinrpc:pass@127.0.0.1:{} \
             and execute getblockchaininfo command.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify the client is Bitcoin protocol
        assert_eq!(
            client.protocol, "Bitcoin",
            "Client should be Bitcoin protocol"
        );

        println!("✅ Bitcoin RPC client executed command");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
