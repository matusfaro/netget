use netget::client::icmp::IcmpClientProtocol;
use netget::llm::actions::protocol_trait::Protocol;
use socket2::{Domain, Protocol as SocketProtocol, Socket, Type};

/// Check if we have CAP_NET_RAW or root privileges
fn has_raw_socket_capability() -> bool {
    // Try to create a raw ICMP socket - this will fail without privileges
    Socket::new(Domain::IPV4, Type::RAW, Some(SocketProtocol::ICMPV4)).is_ok()
}

// ICMP client tests require CAP_NET_RAW or root privileges
// Tests will skip gracefully if privileges are not available

#[tokio::test]
async fn test_icmp_echo_request() -> anyhow::Result<()> {
    // Check for raw socket capability
    if !has_raw_socket_capability() {
        println!("⚠ Skipping ICMP client test: requires CAP_NET_RAW or root privileges");
        println!("  Run with: sudo cargo test --features icmp --test client");
        println!("  Or grant capability: sudo setcap cap_net_raw+ep target/debug/deps/client-*");
        return Ok(());
    }

    // TODO: Implement ICMP echo request test
    // 1. Create ICMP client with instruction "Send ping to 8.8.8.8"
    // 2. Connect client
    // 3. Wait for echo reply event
    // 4. Verify RTT calculation
    // 5. Verify LLM action generation

    println!("✓ Raw socket capability detected");
    println!("TODO: Full ICMP client ping test implementation pending");

    Ok(())
}

/* TODO: Timestamp support requires pnet to add timestamp packet types
#[tokio::test]
async fn test_icmp_timestamp_request() -> anyhow::Result<()> {
    if !has_raw_socket_capability() {
        println!("⚠ Skipping ICMP timestamp test: requires CAP_NET_RAW or root privileges");
        return Ok(());
    }

    // TODO: Implement ICMP timestamp request test
    // 1. Create ICMP client with instruction "Send timestamp request to localhost"
    // 2. Connect client
    // 3. Wait for timestamp reply event
    // 4. Verify timestamp values

    Ok(())
}
*/

#[tokio::test]
async fn test_icmp_traceroute_simulation() -> anyhow::Result<()> {
    // Check for raw socket capability
    if !has_raw_socket_capability() {
        println!("⚠ Skipping ICMP traceroute test: requires CAP_NET_RAW or root privileges");
        println!("  Run with: sudo cargo test --features icmp --test client");
        println!("  Or grant capability: sudo setcap cap_net_raw+ep target/debug/deps/client-*");
        return Ok(());
    }

    // TODO: Implement traceroute simulation test
    // 1. Create ICMP client with instruction "Perform traceroute to 8.8.8.8"
    // 2. Send echo requests with increasing TTL
    // 3. Expect time_exceeded or echo_reply events
    // 4. Verify hop tracking

    println!("✓ Raw socket capability detected");
    println!("TODO: Full ICMP traceroute test implementation pending");

    Ok(())
}

#[tokio::test]
async fn test_icmp_client_actions() -> anyhow::Result<()> {
    // Test action definitions (no raw socket needed)
    let protocol = IcmpClientProtocol::new();
    let sync_actions = protocol.get_sync_actions();

    // Verify expected actions exist
    let action_types: Vec<&str> = sync_actions.iter()
        .map(|a| a.name.as_str())
        .collect();

    assert!(action_types.contains(&"send_echo_request"));
    // Note: send_timestamp_request removed - pnet doesn't support timestamp packets
    assert!(action_types.contains(&"wait_for_more"));
    assert!(action_types.contains(&"disconnect"));

    println!("✓ ICMP client action definitions verified");
    println!("  Actions: {:?}", action_types);

    Ok(())
}
