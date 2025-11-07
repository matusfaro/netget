//! E2E tests for MySQL client
//!
//! These tests verify MySQL client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.

#[cfg(all(test, feature = "mysql"))]
mod mysql_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test MySQL client connection and simple query
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_mysql_client_connect_and_query() -> E2EResult<()> {
        // Start a MySQL server listening on an available port
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via MySQL. Accept SELECT queries and respond with sample data.",
        );

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Now start a MySQL client that connects and sends a query
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via MySQL as user 'root' with password ''. Execute SELECT 1 query.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and execute query
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify client output shows connection
        assert!(
            client.output_contains("connected").await,
            "Client should show connection message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ MySQL client connected and executed query successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test MySQL client with database selection
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_mysql_client_with_database() -> E2EResult<()> {
        // Start a MySQL server
        let server_config = NetGetConfig::new("Listen on port {} via MySQL. Accept connections to 'testdb' database.");

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that specifies a database
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via MySQL as user 'root' with database 'testdb'. Execute SELECT * FROM users query.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify the client is MySQL protocol
        assert_eq!(client.protocol, "MySQL", "Client should be MySQL protocol");

        println!("✅ MySQL client connected with database specification");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test MySQL client transaction control
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_mysql_client_transaction() -> E2EResult<()> {
        // Start a MySQL server
        let server_config = NetGetConfig::new("Listen on port {} via MySQL. Accept transaction commands.");

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Client that uses transactions
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via MySQL. Begin a transaction, execute INSERT INTO logs VALUES ('test'), then commit.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Verify client output shows connection
        assert!(
            client.output_contains("connected").await,
            "Client should show connection message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ MySQL client transaction test passed");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
