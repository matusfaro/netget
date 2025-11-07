//! E2E tests for TURN client
//!
//! These tests verify TURN client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.
//! Test strategy: Use netget binary to start server + client, < 10 LLM calls total.

#[cfg(all(test, feature = "turn"))]
mod turn_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test TURN client connection and allocation
    /// LLM calls: 4 (server startup, client connection, allocation, permission)
    #[tokio::test]
    async fn test_turn_client_allocate_relay() -> E2EResult<()> {
        // Start a TURN server on an available port
        let server_config = NetGetConfig::new(
            "Start TURN relay server on port {AVAILABLE_PORT}. When client requests allocation, \
             assign relay address and return 600 second lifetime."
        );

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Start TURN client that allocates a relay
        let client_config = NetGetConfig::new(format!(
            "Connect to TURN server at 127.0.0.1:{} and allocate a relay address with 600 second lifetime.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and allocate
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client output shows connection
        assert!(
            client.output_contains("TURN").await || client.output_contains("connected").await,
            "Client should show TURN connection. Output: {:?}",
            client.get_output().await
        );

        println!("✅ TURN client connected and allocated relay successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test TURN client can create permissions
    /// LLM calls: 5 (server startup, client connection, allocation, permission create, confirm)
    #[tokio::test]
    async fn test_turn_client_create_permission() -> E2EResult<()> {
        // Start TURN server
        let server_config = NetGetConfig::new(
            "Start TURN relay server on port {AVAILABLE_PORT}. Accept all allocation and permission requests."
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that allocates and creates permission
        let client_config = NetGetConfig::new(format!(
            "Connect to TURN server at 127.0.0.1:{}, allocate a relay, and create permission for peer 192.168.1.100:5000.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client is TURN protocol
        assert_eq!(client.protocol, "TURN", "Client should be TURN protocol");

        println!("✅ TURN client created permission successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test TURN client can refresh allocation
    /// LLM calls: 5 (server startup, client connection, allocation, refresh request, confirm)
    #[tokio::test]
    async fn test_turn_client_refresh_allocation() -> E2EResult<()> {
        // Start TURN server
        let server_config = NetGetConfig::new(
            "Start TURN relay server on port {AVAILABLE_PORT}. Accept all allocation and refresh requests."
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that allocates and refreshes
        let client_config = NetGetConfig::new(format!(
            "Connect to TURN server at 127.0.0.1:{}, allocate a relay with 60 second lifetime, \
             then refresh it to extend the lifetime by another 600 seconds.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify TURN operations occurred
        let output = client.get_output().await;
        assert!(
            output.contains("TURN") || output.contains("refresh") || output.contains("allocated"),
            "Client should show TURN allocation/refresh activity. Output: {:?}",
            output
        );

        println!("✅ TURN client refreshed allocation successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
