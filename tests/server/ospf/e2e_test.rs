//! OSPF E2E tests
//!
//! Tests OSPF server with manual OSPF client

#[cfg(all(test, feature = "ospf"))]
mod tests {
    use crate::helpers::*;
    use tokio::net::UdpSocket;

    // Helper: Parse IPv4 address to bytes
    fn ipv4_to_bytes(ip: &str) -> [u8; 4] {
        let parts: Vec<u8> = ip.split('.').filter_map(|s| s.parse::<u8>().ok()).collect();

        if parts.len() == 4 {
            [parts[0], parts[1], parts[2], parts[3]]
        } else {
            [0, 0, 0, 0]
        }
    }

    // Helper: Build OSPF Hello packet
    fn build_ospf_hello(
        router_id: &str,
        area_id: &str,
        network_mask: &str,
        priority: u8,
    ) -> Vec<u8> {
        let mut packet = Vec::new();

        // OSPF Header (24 bytes)
        packet.push(2); // Version = 2 (OSPFv2)
        packet.push(1); // Type = 1 (Hello)
        packet.extend_from_slice(&[0, 0]); // Packet Length (placeholder)

        // Router ID
        let router_id_bytes = ipv4_to_bytes(router_id);
        packet.extend_from_slice(&router_id_bytes);

        // Area ID
        let area_id_bytes = ipv4_to_bytes(area_id);
        packet.extend_from_slice(&area_id_bytes);

        packet.extend_from_slice(&[0, 0]); // Checksum (placeholder)
        packet.extend_from_slice(&[0, 0]); // AuType = 0 (no authentication)
        packet.extend_from_slice(&[0; 8]); // Authentication (8 bytes, zeros)

        // Hello packet body
        let network_mask_bytes = ipv4_to_bytes(network_mask);
        packet.extend_from_slice(&network_mask_bytes);

        packet.extend_from_slice(&10u16.to_be_bytes()); // Hello interval = 10 seconds
        packet.push(0); // Options = 0
        packet.push(priority); // Router priority
        packet.extend_from_slice(&40u32.to_be_bytes()); // Router dead interval = 40 seconds

        // Designated Router (0.0.0.0 = none)
        packet.extend_from_slice(&[0, 0, 0, 0]);

        // Backup Designated Router (0.0.0.0 = none)
        packet.extend_from_slice(&[0, 0, 0, 0]);

        // Neighbor list (empty for now)

        // Update packet length
        let packet_len = packet.len() as u16;
        packet[2..4].copy_from_slice(&packet_len.to_be_bytes());

        // Calculate checksum (simplified - Fletcher checksum)
        let checksum = calculate_ospf_checksum(&packet);
        packet[12..14].copy_from_slice(&checksum.to_be_bytes());

        packet
    }

    // Helper: Calculate OSPF checksum (Fletcher checksum)
    fn calculate_ospf_checksum(data: &[u8]) -> u16 {
        let mut c0: u32 = 0;
        let mut c1: u32 = 0;

        // Start after first 2 bytes, skip checksum field (12-13)
        for (i, &byte) in data.iter().enumerate() {
            if (i >= 2 && i < 12) || i >= 14 {
                c0 = (c0 + byte as u32) % 255;
                c1 = (c1 + c0) % 255;
            }
        }

        let x = ((data.len() - 14) * c0 as usize - c1 as usize) % 255;
        let y = (510 - c0 as usize - x) % 255;

        ((x as u16) << 8) | (y as u16)
    }

    #[tokio::test]
    async fn test_ospf_hello_exchange() -> E2EResult<()> {
        println!("\n=== E2E Test: OSPF Hello Exchange ===");

        // PROMPT: Tell the LLM to act as an OSPF router
        let prompt = "Listen on port {AVAILABLE_PORT} via UDP. Act as OSPF router with router_id 1.1.1.1 in area 0.0.0.0. \
                     When receiving OSPF Hello packets, respond with Hello packets including the sender's router_id in the neighbor list.";

        // Start the server with mocks
        let config = NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    // Mock 1: Server startup
                    .on_instruction_containing("OSPF")
                    .and_instruction_containing("router")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "UDP",
                            "application_protocol": "OSPF",
                            "instruction": "Act as OSPF router 1.1.1.1 in area 0.0.0.0"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2: Receive OSPF Hello packet
                    .on_event("udp_data_received")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_udp_data",
                            "data": hex::encode(build_ospf_hello_response())
                        }
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let mut server = start_netget_server(config).await?;
        println!("OSPF server started on port {}", server.port);

        // Create UDP client
        let client_socket = UdpSocket::bind("127.0.0.1:0").await?;
        let server_addr = format!("127.0.0.1:{}", server.port);
        client_socket.connect(&server_addr).await?;
        println!("✓ UDP client connected");

        // Build and send OSPF Hello packet
        let hello_packet = build_ospf_hello(
            "2.2.2.2",       // our router_id
            "0.0.0.0",       // area_id (backbone)
            "255.255.255.0", // network_mask
            1,               // priority
        );

        println!("Sending OSPF Hello packet ({} bytes)...", hello_packet.len());
        client_socket.send(&hello_packet).await?;

        // Receive Hello response
        let mut buf = vec![0u8; 1024];
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client_socket.recv(&mut buf),
        )
        .await
        {
            Ok(Ok(n)) => {
                println!("✓ Received OSPF response ({} bytes)", n);

                // Basic validation of OSPF header
                assert!(n >= 24, "OSPF packet must be at least 24 bytes (header)");
                assert_eq!(buf[0], 2, "OSPF version should be 2");
                assert_eq!(buf[1], 1, "OSPF type should be 1 (Hello)");

                println!("✓ OSPF Hello response validated");
            }
            Ok(Err(e)) => {
                panic!("Failed to receive OSPF response: {}", e);
            }
            Err(_) => {
                panic!("Timeout waiting for OSPF response");
            }
        }

        // Verify mock expectations were met
        server.verify_mocks().await?;

        server.stop().await?;
        println!("=== Test completed ===\n");
        Ok(())
    }

    #[tokio::test]
    async fn test_ospf_neighbor_discovery() -> E2EResult<()> {
        println!("\n=== E2E Test: OSPF Neighbor Discovery ===");

        let prompt = "Listen on port {AVAILABLE_PORT} via UDP. Act as OSPF router 1.1.1.1 in area 0.0.0.0. \
                     Track neighbors from received Hello packets. When receiving Hello, respond with Hello including all known neighbors.";

        let config = NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    // Mock 1: Server startup
                    .on_instruction_containing("OSPF")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "UDP",
                            "application_protocol": "OSPF",
                            "instruction": "Track and respond to OSPF neighbors"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2-3: Multiple Hello exchanges
                    .on_event("udp_data_received")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_udp_data",
                            "data": hex::encode(build_ospf_hello_response())
                        }
                    ]))
                    .expect_calls(2)
                    .and()
            });

        let mut server = start_netget_server(config).await?;
        println!("OSPF server started on port {}", server.port);

        let client_socket = UdpSocket::bind("127.0.0.1:0").await?;
        client_socket.connect(format!("127.0.0.1:{}", server.port)).await?;

        // Send first Hello packet
        println!("Sending first OSPF Hello...");
        let hello1 = build_ospf_hello("3.3.3.3", "0.0.0.0", "255.255.255.0", 1);
        client_socket.send(&hello1).await?;

        // Receive first response
        let mut buf = vec![0u8; 1024];
        let n1 = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client_socket.recv(&mut buf),
        )
        .await??;
        println!("✓ Received first Hello response ({} bytes)", n1);

        // Send second Hello packet
        println!("Sending second OSPF Hello...");
        let hello2 = build_ospf_hello("3.3.3.3", "0.0.0.0", "255.255.255.0", 1);
        client_socket.send(&hello2).await?;

        // Receive second response
        let n2 = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client_socket.recv(&mut buf),
        )
        .await??;
        println!("✓ Received second Hello response ({} bytes)", n2);
        println!("✓ OSPF neighbor discovery test passed");

        // Verify mock expectations were met
        server.verify_mocks().await?;

        server.stop().await?;
        println!("=== Test completed ===\n");
        Ok(())
    }

    #[tokio::test]
    async fn test_ospf_multiple_routers() -> E2EResult<()> {
        println!("\n=== E2E Test: OSPF Multiple Routers ===");

        let prompt = "Listen on port {AVAILABLE_PORT} via UDP. Act as OSPF router 1.1.1.1 in area 0.0.0.0. \
                     Accept Hello packets from multiple routers and maintain neighbor relationships.";

        let config = NetGetConfig::new(prompt)
            .with_mock(|mock| {
                mock
                    // Mock 1: Server startup
                    .on_instruction_containing("OSPF")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            "base_stack": "UDP",
                            "application_protocol": "OSPF",
                            "instruction": "Handle multiple OSPF neighbors"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // Mock 2-4: Multiple Hello packets from different routers
                    .on_event("udp_data_received")
                    .respond_with_actions(serde_json::json!([
                        {
                            "type": "send_udp_data",
                            "data": hex::encode(build_ospf_hello_response())
                        }
                    ]))
                    .expect_calls(3)
                    .and()
            });

        let mut server = start_netget_server(config).await?;
        println!("OSPF server started on port {}", server.port);

        // Create three "routers" (UDP clients with different router IDs)
        let routers = vec![
            ("4.4.4.4", "Router 4"),
            ("5.5.5.5", "Router 5"),
            ("6.6.6.6", "Router 6"),
        ];

        for (router_id, name) in &routers {
            let client_socket = UdpSocket::bind("127.0.0.1:0").await?;
            client_socket.connect(format!("127.0.0.1:{}", server.port)).await?;

            println!("Sending Hello from {} ({})", name, router_id);
            let hello = build_ospf_hello(router_id, "0.0.0.0", "255.255.255.0", 1);
            client_socket.send(&hello).await?;

            // Receive response
            let mut buf = vec![0u8; 1024];
            let n = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                client_socket.recv(&mut buf),
            )
            .await??;
            println!("✓ {} received response ({} bytes)", name, n);
        }

        println!("✓ All routers successfully exchanged Hello packets");

        // Verify mock expectations were met
        server.verify_mocks().await?;

        server.stop().await?;
        println!("=== Test completed ===\n");
        Ok(())
    }

    // Helper function to build a simple OSPF Hello response
    fn build_ospf_hello_response() -> Vec<u8> {
        build_ospf_hello(
            "1.1.1.1",       // server router_id
            "0.0.0.0",       // area_id
            "255.255.255.0", // network_mask
            128,             // priority (DR eligible)
        )
    }

    #[test]
    fn test_ospf_hello_packet_construction() {
        // Test Hello packet construction
        let hello = build_ospf_hello(
            "2.2.2.2",       // router_id
            "0.0.0.0",       // area_id (backbone)
            "255.255.255.0", // network_mask
            1,               // priority
        );

        // Verify packet structure
        assert_eq!(hello[0], 2); // Version = 2
        assert_eq!(hello[1], 1); // Type = Hello

        // Verify packet length
        let packet_len = u16::from_be_bytes([hello[2], hello[3]]);
        assert_eq!(packet_len as usize, hello.len());

        // Verify router ID
        assert_eq!(&hello[4..8], &[2, 2, 2, 2]);

        // Verify area ID (backbone)
        assert_eq!(&hello[8..12], &[0, 0, 0, 0]);

        // Verify Hello interval
        let hello_interval = u16::from_be_bytes([hello[28], hello[29]]);
        assert_eq!(hello_interval, 10);

        // Verify priority
        assert_eq!(hello[31], 1);

        // Verify router dead interval
        let dead_interval = u32::from_be_bytes([hello[32], hello[33], hello[34], hello[35]]);
        assert_eq!(dead_interval, 40);

        println!("✓ OSPF Hello packet constructed correctly");
        println!("  Router ID: 2.2.2.2");
        println!("  Area: 0.0.0.0 (backbone)");
        println!("  Packet length: {} bytes", hello.len());
    }

    #[test]
    fn test_ospf_checksum() {
        // Create a simple test packet
        let mut packet = vec![
            2, 1, // Version, Type
            0, 32, // Length (32 bytes)
            1, 1, 1, 1, // Router ID
            0, 0, 0, 0, // Area ID
            0, 0, // Checksum (placeholder)
            0, 0, // AuType
            0, 0, 0, 0, 0, 0, 0, 0, // Authentication
            // Minimal Hello body
            255, 255, 255, 0, // Network mask
            0, 10, // Hello interval
            0, 1, // Options, Priority
        ];

        // Calculate checksum
        let checksum = calculate_ospf_checksum(&packet);
        packet[12..14].copy_from_slice(&checksum.to_be_bytes());

        // Verify checksum is non-zero
        assert_ne!(checksum, 0);

        println!("✓ OSPF checksum calculated: 0x{:04x}", checksum);
    }
}
