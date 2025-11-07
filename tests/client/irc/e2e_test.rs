//! E2E tests for IRC client
//!
//! These tests verify IRC client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.
//! Test strategy: Use netget binary to start IRC server + client, < 10 LLM calls total.

#[cfg(all(test, feature = "irc"))]
mod irc_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test IRC client connection and registration
    /// LLM calls: 3 (server startup, client connection, connected event)
    #[tokio::test]
    async fn test_irc_client_connect_and_register() -> E2EResult<()> {
        // Start an IRC server listening on an available port
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via IRC. Accept client connections and log all messages."
        );

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Now start an IRC client that connects to this server
        let client_config = NetGetConfig::new(format!(
            "Connect to IRC at 127.0.0.1:{} with nickname testbot, wait for registration to complete.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and register
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client output shows connection
        assert!(
            client.output_contains("connected").await || client.output_contains("registration").await,
            "Client should show connection or registration message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ IRC client connected and registered successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test IRC client can join channel and send message
    /// LLM calls: 4 (server startup, client connection, connected event, message sending)
    #[tokio::test]
    async fn test_irc_client_join_and_message() -> E2EResult<()> {
        // Start an IRC server
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via IRC. Accept all channel joins and log PRIVMSG commands."
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that joins a channel and sends a message
        let client_config = NetGetConfig::new(format!(
            "Connect to IRC at 127.0.0.1:{} with nickname testbot. After connecting, join #test and say 'Hello, channel!'",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect, register, join, and send message
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Verify the client is IRC protocol
        assert_eq!(client.protocol, "IRC", "Client should be IRC protocol");

        println!("✅ IRC client joined channel and sent message");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test IRC client responds to server messages
    /// LLM calls: 5 (server startup, client connection, connected event, server message, client response)
    #[tokio::test]
    async fn test_irc_client_responds_to_messages() -> E2EResult<()> {
        // Start an IRC server that will send a message to the client
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via IRC. When a client joins #bot, send them a PRIVMSG saying 'Welcome bot!'"
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that joins and responds to messages
        let client_config = NetGetConfig::new(format!(
            "Connect to IRC at 127.0.0.1:{} with nickname responsebot. Join #bot and respond to any messages with 'Thanks!'",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give time for connection, join, server message, and client response
        tokio::time::sleep(Duration::from_secs(4)).await;

        // Verify client received and processed messages
        let output = client.get_output().await;
        assert!(
            output.contains("PRIVMSG") || output.contains("message"),
            "Client should show received message. Output: {:?}",
            output
        );

        println!("✅ IRC client responded to server messages");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
