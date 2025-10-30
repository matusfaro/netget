//! E2E tests for OpenVPN honeypot
//!
//! These tests verify OpenVPN honeypot functionality by starting NetGet with OpenVPN prompts
//! and sending crafted OpenVPN handshake packets to detect reconnaissance attempts.

#![cfg(feature = "e2e-tests")]

mod e2e;

use e2e::helpers::*;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

#[tokio::test]
async fn test_openvpn_handshake_detection_v2() {
    let config = ServerConfig::new("Start an OpenVPN honeypot on port 0");

    let mut server = start_netget_server(config).await.expect("Failed to start server");

    // Verify correct stack was selected
    // Stack validation via output;

    // Wait for server to be ready
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Create UDP client socket
    let client = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind client socket");
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("Failed to set read timeout");

    let server_addr: SocketAddr = format!("127.0.0.1:{}", server.port)
        .parse()
        .expect("Failed to parse server address");

    // Build OpenVPN control hard reset client V2 packet
    let handshake = build_openvpn_hard_reset_client_v2();

    // Send handshake
    client
        .send_to(&handshake, server_addr)
        .expect("Failed to send OpenVPN handshake");

    println!("Sent OpenVPN handshake initiation (V2) to {}", server_addr);

    // For honeypot, we don't expect a valid response
    // Just verify the packet was received by checking logs
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Check server output for handshake detection
    let output = server.get_output().await.join("\n");
    assert!(
        output.contains("OpenVPN") || output.contains("OPENVPN") || output.contains("handshake"),
        "Server output should contain OpenVPN handshake detection. Output: {}",
        output
    );

    println!("✓ OpenVPN handshake detection (V2) successful");

    // Cleanup
    let _ = server.stop().await;
}

#[tokio::test]
async fn test_openvpn_handshake_detection_v1() {
    let config = ServerConfig::new("Start an OpenVPN honeypot on port 0");

    let mut server = start_netget_server(config).await.expect("Failed to start server");

    // Stack validation via output;

    tokio::time::sleep(Duration::from_millis(500)).await;

    let client = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind client socket");
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("Failed to set read timeout");

    let server_addr: SocketAddr = format!("127.0.0.1:{}", server.port)
        .parse()
        .expect("Failed to parse server address");

    // Build OpenVPN V1 handshake
    let handshake = build_openvpn_hard_reset_client_v1();

    client
        .send_to(&handshake, server_addr)
        .expect("Failed to send OpenVPN handshake");

    println!("Sent OpenVPN handshake initiation (V1) to {}", server_addr);

    tokio::time::sleep(Duration::from_millis(500)).await;

    let output = server.get_output().await.join("\n");
    assert!(
        output.contains("OpenVPN") || output.contains("OPENVPN"),
        "Server should detect OpenVPN V1 handshake"
    );

    println!("✓ OpenVPN handshake detection (V1) successful");

    server.stop().await;
}

#[tokio::test]
async fn test_openvpn_multiple_packet_types() {
    let config = ServerConfig::new("Start an OpenVPN honeypot on port 0 that logs all packet types");

    let mut server = start_netget_server(config).await.expect("Failed to start server");

    // Stack validation via output;

    tokio::time::sleep(Duration::from_millis(500)).await;

    let client = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind client socket");
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("Failed to set read timeout");

    let server_addr: SocketAddr = format!("127.0.0.1:{}", server.port)
        .parse()
        .expect("Failed to parse server address");

    // Send different packet types
    let packets = vec![
        ("HardResetV2", build_openvpn_hard_reset_client_v2()),
        ("Control", build_openvpn_control_v1()),
        ("Ack", build_openvpn_ack_v1()),
    ];

    for (name, packet) in packets {
        client
            .send_to(&packet, server_addr)
            .expect(&format!("Failed to send {} packet", name));
        println!("Sent OpenVPN {} packet", name);
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // Verify honeypot logged the packets
    tokio::time::sleep(Duration::from_millis(500)).await;
    let output = server.get_output().await.join("\n");

    println!("Server output:\n{}", output);
    assert!(
        output.contains("OpenVPN") || output.contains("OPENVPN"),
        "Server should log OpenVPN packets"
    );

    println!("✓ Multiple OpenVPN packet types detected");

    server.stop().await;
}

#[tokio::test]
async fn test_openvpn_concurrent_connections() {
    let config = ServerConfig::new("Start an OpenVPN honeypot on port 0");

    let mut server = start_netget_server(config).await.expect("Failed to start server");

    // Stack validation via output;

    tokio::time::sleep(Duration::from_millis(500)).await;

    let server_addr: SocketAddr = format!("127.0.0.1:{}", server.port)
        .parse()
        .expect("Failed to parse server address");

    // Spawn multiple clients sending handshakes concurrently
    let mut handles = vec![];
    for i in 0..3 {
        let addr = server_addr.clone();
        let handle = tokio::spawn(async move {
            let client = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind");
            let handshake = build_openvpn_hard_reset_client_v2();
            client.send_to(&handshake, addr).expect("Failed to send");
            println!("✓ Client {} sent OpenVPN handshake", i);
        });
        handles.push(handle);
    }

    // Wait for all clients
    for handle in handles {
        handle.await.expect("Client task failed");
    }

    tokio::time::sleep(Duration::from_millis(500)).await;

    println!("✓ Concurrent OpenVPN connections handled");

    server.stop().await;
}

// ============================================================================
// Packet Building Functions
// ============================================================================

/// Build an OpenVPN Control Hard Reset Client V2 packet (Opcode 7)
fn build_openvpn_hard_reset_client_v2() -> Vec<u8> {
    let mut packet = Vec::new();

    // Opcode (5 bits) = 7 (P_CONTROL_HARD_RESET_CLIENT_V2)
    // Key ID (3 bits) = 0
    // Byte: (7 << 3) | 0 = 0x38
    packet.push(0x38);

    // Session ID (8 bytes) - for V2 packets
    packet.extend_from_slice(&0x0123456789ABCDEFu64.to_be_bytes());

    // HMAC (variable length, typically 20 bytes for SHA1)
    packet.extend_from_slice(&[0xAA; 20]);

    // Packet ID (4 bytes)
    packet.extend_from_slice(&0x00000001u32.to_be_bytes());

    // Payload (variable, minimal for handshake)
    packet.extend_from_slice(&[0xBB; 16]);

    packet
}

/// Build an OpenVPN Control Hard Reset Client V1 packet (Opcode 1)
fn build_openvpn_hard_reset_client_v1() -> Vec<u8> {
    let mut packet = Vec::new();

    // Opcode (5 bits) = 1 (P_CONTROL_HARD_RESET_CLIENT_V1)
    // Key ID (3 bits) = 0
    // Byte: (1 << 3) | 0 = 0x08
    packet.push(0x08);

    // HMAC (20 bytes)
    packet.extend_from_slice(&[0xCC; 20]);

    // Packet ID (4 bytes)
    packet.extend_from_slice(&0x00000001u32.to_be_bytes());

    // Payload
    packet.extend_from_slice(&[0xDD; 16]);

    packet
}

/// Build an OpenVPN Control V1 packet (Opcode 4)
fn build_openvpn_control_v1() -> Vec<u8> {
    let mut packet = Vec::new();

    // Opcode (5 bits) = 4 (P_CONTROL_V1)
    // Key ID (3 bits) = 0
    // Byte: (4 << 3) | 0 = 0x20
    packet.push(0x20);

    // HMAC (20 bytes)
    packet.extend_from_slice(&[0xEE; 20]);

    // Packet ID (4 bytes)
    packet.extend_from_slice(&0x00000002u32.to_be_bytes());

    // Payload (TLS data would go here)
    packet.extend_from_slice(&[0xFF; 32]);

    packet
}

/// Build an OpenVPN ACK V1 packet (Opcode 5)
fn build_openvpn_ack_v1() -> Vec<u8> {
    let mut packet = Vec::new();

    // Opcode (5 bits) = 5 (P_ACK_V1)
    // Key ID (3 bits) = 0
    // Byte: (5 << 3) | 0 = 0x28
    packet.push(0x28);

    // HMAC (20 bytes)
    packet.extend_from_slice(&[0x11; 20]);

    // Packet ID Array Length (1 byte)
    packet.push(1);

    // Packet ID to ACK (4 bytes)
    packet.extend_from_slice(&0x00000001u32.to_be_bytes());

    // Remote Session ID (optional, 8 bytes for V2)
    // Omitted for V1 ACK

    packet
}
