# OpenVPN Honeypot E2E Tests

## Test Overview

Tests OpenVPN honeypot functionality by sending crafted OpenVPN handshake packets to NetGet and verifying reconnaissance detection.

**Protocol Status**: Honeypot-only (no actual VPN tunnels)
**Test Focus**: Packet detection and logging

## Test Strategy

### Consolidated Test Suite

Tests reuse single server instances across multiple scenarios:
- **4 test functions** covering V1/V2 handshakes, multiple packet types, concurrency
- Each test spawns server, sends packets, verifies detection

### Packet-Level Testing

No actual OpenVPN clients used - tests construct raw UDP packets:
- **V1 handshakes**: Opcode 1 (P_CONTROL_HARD_RESET_CLIENT_V1)
- **V2 handshakes**: Opcode 7 (P_CONTROL_HARD_RESET_CLIENT_V2)
- **Control packets**: Opcode 4 (P_CONTROL_V1)
- **ACK packets**: Opcode 5 (P_ACK_V1)

### Disabled Protocol Flag

OpenVPN is disabled by default. Tests use `--include-disabled-protocols`:
```rust
let config = ServerConfig::new(prompt)
    .with_include_disabled_protocols(true);
```

## LLM Call Budget

### Per-Test Breakdown

1. **test_openvpn_handshake_detection_v2**: 1 LLM call
   - Server startup (prompt interpretation)

2. **test_openvpn_handshake_detection_v1**: 1 LLM call
   - Server startup

3. **test_openvpn_multiple_packet_types**: 1 LLM call
   - Server startup (handles 3 packet types without additional calls)

4. **test_openvpn_concurrent_connections**: 1 LLM call
   - Server startup (3 concurrent clients, no LLM per-client)

**Total: 4 LLM calls** (well under 10 limit)

### Why So Few Calls?

Honeypot mode logs packets without LLM interpretation. LLM only consulted on startup for server configuration.

## Scripting Usage

**Scripting: Not applicable** - Honeypot doesn't use scripting. Packets logged directly to output.

## Client Library

### Manual Packet Construction

**Why manual**: No Rust OpenVPN client library. OpenVPN protocol is too complex for test libraries.

### OpenVPN Packet Format

**Header structure**:
```
Byte 0: [Opcode (5 bits) | Key ID (3 bits)]
```

**V2 Hard Reset Client**:
```
| Opcode/KeyID (1) | Session ID (8) | HMAC (20) | Packet ID (4) | Payload (variable) |
```

**V1 Hard Reset Client**:
```
| Opcode/KeyID (1) | HMAC (20) | Packet ID (4) | Payload (variable) |
```

### Packet Builders

```rust
fn build_openvpn_hard_reset_client_v2() -> Vec<u8> {
    let mut packet = Vec::new();
    packet.push(0x38);  // Opcode 7, Key ID 0
    packet.extend_from_slice(&0x0123456789ABCDEFu64.to_be_bytes());  // Session ID
    packet.extend_from_slice(&[0xAA; 20]);  // HMAC
    packet.extend_from_slice(&0x00000001u32.to_be_bytes());  // Packet ID
    packet.extend_from_slice(&[0xBB; 16]);  // Payload
    packet
}
```

## Expected Runtime

**Model**: qwen3-coder:30b (or configured model)
**Runtime**: ~15-20 seconds for full test suite
**Breakdown**:
- Server startup: 2-5 seconds per test (4 tests)
- Packet sending: <1 second per test
- LLM calls: 2-3 seconds each (startup only)

**Fast because**: No LLM calls for packet handling.

## Failure Rate

**Low** (<5%) - Occasional timeout if Ollama is slow.

**Stable tests** - Packet detection is deterministic, no flakiness.

## Test Cases

### 1. test_openvpn_handshake_detection_v2

**What it tests**:
- Server starts with OpenVPN stack
- Sends V2 Hard Reset Client packet (opcode 7)
- Verifies handshake detected in logs

**Packet structure**: 45 bytes (1 + 8 + 20 + 4 + 16)

**Assertions**:
```rust
assert_stack_name(&server, "OPENVPN");
assert!(output.contains("OpenVPN") || output.contains("handshake"));
```

**Expected output**:
```
[INFO] Starting OpenVPN honeypot on 0.0.0.0:XXXXX (reconnaissance detection only)
[TRACE] OpenVPN: ControlHardResetClientV2 packet from 127.0.0.1:XXXXX (45 bytes)
[INFO] OpenVPN: Handshake reconnaissance from 127.0.0.1:XXXXX
```

### 2. test_openvpn_handshake_detection_v1

**What it tests**:
- Sends V1 Hard Reset Client packet (opcode 1)
- Verifies V1 handshake detected

**Packet structure**: 41 bytes (1 + 20 + 4 + 16, no session ID)

**Expected output**:
```
[TRACE] OpenVPN: ControlHardResetClientV1 packet from 127.0.0.1:XXXXX (41 bytes)
[INFO] OpenVPN: Handshake reconnaissance from 127.0.0.1:XXXXX
```

### 3. test_openvpn_multiple_packet_types

**What it tests**:
- Sends 3 different packet types:
  1. Hard Reset V2 (opcode 7)
  2. Control V1 (opcode 4)
  3. ACK V1 (opcode 5)
- Verifies all packets logged

**Expected behavior**: Server logs all packet types without crashing.

**Expected output**:
```
[TRACE] OpenVPN: ControlHardResetClientV2 packet from 127.0.0.1:XXXXX
[TRACE] OpenVPN: ControlV1 packet from 127.0.0.1:XXXXX
[TRACE] OpenVPN: AckV1 packet from 127.0.0.1:XXXXX
[DEBUG] OpenVPN ControlV1 from 127.0.0.1:XXXXX (logged)
[DEBUG] OpenVPN AckV1 from 127.0.0.1:XXXXX (logged)
```

### 4. test_openvpn_concurrent_connections

**What it tests**:
- Three concurrent clients send V2 handshakes
- Verifies honeypot handles concurrent UDP packets

**Concurrency**: Uses tokio::spawn for parallel sends.

**Expected behavior**: All handshakes logged, no packet loss.

## Known Issues

### No TLS Handshake Testing

**Issue**: Tests don't verify TLS handshake (control channel establishment).

**Why**: Full OpenVPN handshake requires:
- TLS library integration
- Certificate exchange
- Cipher negotiation
- Authentication

**Acceptable**: Honeypot only detects packets, doesn't establish tunnels.

### No Client Authentication

**Issue**: Tests don't verify client authentication logic.

**Why**: Authentication requires full OpenVPN server (not implemented).

**Future**: If full server ever implemented, add auth tests.

### Session ID Validation

**Issue**: Tests use fake session IDs (not cryptographically valid).

**Why**: Honeypot doesn't validate session IDs, just logs them.

**Acceptable**: Reconnaissance detection doesn't need valid crypto.

### UDP-Only Testing

**Issue**: OpenVPN can run on TCP, tests only cover UDP.

**Why**: UDP is more common for VPN, TCP adds complexity without benefit for honeypot.

**Future**: Add TCP honeypot support if needed.

## Running Tests

### Prerequisites

```bash
# Build release binary with all features
cargo build --release --all-features
```

### Run Tests

```bash
# Run OpenVPN E2E tests
cargo test --features e2e-tests --test server::openvpn::e2e_test

# Run with output
cargo test --features e2e-tests --test server::openvpn::e2e_test -- --nocapture

# Run specific test
cargo test --features e2e-tests --test server::openvpn::e2e_test -- test_openvpn_handshake_detection_v2
```

### Expected Output

```
running 4 tests
test test_openvpn_handshake_detection_v2 ... ok
test test_openvpn_handshake_detection_v1 ... ok
test test_openvpn_multiple_packet_types ... ok
test test_openvpn_concurrent_connections ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 18.45s
```

## Use Cases

### Security Research

Tests demonstrate honeypot's ability to:
- Detect OpenVPN reconnaissance
- Log handshake attempts
- Identify OpenVPN version (V1 vs V2)
- Track session IDs

### Protocol Analysis

Tests verify:
- Opcode extraction works correctly
- Session ID parsing (V2)
- Packet type detection
- Concurrent packet handling

## Future Improvements

### Full OpenVPN Server (Not Planned)

If full OpenVPN server ever implemented, tests would need:
```rust
#[tokio::test]
async fn test_openvpn_full_tunnel() {
    // Requires OpenVPN library integration
    // Spawn real OpenVPN client
    // Verify TLS handshake
    // Test tunnel traffic
}
```

**Note**: Full implementation is **not planned** - see OPENVPN_RESEARCH.md for why.

### LLM Analysis Tests

Test LLM's ability to analyze handshakes:
```rust
#[tokio::test]
async fn test_llm_handshake_analysis() {
    let server = start_server("analyze and categorize OpenVPN handshakes").await;
    // Send handshakes from different source IPs
    // Verify LLM identifies patterns
}
```

### TCP Honeypot

Add TCP variant:
```rust
#[tokio::test]
async fn test_openvpn_tcp_honeypot() {
    let server = start_server("start openvpn honeypot on tcp port 1194").await;
    // Test TCP-based OpenVPN detection
}
```

## References

- [OpenVPN Protocol](https://openvpn.net/community-resources/openvpn-protocol/)
- [OpenVPN Security Overview](https://openvpn.net/community-resources/openvpn-cryptographic-layer/)
- [OPENVPN_RESEARCH.md](../../../OPENVPN_RESEARCH.md) - Why full implementation is infeasible
- [NetGet OpenVPN Implementation](../../../src/server/openvpn/CLAUDE.md)
