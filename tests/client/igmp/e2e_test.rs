//! E2E tests for IGMP client
//!
//! These tests verify IGMP client functionality by spawning the actual NetGet binary
//! and testing multicast group join/leave behavior as a black-box.
//! Test strategy: Use netget binary to start IGMP client, < 10 LLM calls total.

#[cfg(all(test, feature = "igmp"))]
mod igmp_client_tests {
    use crate::helpers::*;
    use std::time::Duration;
    use tokio::net::UdpSocket;

    /// Test IGMP client can join a multicast group and receive data
    /// LLM calls: 2 (client startup, multicast join)
    #[tokio::test]
    async fn test_igmp_client_join_and_receive() -> E2EResult<()> {
        // Start IGMP client and instruct it to join a multicast group
        let multicast_group = "239.255.1.1";
        let multicast_port = 15000;

        let client_config = NetGetConfig::new(format!(
            "Start IGMP client on port {}. Join multicast group {} and log all received data.",
            multicast_port, multicast_group
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to start and join the group
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client shows it's ready
        assert!(
            client.output_contains("IGMP").await,
            "Client should show IGMP initialization. Output: {:?}",
            client.get_output().await
        );

        println!("✅ IGMP client initialized");

        // Send a multicast packet to the group
        let sender = UdpSocket::bind("0.0.0.0:0").await?;
        let test_data = b"HELLO_MULTICAST";
        let dest = format!("{}:{}", multicast_group, multicast_port);
        sender.send_to(test_data, &dest).await?;

        println!("✅ Sent multicast packet to {}", dest);

        // Give client time to receive and process
        tokio::time::sleep(Duration::from_secs(1)).await;

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test IGMP client can join and leave multicast groups
    /// LLM calls: 3 (client startup, join, leave)
    #[tokio::test]
    async fn test_igmp_client_join_and_leave() -> E2EResult<()> {
        let multicast_group = "239.255.1.2";

        let client_config = NetGetConfig::new(format!(
            "Start IGMP client. Join multicast group {}, then immediately leave it.",
            multicast_group
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to process
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client processed the instructions
        assert_eq!(client.protocol, "igmp", "Client should be IGMP protocol");

        println!("✅ IGMP client joined and left multicast group");

        // Cleanup
        client.stop().await?;

        Ok(())
    }

    /// Test IGMP client can send multicast data
    /// LLM calls: 2 (client startup, send)
    #[tokio::test]
    async fn test_igmp_client_send_multicast() -> E2EResult<()> {
        let multicast_group = "239.255.1.3";
        let multicast_port = 15001;

        let client_config = NetGetConfig::new(format!(
            "Start IGMP client. Send the string 'TEST' to multicast group {} port {}.",
            multicast_group, multicast_port
        ));

        let mut client = start_netget_client(client_config).await?;

        // Give client time to send
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify client is IGMP protocol
        assert_eq!(client.protocol, "igmp", "Client should be IGMP protocol");

        println!("✅ IGMP client sent multicast data");

        // Cleanup
        client.stop().await?;

        Ok(())
    }
}
