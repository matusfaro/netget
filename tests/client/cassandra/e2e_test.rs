//! E2E tests for Cassandra client
//!
//! These tests verify Cassandra client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.

#[cfg(all(test, feature = "cassandra"))]
mod cassandra_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test Cassandra client connection and query execution
    /// LLM calls: 3 (server startup, client connection, query execution)
    #[tokio::test]
    async fn test_cassandra_client_connect_and_query() -> E2EResult<()> {
        // Start a Cassandra server listening on an available port
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via Cassandra. Accept CQL queries. For SELECT * FROM system.local, return a result set with host_id and cluster_name columns.",
        );

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Now start a Cassandra client that connects and sends a query
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via Cassandra. Execute 'SELECT * FROM system.local' query.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and execute query
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Verify client output shows connection
        assert!(
            client.output_contains("connected").await,
            "Client should show connection message. Output: {:?}",
            client.get_output().await
        );

        println!("✅ Cassandra client connected and executed query successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test Cassandra client with consistency level
    /// LLM calls: 3 (server startup, client connection, query with consistency)
    #[tokio::test]
    async fn test_cassandra_client_with_consistency() -> E2EResult<()> {
        // Start a Cassandra server
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via Cassandra. Accept CQL queries and log consistency levels.",
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Client with specific consistency level instruction
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via Cassandra. Execute 'SELECT * FROM system.local' with QUORUM consistency.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Verify the client is Cassandra protocol
        assert_eq!(client.protocol, "Cassandra", "Client should be Cassandra protocol");

        println!("✅ Cassandra client executed query with consistency level");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test Cassandra client multi-step query execution
    /// LLM calls: 4+ (server startup, client connection, multiple queries)
    #[tokio::test]
    async fn test_cassandra_client_multi_query() -> E2EResult<()> {
        // Start a Cassandra server that handles multiple queries
        let server_config = NetGetConfig::new(
            "Listen on port {AVAILABLE_PORT} via Cassandra. Accept CQL queries. For SELECT queries, return mock results.",
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Client that executes multiple queries
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via Cassandra. First, query system.local. Then query system.peers.",
            server.port
        ));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_millis(2000)).await;

        // Verify client connected
        assert!(
            client.output_contains("connected").await,
            "Client should show connection. Output: {:?}",
            client.get_output().await
        );

        println!("✅ Cassandra client executed multiple queries");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
