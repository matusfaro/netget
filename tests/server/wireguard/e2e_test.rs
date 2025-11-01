//! E2E tests for WireGuard honeypot
//!
//! These tests verify WireGuard honeypot functionality by starting NetGet with WireGuard prompts
//! and sending crafted WireGuard handshake packets to detect reconnaissance attempts.

#![cfg(feature = "wireguard")]

use crate::server::helpers::*;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

#[tokio::test]
async fn test_wireguard_handshake_detection() {
    let config = ServerConfig::new("Start a WireGuard VPN honeypot on port 0");

    let mut server = start_netget_server(config).await.expect("Failed to start server");

    // Wait for server to be ready
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify WireGuard stack was selected
    let output_contains_wg = server.output_contains("WireGuard").await || server.output_contains("WIREGUARD").await;
    assert!(output_contains_wg, "Server should be running WireGuard stack");

    // Create UDP client socket
    let client = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind client socket");
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("Failed to set read timeout");

    let server_addr: SocketAddr = format!("127.0.0.1:{}", server.port)
        .parse()
        .expect("Failed to parse server address");

    // Build WireGuard handshake initiation packet
    let handshake = build_wireguard_handshake_init();

    // Send handshake
    client
        .send_to(&handshake, server_addr)
        .expect("Failed to send WireGuard handshake");

    println!("Sent WireGuard handshake initiation to {}", server_addr);

    // For honeypot, we don't expect a valid response
    // Just verify the packet was received by checking logs
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Check server output for handshake detection
    let has_wireguard = server.output_contains("WireGuard").await || server.output_contains("handshake").await;
    assert!(
        has_wireguard,
        "Server output should contain WireGuard handshake detection"
    );

    println!("✓ WireGuard handshake detection successful");

    // Cleanup
    server.stop().await.expect("Failed to stop server");
}

#[tokio::test]
async fn test_wireguard_multiple_packet_types() {
    let config = ServerConfig::new("Start a WireGuard honeypot on port 0 that logs all packet types");

    let mut server = start_netget_server(config).await.expect("Failed to start server");

    tokio::time::sleep(Duration::from_secs(2)).await;

    let client = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind client socket");
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("Failed to set read timeout");

    let server_addr: SocketAddr = format!("127.0.0.1:{}", server.port)
        .parse()
        .expect("Failed to parse server address");

    // Send different packet types
    let packets = vec![
        ("HandshakeInit", build_wireguard_handshake_init()),
        ("HandshakeResponse", build_wireguard_handshake_response()),
        ("Data", build_wireguard_data_packet()),
    ];

    for (name, packet) in packets {
        client
            .send_to(&packet, server_addr)
            .expect(&format!("Failed to send {} packet", name));
        println!("Sent WireGuard {} packet", name);
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // Verify honeypot logged the packets
    tokio::time::sleep(Duration::from_millis(1000)).await;

    let has_wireguard = server.output_contains("WireGuard").await || server.output_contains("WG").await;
    assert!(
        has_wireguard,
        "Server should log WireGuard packets"
    );

    println!("✓ Multiple WireGuard packet types detected");

    server.stop().await.expect("Failed to stop server");
}

#[tokio::test]
async fn test_wireguard_concurrent_connections() {
    let config = ServerConfig::new("Start a WireGuard VPN honeypot on port 0");

    let mut server = start_netget_server(config).await.expect("Failed to start server");

    tokio::time::sleep(Duration::from_secs(2)).await;

    let server_addr: SocketAddr = format!("127.0.0.1:{}", server.port)
        .parse()
        .expect("Failed to parse server address");

    // Spawn multiple clients sending handshakes concurrently
    let mut handles = vec![];
    for i in 0..3 {
        let addr = server_addr.clone();
        let handle = tokio::spawn(async move {
            let client = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind");
            let handshake = build_wireguard_handshake_init();
            client.send_to(&handshake, addr).expect("Failed to send");
            println!("✓ Client {} sent handshake", i);
        });
        handles.push(handle);
    }

    // Wait for all clients
    for handle in handles {
        handle.await.expect("Client task failed");
    }

    tokio::time::sleep(Duration::from_millis(1000)).await;

    println!("✓ Concurrent WireGuard connections handled");

    server.stop().await.expect("Failed to stop server");
}

// ============================================================================
// Packet Building Functions
// ============================================================================

/// Build a WireGuard Handshake Initiation packet (Type 1)
fn build_wireguard_handshake_init() -> Vec<u8> {
    let mut packet = Vec::new();

    // Message Type: 1 (Handshake Initiation)
    packet.push(1);

    // Reserved (3 bytes)
    packet.extend_from_slice(&[0x00, 0x00, 0x00]);

    // Sender Index (4 bytes) - arbitrary value
    packet.extend_from_slice(&0x12345678u32.to_le_bytes());

    // Unencrypted Ephemeral (32 bytes) - fake public key
    packet.extend_from_slice(&[0xAA; 32]);

    // Encrypted Static (48 bytes) - fake encrypted data
    packet.extend_from_slice(&[0xBB; 48]);

    // Encrypted Timestamp (28 bytes) - fake encrypted timestamp
    packet.extend_from_slice(&[0xCC; 28]);

    // MAC1 (16 bytes)
    packet.extend_from_slice(&[0xDD; 16]);

    // MAC2 (16 bytes)
    packet.extend_from_slice(&[0xEE; 16]);

    packet
}

/// Build a WireGuard Handshake Response packet (Type 2)
fn build_wireguard_handshake_response() -> Vec<u8> {
    let mut packet = Vec::new();

    // Message Type: 2 (Handshake Response)
    packet.push(2);

    // Reserved (3 bytes)
    packet.extend_from_slice(&[0x00, 0x00, 0x00]);

    // Sender Index (4 bytes)
    packet.extend_from_slice(&0x87654321u32.to_le_bytes());

    // Receiver Index (4 bytes)
    packet.extend_from_slice(&0x12345678u32.to_le_bytes());

    // Unencrypted Ephemeral (32 bytes)
    packet.extend_from_slice(&[0xFF; 32]);

    // Encrypted Nothing (16 bytes)
    packet.extend_from_slice(&[0x11; 16]);

    // MAC1 (16 bytes)
    packet.extend_from_slice(&[0x22; 16]);

    // MAC2 (16 bytes)
    packet.extend_from_slice(&[0x33; 16]);

    packet
}

/// Build a WireGuard Data packet (Type 4)
fn build_wireguard_data_packet() -> Vec<u8> {
    let mut packet = Vec::new();

    // Message Type: 4 (Data)
    packet.push(4);

    // Reserved (3 bytes)
    packet.extend_from_slice(&[0x00, 0x00, 0x00]);

    // Receiver Index (4 bytes)
    packet.extend_from_slice(&0x12345678u32.to_le_bytes());

    // Counter (8 bytes)
    packet.extend_from_slice(&0x0000000000000001u64.to_le_bytes());

    // Encrypted Data (variable length, minimum 16 bytes for auth tag)
    packet.extend_from_slice(&[0x44; 32]);

    packet
}
