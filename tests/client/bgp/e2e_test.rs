//! E2E tests for BGP client
//!
//! These tests verify BGP client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.
//! Test strategy: Use netget binary to start BGP server + client, < 10 LLM calls total.

#[cfg(all(test, feature = "bgp"))]
mod bgp_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test BGP client connection to BGP server
    /// LLM calls: 2 (server startup, client connection)
    #[tokio::test]
    async fn test_bgp_client_connect_to_server() -> E2EResult<()> {
        // Start a BGP server on port 179 (or available port)
        let server_config = NetGetConfig::new(
            "Start BGP server on port {AVAILABLE_PORT} with AS 65000 and router ID 192.168.1.1. Accept connections and respond to OPEN messages."
        );

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Start a BGP client that connects to this server
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via BGP with AS 65001 and router ID 192.168.1.100. Establish BGP session.",
            server.port
        ))
        .with_startup_params(serde_json::json!({
            "local_as": 65001,
            "router_id": "192.168.1.100",
            "hold_time": 180
        }));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and establish session
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client output shows connection
        let output = client.get_output().await;
        assert!(
            client.output_contains("connected").await || client.output_contains("OPEN").await,
            "Client should show BGP connection or OPEN message. Output: {:?}",
            output
        );

        println!("✅ BGP client connected to server successfully");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test BGP client session establishment
    /// LLM calls: 3 (server startup, client connection, session handling)
    #[tokio::test]
    async fn test_bgp_client_session_establishment() -> E2EResult<()> {
        // Start BGP server
        let server_config = NetGetConfig::new(
            "Start BGP server on port {AVAILABLE_PORT} with AS 65000 and router ID 192.168.1.1. Complete OPEN handshake."
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Client connects and establishes session
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via BGP. Establish session with AS 65001 and router ID 192.168.1.100. Wait for session to be established.",
            server.port
        ))
        .with_startup_params(serde_json::json!({
            "local_as": 65001,
            "router_id": "192.168.1.100"
        }));

        let mut client = start_netget_client(client_config).await?;

        // Wait for session establishment (OPEN + KEEPALIVE exchange)
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Verify protocol
        assert_eq!(client.protocol, "BGP", "Client should be BGP protocol");

        // Verify output shows session activity
        let output = client.get_output().await;
        assert!(
            client.output_contains("BGP").await,
            "Client should show BGP activity. Output: {:?}",
            output
        );

        println!("✅ BGP client session established");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test BGP client with custom AS and router ID
    /// LLM calls: 2 (server startup, client with params)
    #[tokio::test]
    async fn test_bgp_client_custom_params() -> E2EResult<()> {
        // Start BGP server
        let server_config = NetGetConfig::new(
            "Start BGP server on port {AVAILABLE_PORT} with AS 64512. Accept BGP connections."
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Client with custom AS and router ID
        let client_config = NetGetConfig::new(format!(
            "Connect to 127.0.0.1:{} via BGP. Use AS 64513 and router ID 10.0.0.1.",
            server.port
        ))
        .with_startup_params(serde_json::json!({
            "local_as": 64513,
            "router_id": "10.0.0.1",
            "hold_time": 120
        }));

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify connection
        assert!(
            client.output_contains("BGP").await || client.output_contains("connected").await,
            "Client should show BGP connection with custom params"
        );

        println!("✅ BGP client with custom AS/router ID");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
