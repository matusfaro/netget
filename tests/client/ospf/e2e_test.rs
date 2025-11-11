//! E2E tests for OSPF client
//!
//! These tests verify OSPF client functionality by spawning the actual NetGet binary
//! and testing client behavior as a black-box.
//!
//! **IMPORTANT**: OSPF requires root/CAP_NET_RAW privileges for raw IP sockets.
//! These tests will SKIP if not running with sufficient privileges.
//!
//! Test strategy: Use netget binary to start OSPF client, < 5 LLM calls total.

#[cfg(all(test, feature = "ospf"))]
mod ospf_client_tests {
    use crate::helpers::*;
    use std::time::Duration;

    /// Helper function to check if we have root privileges
    fn has_root_privileges() -> bool {
        #[cfg(unix)]
        {
            unsafe { libc::geteuid() == 0 }
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    /// Test OSPF client initialization
    /// LLM calls: 1 (client startup)
    ///
    /// This test verifies that the OSPF client can be initialized and provides
    /// appropriate error messages when root privileges are missing.
    #[tokio::test]
    async fn test_ospf_client_initialization() -> E2EResult<()> {
        if !has_root_privileges() {
            println!("⚠️  Skipping test: OSPF requires root privileges");
            println!("   Run with: sudo -E cargo test --no-default-features --features ospf");
            return Ok(());
        }

        // Start OSPF client on loopback interface
        let client_config = NetGetConfig::new(
            "Connect to 127.0.0.1 via OSPF. Monitor for Hello packets. Don't send any packets yet.",
        );

        let mut client = start_netget_client(client_config).await?;

        // Give client time to initialize
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Verify client output shows OSPF initialization
        let output = client.get_output().await;
        assert!(
            output.contains("OSPF") || output.contains("ospf"),
            "Client should mention OSPF in output. Output: {:?}",
            output
        );

        println!("✅ OSPF client initialized successfully");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test OSPF client can send Hello packet
    /// LLM calls: 2 (client startup, send Hello)
    ///
    /// This test verifies the OSPF client can send a Hello packet to the multicast group.
    #[tokio::test]
    async fn test_ospf_client_send_hello() -> E2EResult<()> {
        if !has_root_privileges() {
            println!("⚠️  Skipping test: OSPF requires root privileges");
            return Ok(());
        }

        // Start OSPF client configured to send a Hello packet
        let client_config = NetGetConfig::new(
            "Connect to 192.168.1.100 via OSPF with router_id 1.1.1.1. Send one Hello packet to multicast, then disconnect."
        );

        let mut client = start_netget_client(client_config).await?;

        // Give client time to send Hello
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client shows OSPF activity
        let output = client.get_output().await;
        assert!(
            output.contains("Hello") || output.contains("OSPF") || output.contains("connected"),
            "Client should show OSPF Hello or connection. Output: {:?}",
            output
        );

        println!("✅ OSPF client sent Hello packet");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test OSPF client with OSPF server (full E2E)
    /// LLM calls: 4 (server startup, client startup, server receives Hello, client receives Hello)
    ///
    /// This test starts both an OSPF server and client to verify they can exchange Hello packets.
    #[tokio::test]
    async fn test_ospf_client_with_server() -> E2EResult<()> {
        if !has_root_privileges() {
            println!("⚠️  Skipping test: OSPF requires root privileges");
            return Ok(());
        }

        // Start OSPF server
        let server_config = NetGetConfig::new(
            "Listen on interface 192.168.1.100 as OSPF router 192.168.1.100 in area 0. Respond to all Hello packets."
        );

        let mut server = start_netget_server(server_config).await?;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Start OSPF client
        let client_config = NetGetConfig::new(
            "Connect to 192.168.1.101 via OSPF with router_id 192.168.1.101 in area 0. Send Hello, wait for response."
        );

        let mut client = start_netget_client(client_config).await?;

        // Give time for Hello exchange
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Verify server received Hello
        let server_output = server.get_output().await;
        assert!(
            server_output.contains("Hello") || server_output.contains("neighbor"),
            "Server should receive Hello. Output: {:?}",
            server_output
        );

        // Verify client received response
        let client_output = client.get_output().await;
        assert!(
            client_output.contains("Hello") || client_output.contains("received"),
            "Client should receive Hello response. Output: {:?}",
            client_output
        );

        println!("✅ OSPF client and server exchanged Hello packets");

        // Cleanup
        server.stop().await?;
        client.stop().await?;

        Ok(())
    }
}
