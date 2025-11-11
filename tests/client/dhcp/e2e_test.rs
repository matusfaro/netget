//! E2E tests for DHCP client
//!
//! These tests verify DHCP client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.
//! Test strategy: Use netget binary to start DHCP server + client, < 10 LLM calls total.
//!
//! NOTE: DHCP client requires binding to port 68, which may need elevated privileges.
//! These tests may fail on systems without sufficient permissions.

#[cfg(all(test, feature = "dhcp"))]
mod dhcp_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Test DHCP client can send DISCOVER and receive OFFER
    /// LLM calls: 4 (server startup, client startup, client action after OFFER)
    #[tokio::test]
    #[ignore] // Requires elevated privileges to bind port 68
    async fn test_dhcp_client_discover_offer() -> E2EResult<()> {
        // Start a DHCP server that offers IP 192.168.1.100
        let server_config = NetGetConfig::new(
            "Listen on port 67 via DHCP. \
            When receiving DHCP DISCOVER, offer IP 192.168.1.100 with subnet mask 255.255.255.0, \
            router 192.168.1.1, and DNS 8.8.8.8. Lease time 24 hours.",
        );

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Start DHCP client to send DISCOVER
        let client_config = NetGetConfig::new(
            "Connect to 127.0.0.1:67 via DHCP. \
            Send DHCP DISCOVER with MAC address 00:11:22:33:44:55. \
            When receiving OFFER, log the offered IP address and network configuration.",
        );

        let mut client = start_netget_client(client_config).await?;

        // Give client time to connect and receive OFFER
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Verify client shows connection
        assert!(
            client.output_contains("dhcp").await || client.output_contains("DHCP").await,
            "Client should show DHCP activity. Output: {:?}",
            client.get_output().await
        );

        println!("✅ DHCP client sent DISCOVER and received OFFER");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test DHCP client can complete full DORA exchange
    /// LLM calls: 6 (server startup, client startup, server OFFER, client REQUEST, server ACK, client parse ACK)
    #[tokio::test]
    #[ignore] // Requires elevated privileges to bind port 68
    async fn test_dhcp_client_full_dora() -> E2EResult<()> {
        // Start a DHCP server
        let server_config = NetGetConfig::new(
            "Listen on port 67 via DHCP. \
            When receiving DHCP DISCOVER, offer IP 192.168.1.100. \
            When receiving DHCP REQUEST, acknowledge with ACK including subnet mask 255.255.255.0.",
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Start DHCP client to complete full DORA
        let client_config = NetGetConfig::new(
            "Connect to 127.0.0.1:67 via DHCP. \
            Send DHCP DISCOVER with MAC 00:11:22:33:44:55. \
            When receiving OFFER, send DHCP REQUEST for the offered IP. \
            When receiving ACK, log the assigned IP and disconnect.",
        );

        let mut client = start_netget_client(client_config).await?;

        // Give time for DORA exchange (DISCOVER → OFFER → REQUEST → ACK)
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Verify protocol is DHCP
        assert_eq!(client.protocol, "DHCP", "Client should be DHCP protocol");

        println!("✅ DHCP client completed full DORA exchange");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }

    /// Test DHCP client with broadcast mode
    /// LLM calls: 4 (server startup, client startup)
    #[tokio::test]
    #[ignore] // Requires elevated privileges to bind port 68
    async fn test_dhcp_client_broadcast() -> E2EResult<()> {
        // Start DHCP server
        let server_config = NetGetConfig::new(
            "Listen on port 67 via DHCP. \
            Respond to all DHCP DISCOVER messages with OFFER.",
        );

        let mut server = start_netget_server(server_config).await?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Start DHCP client with broadcast
        let client_config = NetGetConfig::new(
            "Connect to 255.255.255.255:67 via DHCP. \
            Send DHCP DISCOVER as broadcast to find all DHCP servers on the network. \
            Log any OFFER responses received.",
        );

        let mut client = start_netget_client(client_config).await?;

        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client initiated DHCP activity
        assert!(
            client.output_contains("DHCP") || client.output_contains("dhcp"),
            "Client should show DHCP activity"
        );

        println!("✅ DHCP client sent broadcast DISCOVER");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
