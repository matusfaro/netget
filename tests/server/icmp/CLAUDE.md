# ICMP Server E2E Test Strategy

## Overview

Testing the ICMP server presents unique challenges due to raw socket requirements and privilege constraints.

## Privilege Requirements

**CRITICAL**: ICMP server tests require `CAP_NET_RAW` or root access.

- Tests are marked with `#[ignore]` by default
- Cannot run in unprivileged environments (including Claude Code for Web)
- Must explicitly enable with `cargo test -- --ignored --test-threads=100`

## Test Approach

### Option 1: Mock-Based Testing (Recommended)
Use `.with_mock()` pattern to verify LLM integration without raw sockets:

```rust
#[tokio::test]
async fn test_icmp_echo_request_with_mocks() {
    let config = NetGetConfig::new("Listen for ICMP echo requests")
        .with_mock(|mock| {
            mock
                .on_event("icmp_echo_request")
                .and_event_data_contains("source_ip", "127.0.0.1")
                .respond_with_actions_from_event(|event_data| {
                    let identifier = event_data["identifier"].as_u64().unwrap();
                    let sequence = event_data["sequence"].as_u64().unwrap();
                    let payload_hex = event_data["payload_hex"].as_str().unwrap();

                    serde_json::json!([{
                        "type": "send_echo_reply",
                        "source_ip": "127.0.0.1",
                        "destination_ip": "127.0.0.1",
                        "identifier": identifier,
                        "sequence": sequence,
                        "payload_hex": payload_hex
                    }])
                })
                .expect_calls(1)
                .and()
        });

    // Mock mode testing verifies action generation without raw sockets
    let server = config.start_server("icmp", "127.0.0.1:0", Some(json!({"interface": "eth0"}))).await?;

    // Simulate echo request event
    // ... test logic ...

    server.verify_mocks().await?;
}
```

### Option 2: Real Socket Testing (Privileged)
Requires root/CAP_NET_RAW:

```rust
#[tokio::test]
#[ignore] // Run with: cargo test -- --ignored --test-threads=100
async fn test_icmp_echo_real_socket() {
    // Check for privileges
    if !has_raw_socket_capability() {
        eprintln!("Skipping test - requires CAP_NET_RAW or root");
        return;
    }

    // Start ICMP server with real socket
    // Send ping using pnet or external ping tool
    // Verify reply
}
```

## Test Scenarios

### Scenario 1: Echo Request → Echo Reply
- **Input**: ICMP Echo Request (type 8)
- **LLM Action**: `send_echo_reply`
- **Verification**: Reply has matching identifier, sequence, payload

### Scenario 2: Timestamp Request → Timestamp Reply
- **Input**: ICMP Timestamp Request (type 13)
- **LLM Action**: `send_timestamp_reply`
- **Verification**: Reply includes originate/receive/transmit timestamps

### Scenario 3: Ignore Packet
- **Input**: ICMP Echo Request
- **LLM Action**: `ignore_icmp`
- **Verification**: No reply sent

### Scenario 4: Destination Unreachable
- **Input**: Generic ICMP message
- **LLM Action**: `send_destination_unreachable`
- **Verification**: Unreachable message sent with correct code

## LLM Call Budget

Target: < 5 calls per test suite
- Echo request/reply: 1 call
- Timestamp request/reply: 1 call
- Ignore test: 1 call
- Destination unreachable: 1 call
- Time exceeded: 1 call

Total: 5 LLM calls (within budget)

## Runtime

Expected: < 30 seconds for full suite (with mocks)
- Mock tests: instant (no real network I/O)
- Real socket tests: ~5-10 seconds (with --ignored flag)

## Known Issues

1. **Kernel ICMP Handling**: Linux kernel may intercept Echo Requests
   - Workaround: Use non-standard ICMP types for testing
   - Alternative: Disable kernel ICMP with `sysctl -w net.ipv4.icmp_echo_ignore_all=1`

2. **Firewall Interference**: Firewall rules may block ICMP
   - Ensure loopback ICMP is allowed
   - Use `iptables -L` to check rules

3. **Privilege Escalation**: Cannot elevate privileges in test
   - Must run entire test suite with appropriate permissions
   - Use `sudo -E cargo test` or `setcap cap_net_raw+ep`

## Future Enhancements

1. **Privilege Detection**: Auto-skip tests without CAP_NET_RAW
2. **Mock Packet Injection**: Simulate raw socket reads without kernel
3. **Docker Test Container**: Isolated environment with raw socket access
4. **IPv6 Testing**: ICMPv6 echo request/reply

## References

- RFC 792 - ICMP specification
- Similar tests: `tests/server/arp/e2e_test.rs` (also requires raw sockets)
- Mock pattern: `tests/server/dns/e2e_test.rs`
