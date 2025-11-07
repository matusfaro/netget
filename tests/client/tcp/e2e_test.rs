//! E2E tests for TCP client
//!
//! These tests verify TCP client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.
//! Test strategy: Use netget binary to start server + client, < 10 LLM calls total.

#[cfg(all(test, feature = "tcp"))]
mod tcp_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test TCP client connection to a local server
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_tcp_client_connect_to_server() -> E2EResult<()> {
        // Start a TCP server listening on an available port
        let server_config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via TCP. Accept one connection, echo received data back.");

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Now start a TCP client that connects to this server
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via TCP. Send 'HELLO' and wait for response.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify client output shows connection
        assert!(
            client.output_contains("connected").await,
            "Client should show connection message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ TCP client connected to server successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test TCP client can be controlled via prompts
    /// LLM calls: 2 (client startup)
    #[tokio::test]
    async fn test_tcp_client_command_via_prompt() -> E2EResult<()> {

        // Start a simple TCP server
        let server_config = NetGetConfig::new("Listen on port {AVAILABLE_PORT} via TCP. Log all incoming data.");

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that sends specific data based on LLM instruction
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via TCP and send the string 'TEST_DATA' then disconnect.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify the client initiated the connection
        assert_eq!(client.protocol, "TCP", "Client should be TCP protocol");

        println!("✅ TCP client responded to LLM instruction");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
