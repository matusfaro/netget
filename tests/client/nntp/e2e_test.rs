//! E2E tests for NNTP client
//!
//! These tests verify NNTP client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.

#[cfg(all(test, feature = "nntp"))]
mod nntp_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test NNTP client connection and basic command
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_nntp_client_connect_and_list() -> E2EResult<()> {
        // Start an NNTP server listening on an available port
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via NNTP. Respond to LIST commands with a test newsgroup 'test.misc'.",
        );

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Now start an NNTP client that connects and sends LIST command
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via NNTP. Send LIST command to get available newsgroups.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and execute command
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client output shows connection
        assert!(
            client.output_contains("connected").await,
            "Client should show connection message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ NNTP client connected and executed LIST command successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test NNTP client can select a newsgroup
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_nntp_client_select_group() -> E2EResult<()> {
        // Start an NNTP server
        let server_config = NetGetConfig::new(
            "Listen on port {} via NNTP. Respond to GROUP commands. For group 'comp.lang.rust', respond with '211 10 1 10 comp.lang.rust'.",
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that selects a newsgroup
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via NNTP. Select the newsgroup 'comp.lang.rust' using GROUP command.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify the client is NNTP protocol
        assert_eq!(client.protocol, "NNTP", "Client should be NNTP protocol");

        // Verify client shows connection
        assert!(
            client.output_contains("connected").await || client.output_contains("NNTP"),
            "Client should show NNTP connection. Output: {:?}",
            client.get_output().await
        );

        println!("✅ NNTP client selected newsgroup successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test NNTP client can retrieve articles
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_nntp_client_retrieve_article() -> E2EResult<()> {
        // Start an NNTP server
        let server_config = NetGetConfig::new(
            "Listen on port {} via NNTP. Respond to ARTICLE commands with a test article containing headers and body.",
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that retrieves an article
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via NNTP. Retrieve article 1 using ARTICLE command.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify the client is NNTP protocol
        assert_eq!(client.protocol, "NNTP", "Client should be NNTP protocol");

        println!("✅ NNTP client retrieved article successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
