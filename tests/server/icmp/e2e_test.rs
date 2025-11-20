#[cfg(all(test, feature = "icmp"))]
mod tests {
    use socket2::{Domain, Protocol, Socket, Type};

    /// Check if we have CAP_NET_RAW or root privileges
    fn has_raw_socket_capability() -> bool {
        // Try to create a raw ICMP socket - this will fail without privileges
        Socket::new(Domain::IPV4, Type::RAW, Some(Protocol::ICMPV4)).is_ok()
    }

    #[tokio::test]
    async fn test_icmp_echo_server_placeholder() {
        // Check for raw socket capability
        if !has_raw_socket_capability() {
            println!("⚠ Skipping ICMP server test: requires CAP_NET_RAW or root privileges");
            println!("  Run with: sudo cargo test --features icmp --test server");
            println!("  Or grant capability: sudo setcap cap_net_raw+ep target/debug/netget");
            return;
        }

        // TODO: Implement ICMP server E2E test
        // This test requires:
        // 1. Root/CAP_NET_RAW privileges ✓ (checked above)
        // 2. Mock LLM responses for echo requests
        // 3. Raw socket client to send ICMP Echo Request
        // 4. Verification of Echo Reply
        //
        // See tests/server/arp/e2e_test.rs for similar raw socket test pattern
        // See tests/server/dns/e2e_test.rs for mock pattern example

        println!("✓ Raw socket capability detected");
        println!("TODO: Full ICMP server test implementation pending");
    }
}
