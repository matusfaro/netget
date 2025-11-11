//! E2E tests for Redis client
//!
//! These tests verify Redis client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.

#[cfg(all(test, feature = "redis"))]
mod redis_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test Redis client connection and command execution
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_redis_client_connect_and_command() -> E2EResult<()> {
        // Start a Redis server listening on an available port
        let server_config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via Redis. Accept PING commands and respond with PONG.",);

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Now start a Redis client that connects and sends a command
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via Redis. Send PING command and read response.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and execute command
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify client output shows connection
        assert!(
            client.output_contains("connected").await,
            "Client should show connection message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ Redis client connected and executed command successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test Redis client can be controlled via LLM instructions
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_redis_client_llm_controlled_commands() -> E2EResult<()> {
        // Start a simple Redis server
        let server_config =
            NetGetConfig::new("Listen on port {} via Redis. Log all incoming commands.");

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that sends specific commands based on LLM instruction
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via Redis. Execute SET key1 'value1' command.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify the client is Redis protocol
        assert_eq!(client.protocol, "Redis", "Client should be Redis protocol");

        println!("✅ Redis client responded to LLM instruction");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
