//! E2E tests for IS-IS server
//!
//! These tests spawn the NetGet binary and test IS-IS protocol operations
//! using raw UDP clients to send/receive IS-IS PDUs.

#[cfg(all(test, feature = "isis"))]
mod e2e_isis {
    use crate::server::helpers::{start_netget_server, E2EResult, ServerConfig};
    use tokio::net::UdpSocket;
    use tokio::time::{timeout, Duration};

    // IS-IS constants
    const ISIS_DISCRIMINATOR: u8 = 0x83;
    const ISIS_VERSION: u8 = 1;
    const ISIS_HELLO_LAN_L2: u8 = 16; // Level 2 LAN Hello

    /// Helper to build a basic IS-IS Hello PDU
    fn build_isis_hello(system_id: [u8; 6], area_id: &[u8], holding_time: u16) -> Vec<u8> {
        let mut pdu = Vec::new();

        // Common Header (8 bytes)
        pdu.push(ISIS_DISCRIMINATOR); // 0x83
        pdu.push(27); // Length Indicator (header length, will update)
        pdu.push(1); // Version/Protocol ID Extension
        pdu.push(0); // ID Length (0 = 6 bytes)
        pdu.push(ISIS_HELLO_LAN_L2); // PDU Type
        pdu.push(ISIS_VERSION); // Version
        pdu.push(0); // Reserved
        pdu.push(0); // Max Area Addresses

        // LAN Hello specific header
        pdu.push(2); // Circuit Type (Level 2)

        // Source ID (6 bytes)
        pdu.extend_from_slice(&system_id);

        // Holding Time (2 bytes)
        pdu.extend_from_slice(&holding_time.to_be_bytes());

        // PDU Length (2 bytes) - placeholder
        let pdu_len_offset = pdu.len();
        pdu.extend_from_slice(&[0, 0]);

        // Priority (1 byte)
        pdu.push(64);

        // LAN ID (7 bytes: 6 bytes system ID + 1 byte pseudonode)
        pdu.extend_from_slice(&system_id);
        pdu.push(0); // Pseudonode ID

        // TLVs
        // TLV 1: Area Addresses
        pdu.push(1); // Type
        pdu.push(area_id.len() as u8 + 1); // Length (area length + 1-byte length prefix)
        pdu.push(area_id.len() as u8); // Area address length
        pdu.extend_from_slice(area_id);

        // TLV 129: Protocols Supported (IPv4)
        pdu.push(129); // Type
        pdu.push(1); // Length
        pdu.push(0xCC); // IPv4 NLPID

        // Update PDU Length
        let pdu_len = pdu.len() as u16;
        pdu[pdu_len_offset..pdu_len_offset + 2].copy_from_slice(&pdu_len.to_be_bytes());

        pdu
    }

    /// Helper to parse IS-IS PDU header
    fn parse_isis_header(data: &[u8]) -> E2EResult<(u8, u8)> {
        if data.len() < 8 {
            return Err("IS-IS PDU too short".into());
        }

        let discriminator = data[0];
        let pdu_type = data[4];

        if discriminator != ISIS_DISCRIMINATOR {
            return Err(format!("Invalid IS-IS discriminator: 0x{:02x}", discriminator).into());
        }

        Ok((pdu_type, data[5])) // (PDU type, version)
    }

    #[tokio::test]
    async fn test_isis_hello_exchange() -> E2EResult<()> {
        println!("\n=== Test: IS-IS Hello Exchange ===");

        let prompt = "Start an IS-IS router on port 0 with system-id 0000.0000.0001 in area 49.0001 at level-2. \
             When you receive a Hello PDU from a neighbor, respond with your own Hello PDU using the \
             send_isis_hello action. Include your system-id 0000.0000.0001 and area 49.0001 in the response.";

        let server = start_netget_server(ServerConfig::new(prompt)).await?;

        // Wait for server to be ready
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Create UDP socket
        println!("  [TEST] Creating UDP client socket");
        let client = UdpSocket::bind("127.0.0.1:0").await?;
        let server_addr = format!("127.0.0.1:{}", server.port);

        // Build IS-IS Hello PDU from client
        let system_id = [0x00, 0x00, 0x00, 0x00, 0x00, 0x02]; // 0000.0000.0002
        let area_id = &[0x49, 0x00, 0x01]; // 49.0001
        let hello_pdu = build_isis_hello(system_id, area_id, 30);

        // Send Hello PDU
        println!("  [TEST] Sending IS-IS Hello PDU to {}", server_addr);
        client.send_to(&hello_pdu, &server_addr).await?;

        // Wait for response
        println!("  [TEST] Waiting for IS-IS Hello response");
        let mut buf = vec![0u8; 1500];
        let (n, _peer_addr) =
            timeout(Duration::from_secs(120), client.recv_from(&mut buf)).await??;

        println!("  [TEST] Received {} bytes from server", n);

        // Parse response header
        let response = &buf[..n];
        let (pdu_type, version) = parse_isis_header(response)?;

        println!("  [TEST] PDU Type: {}, Version: {}", pdu_type, version);

        // Verify it's a Hello PDU
        assert!(
            pdu_type == 15 || pdu_type == 16 || pdu_type == 17,
            "Expected Hello PDU (type 15, 16, or 17), got type {}",
            pdu_type
        );
        assert_eq!(version, ISIS_VERSION, "IS-IS version should be 1");

        // Verify the response contains valid IS-IS structure
        assert!(
            response.len() >= 27,
            "IS-IS Hello PDU should be at least 27 bytes"
        );

        println!("  [TEST] ✓ IS-IS Hello exchange successful");
        Ok(())
    }

    #[tokio::test]
    async fn test_isis_multiple_hellos() -> E2EResult<()> {
        println!("\n=== Test: IS-IS Multiple Hello Exchanges ===");

        let prompt =
            "Start an IS-IS router on port 0 with system-id 0000.0000.0001 in area 49.0001. \
             Respond to all Hello PDUs with your own Hello PDU.";

        let server = start_netget_server(ServerConfig::new(prompt)).await?;

        // Wait for server to be ready
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Create UDP socket
        let client = UdpSocket::bind("127.0.0.1:0").await?;
        let server_addr = format!("127.0.0.1:{}", server.port);

        // Send multiple Hellos
        for i in 0..3 {
            println!("  [TEST] Sending Hello #{}", i + 1);

            let system_id = [0x00, 0x00, 0x00, 0x00, 0x00, 0x02 + i];
            let area_id = &[0x49, 0x00, 0x01];
            let hello_pdu = build_isis_hello(system_id, area_id, 30);

            client.send_to(&hello_pdu, &server_addr).await?;

            // Wait for response
            let mut buf = vec![0u8; 1500];
            let (n, _) = timeout(Duration::from_secs(120), client.recv_from(&mut buf)).await??;

            let (pdu_type, _) = parse_isis_header(&buf[..n])?;
            assert!(
                pdu_type == 15 || pdu_type == 16 || pdu_type == 17,
                "Expected Hello PDU"
            );

            println!("  [TEST] ✓ Received Hello response #{}", i + 1);
        }

        println!("  [TEST] ✓ Multiple Hello exchanges successful");
        Ok(())
    }
}
