# WireGuard E2E Tests

## Test Overview

Tests WireGuard honeypot functionality by sending crafted WireGuard handshake packets to NetGet. **Note**: These tests
currently treat WireGuard as a honeypot (packet detection only), not a full VPN server.

**Important**: The actual WireGuard implementation in `src/server/wireguard/mod.rs` is a **full VPN server** with tunnel
support. These tests, however, focus on packet detection without requiring elevated privileges.

## Test Strategy

### Honeypot Testing Approach

Tests send raw WireGuard packets and verify server logs them:

- **No actual VPN tunnel**: Tests don't establish connections through defguard_wireguard_rs
- **Packet detection only**: Verify server receives and logs WireGuard packets
- **No privilege requirement**: Honeypot mode doesn't require root/admin

### Why Not Full VPN Testing?

Full VPN server testing would require:

- Root/admin privileges for TUN interface creation
- Real WireGuard client (wg, wg-quick, or wireguard-go)
- Network configuration (routing, IP forwarding)
- Cross-platform testing (different TUN APIs per OS)

**Decision**: E2E tests focus on packet detection. Manual testing required for full VPN functionality.

## LLM Call Budget

### Per-Test Breakdown

1. **test_wireguard_handshake_detection**: 1 LLM call
    - Server startup (prompt interpretation)
    - No LLM calls for packet handling (honeypot mode)

2. **test_wireguard_multiple_packet_types**: 1 LLM call
    - Server startup only

3. **test_wireguard_concurrent_connections**: 1 LLM call
    - Server startup only

**Total: 3 LLM calls** (well under 10 limit)

### Why So Few Calls?

Honeypot mode doesn't invoke LLM for each packet - just logs them. Full VPN mode would require LLM calls for peer
authorization decisions.

## Scripting Usage

**Scripting: Not applicable** - Honeypot mode doesn't use scripting. Packets are logged directly without LLM
interpretation.

## Client Library

### Manual Packet Construction

**Why manual**: No Rust WireGuard client library suitable for testing. Tests build raw packets:

```rust
fn build_wireguard_handshake_init() -> Vec<u8> {
    let mut packet = Vec::new();
    packet.push(1);  // Message Type: Handshake Initiation
    packet.extend_from_slice(&[0x00, 0x00, 0x00]);  // Reserved
    packet.extend_from_slice(&0x12345678u32.to_le_bytes());  // Sender Index
    packet.extend_from_slice(&[0xAA; 32]);  // Unencrypted Ephemeral
    packet.extend_from_slice(&[0xBB; 48]);  // Encrypted Static
    packet.extend_from_slice(&[0xCC; 28]);  // Encrypted Timestamp
    packet.extend_from_slice(&[0xDD; 16]);  // MAC1
    packet.extend_from_slice(&[0xEE; 16]);  // MAC2
    packet
}
```

**Packet types**:

- **Handshake Initiation** (Type 1): 148 bytes
- **Handshake Response** (Type 2): 92 bytes
- **Data** (Type 4): Variable length (minimum 32 bytes)

### WireGuard Packet Format

From [WireGuard Protocol](https://www.wireguard.com/protocol/):

**Handshake Initiation**:

```
| Type (1) | Reserved (3) | Sender Index (4) |
| Unencrypted Ephemeral (32) |
| Encrypted Static (48) |
| Encrypted Timestamp (28) |
| MAC1 (16) | MAC2 (16) |
```

**Handshake Response**:

```
| Type (2) | Reserved (3) | Sender Index (4) | Receiver Index (4) |
| Unencrypted Ephemeral (32) |
| Encrypted Nothing (16) |
| MAC1 (16) | MAC2 (16) |
```

**Data**:

```
| Type (4) | Reserved (3) | Receiver Index (4) |
| Counter (8) |
| Encrypted Data (variable) |
```

## Expected Runtime

**Model**: qwen3-coder:30b (or configured model)
**Runtime**: ~10-15 seconds for full test suite
**Breakdown**:

- Server startup: 2-5 seconds per test
- Packet sending: <1 second per test
- LLM calls: 2-3 seconds each (startup only)

**Fast because**: No LLM calls for packet handling, only server startup.

## Failure Rate

**Low** (<5%) - Occasional timeout if Ollama is slow on startup.

**Stable tests** - No flakiness, packet detection is deterministic.

## Test Cases

### 1. test_wireguard_handshake_detection

**What it tests**:

- Server starts with WireGuard stack
- Sends Handshake Initiation packet
- Verifies packet detected in logs

**Assertions**:

```rust
assert!(output_contains_wg, "Server should be running WireGuard stack");
assert!(has_wireguard, "Server output should contain WireGuard handshake detection");
```

**Expected output**:

```
[INFO] Starting WireGuard VPN server on 0.0.0.0:XXXXX
[TRACE] WireGuard: Handshake Initiation packet from 127.0.0.1:XXXXX
```

### 2. test_wireguard_multiple_packet_types

**What it tests**:

- Sends three packet types: Handshake Init, Handshake Response, Data
- Verifies all packets logged

**Packet sequence**:

1. Handshake Initiation (Type 1)
2. Handshake Response (Type 2)
3. Data (Type 4)

**Expected behavior**: Server logs all packet types without crashing.

### 3. test_wireguard_concurrent_connections

**What it tests**:

- Three concurrent clients send handshakes
- Verifies server handles concurrent UDP packets

**Concurrency**: Uses tokio::spawn for parallel packet sends.

**Expected behavior**: No packet loss, all handshakes logged.

## Known Issues

### Honeypot vs. Full Server Mismatch

**Issue**: Tests treat WireGuard as honeypot, but implementation is full server.

**Why**: Full server testing requires:

- Root/admin privileges (TUN interface)
- Platform-specific setup (different per OS)
- Complex network configuration

**Solution**: Tests focus on packet detection. Manual testing for full VPN:

```bash
# Manual testing workflow (requires root):
sudo ./cargo-isolated.sh run --release --all-features -- "start wireguard vpn on port 51820"

# In separate terminal, configure WireGuard client:
sudo wg-quick up /path/to/client.conf
```

### No Peer Authorization Testing

**Issue**: Tests don't verify LLM peer authorization logic.

**Why**: Authorization requires full VPN mode with TUN interface.

**Future work**: Add privileged E2E tests that:

1. Start server with root
2. Connect real WireGuard client
3. Verify LLM authorizes peer
4. Test actual tunnel traffic

### UDP Socket Limitations

**Issue**: Standard UDP client cannot establish WireGuard tunnel.

**Why**: WireGuard requires crypto handshake with valid keys.

**Acceptable**: Honeypot tests verify packet detection, which is sufficient for current scope.

## Running Tests

### Prerequisites

```bash
# Build release binary with all features
./cargo-isolated.sh build --release --all-features
```

### Run Tests

```bash
# Run WireGuard E2E tests
./cargo-isolated.sh test --features wireguard --test server::wireguard::e2e_test

# Run with output
./cargo-isolated.sh test --features wireguard --test server::wireguard::e2e_test -- --nocapture

# Run specific test
./cargo-isolated.sh test --features wireguard --test server::wireguard::e2e_test -- test_wireguard_handshake_detection
```

### Expected Output

```
running 3 tests
test test_wireguard_handshake_detection ... ok
test test_wireguard_multiple_packet_types ... ok
test test_wireguard_concurrent_connections ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 12.34s
```

## Future Improvements

### Privileged Tests

Create separate test suite for full VPN testing:

```rust
#[cfg(all(feature = "wireguard", feature = "privileged-tests"))]
mod privileged {
    #[tokio::test]
    async fn test_wireguard_full_tunnel() {
        // Requires root/admin
        // Creates TUN interface
        // Tests actual VPN traffic
    }
}
```

### Real Client Integration

Use wireguard-go or kernel WireGuard as client:

```rust
// Spawn wg-quick or wireguard-go
let client = Command::new("wg-quick")
    .arg("up")
    .arg("./tmp/test-client.conf")
    .spawn()?;
```

### Peer Authorization Tests

Test LLM authorization decisions:

```rust
async fn test_peer_authorization() {
    let server = start_server("authorize peers with allowed_ips 10.20.30.0/24").await;
    // Connect client with public key
    // Verify LLM authorizes with correct allowed_ips
    // Verify client receives VPN IP
}
```

## References

- [WireGuard Protocol Spec](https://www.wireguard.com/protocol/)
- [WireGuard White Paper](https://www.wireguard.com/papers/wireguard.pdf)
- [defguard_wireguard_rs](https://docs.rs/defguard_wireguard_rs/)
- [NetGet WireGuard Implementation](../../../src/server/wireguard/CLAUDE.md)
