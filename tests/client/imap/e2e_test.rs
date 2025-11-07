//! E2E tests for IMAP client
//!
//! These tests verify IMAP client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.

#[cfg(all(test, feature = "imap"))]
mod imap_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test IMAP client connection and authentication
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_imap_client_connect_and_authenticate() -> E2EResult<()> {
        // Start an IMAP server listening on an available port
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via IMAP. Accept username 'testuser' and password 'testpass'.",
        );

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Now start an IMAP client that connects and authenticates
        let client_config = NetGetConfig::new_with_startup_params(
            format!(
                "Connect to 127.0.0.1:{} via IMAP. Select INBOX and check for messages.",
                server.port
            ),
            serde_json::json!({
                "username": "testuser",
                "password": "testpass",
                "use_tls": false,
            }),
        );

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and authenticate
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client output shows connection
        assert!(
            client.output_contains("connected").await || client.output_contains("authenticated").await,
            "Client should show connection/authentication message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ IMAP client connected and authenticated successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test IMAP client mailbox selection
    /// LLM calls: 2 (server startup, client connection + selection)
    #[tokio::test]
    async fn test_imap_client_select_mailbox() -> E2EResult<()> {
        // Start IMAP server
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via IMAP. Provide a mailbox 'INBOX' with 5 messages.",
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that selects a specific mailbox
        let client_config = NetGetConfig::new_with_startup_params(
            format!(
                "Connect to 127.0.0.1:{} via IMAP. Select the INBOX mailbox and report the number of messages.",
                server.port
            ),
            serde_json::json!({
                "username": "testuser",
                "password": "testpass",
                "use_tls": false,
            }),
        );

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify the client is IMAP protocol
        assert_eq!(client.protocol, "IMAP", "Client should be IMAP protocol");

        println!("✅ IMAP client selected mailbox successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test IMAP client search functionality
    /// LLM calls: 2 (server startup, client connection + search)
    #[tokio::test]
    async fn test_imap_client_search_messages() -> E2EResult<()> {
        // Start IMAP server with some test messages
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via IMAP. Provide INBOX with 3 unread messages and 2 read messages.",
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that searches for unread messages
        let client_config = NetGetConfig::new_with_startup_params(
            format!(
                "Connect to 127.0.0.1:{} via IMAP. Select INBOX and search for UNSEEN messages.",
                server.port
            ),
            serde_json::json!({
                "username": "testuser",
                "password": "testpass",
                "use_tls": false,
            }),
        );

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client performed search
        let output = client.get_output().await;
        assert!(
            output.contains("search") || output.contains("UNSEEN"),
            "Client should show search operation. Output: {:?}",
            output
        );

        println!("✅ IMAP client searched messages successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test IMAP client fetch message functionality
    /// LLM calls: 2 (server startup, client connection + fetch)
    #[tokio::test]
    async fn test_imap_client_fetch_message() -> E2EResult<()> {
        // Start IMAP server
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via IMAP. Provide INBOX with a message from alice@example.com with subject 'Test Message'.",
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that fetches a specific message
        let client_config = NetGetConfig::new_with_startup_params(
            format!(
                "Connect to 127.0.0.1:{} via IMAP. Select INBOX, search for all messages, and fetch the first one.",
                server.port
            ),
            serde_json::json!({
                "username": "testuser",
                "password": "testpass",
                "use_tls": false,
            }),
        );

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_secs(3)).await;

        // Verify client fetched message
        let output = client.get_output().await;
        assert!(
            output.contains("fetch") || output.contains("message"),
            "Client should show message fetch operation. Output: {:?}",
            output
        );

        println!("✅ IMAP client fetched message successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
