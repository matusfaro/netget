# ICMP Client E2E Test Strategy

## Overview

Testing the ICMP client presents unique challenges due to raw socket requirements and external server dependencies.

## Privilege Requirements

**CRITICAL**: ICMP client tests require `CAP_NET_RAW` or root access.

- Tests are marked with `#[ignore]` by default
- Cannot run in unprivileged environments (including Claude Code for Web)
- Must explicitly enable with `cargo test -- --ignored --test-threads=100`

## Test Approach

### Option 1: Action Definition Testing (No Privileges Required)

Test the client trait implementation without creating raw sockets:

```rust
#[tokio::test]
async fn test_icmp_client_actions() {
    let protocol = IcmpClientProtocol;
    let sync_actions = protocol.get_sync_actions();

    // Verify action definitions
    assert!(sync_actions.iter().any(|a| a.name == "send_echo_request"));
    assert!(sync_actions.iter().any(|a| a.name == "send_timestamp_request"));
}
```

### Option 2: Real Socket Testing (Privileged)

Requires root/CAP_NET_RAW:

```rust
#[tokio::test]
#[ignore] // Run with: cargo test -- --ignored --test-threads=100
async fn test_icmp_echo_request() {
    // Check for privileges
    if !has_raw_socket_capability() {
        eprintln!("Skipping test - requires CAP_NET_RAW or root");
        return;
    }

    // Create ICMP client
    let client = create_test_client("Send echo request to 8.8.8.8").await?;

    // Wait for reply event
    // Verify RTT calculation
    // Verify LLM action generation
}
```

## Test Scenarios

### Scenario 1: Echo Request → Echo Reply (Ping)

- **Action**: `send_echo_request` to 8.8.8.8 or localhost
- **Expected Event**: `icmp_echo_reply_received` with matching identifier/sequence
- **Verification**: RTT calculation, payload matching

### Scenario 2: Timestamp Request → Timestamp Reply

- **Action**: `send_timestamp_request` to localhost
- **Expected Event**: `icmp_timestamp_reply_received`
- **Verification**: Timestamp values (originate, receive, transmit)

### Scenario 3: Traceroute Simulation

- **Action**: Multiple `send_echo_request` with increasing TTL
- **Expected Events**:
  - `icmp_time_exceeded` from intermediate hops
  - `icmp_echo_reply_received` from destination
- **Verification**: Hop tracking, RTT per hop

### Scenario 4: Destination Unreachable

- **Action**: `send_echo_request` to unreachable IP (e.g., 192.0.2.1)
- **Expected Event**: `icmp_destination_unreachable`
- **Verification**: Unreachable code (network, host, protocol)

## LLM Call Budget

Target: < 5 calls per test suite

- Echo request/reply: 1 call
- Timestamp request/reply: 1 call
- Traceroute simulation: 2-3 calls (multiple hops)
- Destination unreachable: 1 call

Total: ~5 LLM calls (within budget)

## Runtime

Expected: < 30 seconds for full suite

- Action definition tests: instant (no network I/O)
- Real socket tests: ~10-20 seconds (with --ignored flag)
  - 8.8.8.8 RTT: ~20-50ms
  - Localhost RTT: <1ms
  - Traceroute: ~500ms per hop

## Known Issues

1. **Kernel ICMP Echo Handling**: Linux kernel may intercept Echo Replies
   - Workaround: Use non-localhost destinations for echo tests
   - Alternative: Test with timestamp requests (less common, not intercepted)

2. **Firewall Interference**: Firewall rules may block ICMP
   - Ensure outbound ICMP is allowed
   - Use `iptables -L OUTPUT` to check rules

3. **Network Dependency**: Tests require network connectivity
   - Use localhost (127.0.0.1) where possible
   - Public IPs (8.8.8.8) may fail in isolated environments

4. **Privilege Escalation**: Cannot elevate privileges in test
   - Must run entire test suite with appropriate permissions
   - Use `sudo -E cargo test` or `setcap cap_net_raw+ep`

5. **Non-deterministic Timing**: RTT varies based on network conditions
   - Use tolerance ranges in assertions (e.g., RTT < 100ms)
   - Retry transient failures

## Test Isolation

- **Port Conflicts**: N/A (ICMP is connectionless, no ports)
- **Parallel Execution**: Safe with `--test-threads=100`
  - Each test uses unique identifier values
  - No shared state between tests

## Future Enhancements

1. **Privilege Detection**: Auto-skip tests without CAP_NET_RAW
2. **Mock Network Layer**: Simulate ICMP replies without kernel
3. **Docker Test Container**: Isolated environment with raw socket access
4. **IPv6 Testing**: ICMPv6 echo request/reply
5. **LLM Mock Mode**: Pre-programmed action responses for deterministic testing

## References

- RFC 792 - ICMP specification
- Similar tests: `tests/client/tcp/e2e_test.rs` (socket-based client)
- Privilege handling: `tests/server/arp/e2e_test.rs` (also requires raw sockets)
