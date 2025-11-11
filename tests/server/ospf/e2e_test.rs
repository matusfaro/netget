//! OSPF E2E tests
//!
//! Tests OSPF server with manual OSPF client

#[cfg(all(test, feature = "ospf"))]
mod tests {
    use std::net::{Ipv4Addr, SocketAddr};
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
    async fn test_ospf_hello_exchange() {
        // Find available port for test
        let test_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let test_port = test_socket.local_addr().unwrap().port();
        drop(test_socket); // Release the port

        // Start NetGet server
        let server_addr =
            SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), test_port);

        // TODO: Start server with LLM
        // For now, this is a compilation test
        // Full implementation would:
        // 1. Start server with instruction: "Listen on port {port} via OSPF as router 1.1.1.1 in area 0.0.0.0"
        // 2. Create UDP client
        // 3. Send Hello packet
        // 4. Receive and verify Hello response
        // 5. Verify neighbor state transitions

        println!(
            "OSPF E2E test placeholder - server would listen on {}",
            server_addr
        );
        println!("Future implementation:");
        println!("  1. Start OSPF server with LLM");
        println!("  2. Send OSPF Hello packet");
        println!("  3. Verify Hello response");
        println!("  4. Check neighbor state: Down → Init → 2-Way");
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
