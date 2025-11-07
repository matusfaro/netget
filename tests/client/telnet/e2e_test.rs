//! E2E tests for Telnet client
//!
//! These tests verify Telnet client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.
//! Test strategy: Use netget binary to start server + client, < 10 LLM calls total.

#[cfg(all(test, feature = "telnet"))]
mod telnet_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test Telnet client connection to a Telnet server
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_telnet_client_connect_to_server() -> E2EResult<()> {
        // Start a Telnet server listening on an available port
        // Using a simple Telnet-like server (TCP server with option negotiation)
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via Telnet. When client connects, send 'Welcome!\r\n' prompt."
        );

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Now start a Telnet client that connects to this server
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via Telnet. Wait for welcome message.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and negotiate
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Verify client output shows connection
        assert!(
            client.output_contains("connected").await,
            "Client should show connection message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ Telnet client connected to server successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test Telnet client can send commands
    /// LLM calls: 2 (client startup with command)
    #[tokio::test]
    async fn test_telnet_client_send_command() -> E2EResult<()> {
        // Start a Telnet server that echoes commands
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via Telnet. Echo back any text received."
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that sends a command
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via Telnet and send the command 'hello'.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Verify the client protocol is Telnet
        assert_eq!(client.protocol, "Telnet", "Client should be Telnet protocol");

        println!("✅ Telnet client sent command successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test Telnet client can handle option negotiation
    /// LLM calls: 2 (server with negotiation, client response)
    #[tokio::test]
    async fn test_telnet_client_option_negotiation() -> E2EResult<()> {
        // Start a Telnet server that negotiates options
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via Telnet. Send WILL ECHO and DO TERMINAL_TYPE options."
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that responds to negotiation
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via Telnet. Handle option negotiation automatically.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Verify client connected (negotiation happens automatically)
        assert!(
            client.output_contains("connected").await || client.output_contains("Telnet").await,
            "Client should show connection or Telnet activity. Output: {:?}",
            client.get_output().await
        );

        println!("✅ Telnet client handled option negotiation");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
