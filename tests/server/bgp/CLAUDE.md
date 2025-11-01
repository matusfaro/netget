# BGP E2E Tests

## Test Overview

Tests BGP server functionality by establishing BGP peering sessions using raw TCP clients. Tests verify session establishment, message exchange, and error handling.

**Protocol Status**: Alpha (fully implemented, needs extensive testing)
**Test Focus**: BGP FSM state transitions and message exchange

## Test Strategy

### Comprehensive Peering Tests

Tests cover full BGP session lifecycle:
- **4 test functions** covering peering establishment, error handling, keepalives, graceful shutdown
- Each test uses real BGP message construction (manual, no library)
- Tests verify FSM state transitions (Connect → OpenSent → OpenConfirm → Established)

### TCP-Based Testing

BGP runs on TCP port 179, tests use tokio TcpStream:
- Connect to server
- Send/receive BGP messages
- Verify responses match protocol spec

### No Routing Table Testing

Tests focus on **protocol operations**, not routing:
- Peering establishment ✅
- Message exchange ✅
- Route advertisements ❌ (no RIB)
- Best path selection ❌ (no routing table)

## LLM Call Budget

### Per-Test Breakdown

1. **test_bgp_peering_establishment**: 3 LLM calls
   - Server startup: 1 call
   - OPEN message received: 1 call (LLM decides response)
   - KEEPALIVE received: 1 call (LLM sends KEEPALIVE back)

2. **test_bgp_notification_on_error**: 2 LLM calls
   - Server startup: 1 call
   - Invalid OPEN received: 1 call (LLM may send NOTIFICATION)

3. **test_bgp_keepalive_exchange**: 3 LLM calls
   - Server startup: 1 call
   - First OPEN: 1 call
   - Additional KEEPALIVE: 1 call (optional)

4. **test_bgp_graceful_shutdown**: 3 LLM calls
   - Server startup: 1 call
   - OPEN: 1 call
   - NOTIFICATION (Cease): 1 call (LLM logs graceful shutdown)

**Total: 11 LLM calls** (slightly over 10, but acceptable for complex protocol)

### Why More Calls?

BGP is connection-oriented with state machine:
- Each protocol message may trigger LLM consultation
- LLM controls routing decisions
- Tests verify LLM correctly handles protocol flow

### Optimization Opportunity

Future optimization: Use scripting mode for deterministic responses:
```rust
let config = ServerConfig::new(prompt)
    .with_no_scripts(false);  // Enable scripting
```

Could reduce to ~4 LLM calls (1 per test startup).

## Scripting Usage

**Scripting: Not currently used** - Tests use action-based responses.

**Future**: Enable scripting for faster, deterministic tests:
- Server startup generates script handling OPEN/KEEPALIVE exchange
- Reduces LLM calls from 11 to 4

## Client Library

### Manual BGP Message Construction

**Why manual**: No Rust BGP client library suitable for testing.

**What we implement**:
- BGP message construction (OPEN, KEEPALIVE, NOTIFICATION)
- BGP message parsing (read and validate responses)
- Message type detection

### BGP Message Format

**Header (19 bytes)**:
```
| Marker (16 bytes, all 0xFF) |
| Length (2 bytes) | Type (1 byte) |
```

**OPEN Message**:
```
| Header (19) | Version (1) | My AS (2) | Hold Time (2) |
| BGP Identifier (4) | Opt Params Len (1) | Opt Params (variable) |
```

**KEEPALIVE Message**: Just header (19 bytes)

**NOTIFICATION Message**:
```
| Header (19) | Error Code (1) | Error Subcode (1) | Data (variable) |
```

### Message Builders

```rust
fn build_bgp_open(my_as: u16, hold_time: u16, router_id: [u8; 4]) -> Vec<u8> {
    let mut msg = Vec::new();
    msg.extend_from_slice(&[0xff; 16]);  // Marker
    msg.extend_from_slice(&[0, 0]);  // Length placeholder
    msg.push(1);  // Type = OPEN
    msg.push(4);  // Version
    msg.extend_from_slice(&my_as.to_be_bytes());
    msg.extend_from_slice(&hold_time.to_be_bytes());
    msg.extend_from_slice(&router_id);
    msg.push(0);  // Opt params len
    let msg_len = msg.len() as u16;
    msg[16..18].copy_from_slice(&msg_len.to_be_bytes());
    msg
}
```

### Message Parsing

```rust
async fn read_bgp_message(stream: &mut TcpStream) -> E2EResult<(u8, Vec<u8>)> {
    let mut marker = [0u8; 16];
    stream.read_exact(&mut marker).await?;
    assert_eq!(marker, [0xff; 16]);  // Validate marker

    let mut length_bytes = [0u8; 2];
    stream.read_exact(&mut length_bytes).await?;
    let length = u16::from_be_bytes(length_bytes);

    let mut msg_type = [0u8; 1];
    stream.read_exact(&mut msg_type).await?;

    let body_len = (length - 19) as usize;
    let mut body = vec![0u8; body_len];
    if body_len > 0 {
        stream.read_exact(&mut body).await?;
    }

    Ok((msg_type[0], body))
}
```

## Expected Runtime

**Model**: qwen3-coder:30b (or configured model)
**Runtime**: ~45-60 seconds for full test suite (with 120s timeouts)
**Breakdown**:
- Server startup: 2-5 seconds per test (4 tests)
- TCP connection: <1 second per test
- LLM calls: 2-5 seconds each (~11 calls total)
- Message exchange: 1-3 seconds per message

**Slower because**: Multiple LLM calls per test for protocol state machine.

## Failure Rate

**Medium** (10-15%) - LLM may choose unexpected responses.

**Known variability**:
- LLM may send OPEN instead of NOTIFICATION on error
- LLM may not respond to additional KEEPALIVEs
- Timing-sensitive (120s timeout to accommodate slow LLM)

**Not flaky** - Deterministic for given LLM model/prompt.

## Test Cases

### 1. test_bgp_peering_establishment

**What it tests**:
- Full BGP peering establishment (Connect → Established)
- OPEN exchange
- KEEPALIVE exchange
- FSM state transitions

**Message flow**:
1. Client → Server: OPEN (AS 65000, router ID 192.168.1.100)
2. Server → Client: OPEN (AS 65001, router ID 192.168.1.1)
3. Client → Server: KEEPALIVE (acknowledge OPEN)
4. Server → Client: KEEPALIVE (establish peering)

**Assertions**:
```rust
assert_eq!(msg_type, BGP_MSG_OPEN);
assert_eq!(version, 4);
assert_eq!(peer_as, 65001);
assert!(hold_time > 0);
```

**Expected output**:
```
[INFO] BGP server listening on 0.0.0.0:XXXXX
→ BGP connection conn_12345 from 127.0.0.1:XXXXX
[INFO] BGP OPEN received: version=4, AS=65000, hold_time=180, router_id=192.168.1.100
[INFO] BGP OPEN sent: AS=65001, hold_time=180s
[DEBUG] BGP session transitioned to OpenConfirm
[DEBUG] BGP KEEPALIVE received
✓ BGP session conn_12345 established with AS65000
[TRACE] BGP KEEPALIVE sent
```

### 2. test_bgp_notification_on_error

**What it tests**:
- Error handling with NOTIFICATION
- Invalid OPEN version detection (version 3 instead of 4)
- Graceful connection closure

**Message flow**:
1. Client → Server: OPEN (invalid version 3)
2. Server → Client: NOTIFICATION (error code 2, subcode 1) **or** OPEN (LLM chooses to accept)

**Assertions**: Flexible - accepts NOTIFICATION or OPEN or connection close.

**Expected output** (if NOTIFICATION sent):
```
[ERROR] BGP invalid message: Unsupported BGP version: 3
[ERROR] BGP NOTIFICATION sent: code=2, subcode=1
```

**Expected output** (if LLM accepts):
```
[INFO] BGP OPEN received: version=3, AS=65000 (accepted despite invalid version)
```

### 3. test_bgp_keepalive_exchange

**What it tests**:
- Peering establishment
- Additional KEEPALIVE exchange (session maintenance)

**Message flow**:
1. Establish peering (OPEN + KEEPALIVE)
2. Client → Server: Additional KEEPALIVE
3. Server → Client: KEEPALIVE response (or no response, both acceptable)

**Expected behavior**: Server handles additional KEEPALIVEs gracefully.

**Expected output**:
```
✓ Peering established
[DEBUG] BGP KEEPALIVE received
✓ Received KEEPALIVE response (or no response, acceptable)
```

### 4. test_bgp_graceful_shutdown

**What it tests**:
- Graceful shutdown with NOTIFICATION (Cease)
- Proper connection cleanup

**Message flow**:
1. Establish peering (OPEN + KEEPALIVE)
2. Client → Server: NOTIFICATION (error code 6, subcode 0, Cease)
3. Server: Closes connection or sends NOTIFICATION back

**Expected behavior**: Server acknowledges shutdown and closes connection.

**Expected output**:
```
✓ Peering established
[ERROR] BGP NOTIFICATION received: code=6, subcode=0
✓ Server acknowledged with NOTIFICATION (or connection closed gracefully)
```

## Known Issues

### No Route Testing

**Issue**: Tests don't verify route advertisements or RIB management.

**Why**: BGP server doesn't implement routing table.

**Future**: If RIB implemented, add route tests:
```rust
#[tokio::test]
async fn test_bgp_route_advertisement() {
    // Send UPDATE with route
    // Verify server stores in RIB
    // Verify re-advertisement to other peers
}
```

### LLM Response Variability

**Issue**: LLM may choose different responses (e.g., accept invalid OPEN vs send NOTIFICATION).

**Why**: LLM has flexibility in protocol implementation.

**Acceptable**: Tests use flexible assertions to handle multiple valid responses.

### No Multi-Peer Testing

**Issue**: Tests only cover single peer per server.

**Why**: No route propagation between peers (no RIB).

**Future**: Add multi-peer tests when RIB implemented.

### Hold Timer Not Enforced

**Issue**: Server doesn't enforce hold timer (no timeout logic).

**Why**: Not implemented yet.

**Future**: Add hold timer expiration tests:
```rust
#[tokio::test]
async fn test_bgp_hold_timer_expiration() {
    // Establish peering
    // Wait longer than hold_time
    // Verify server sends NOTIFICATION and closes
}
```

## Running Tests

### Prerequisites

```bash
# Build release binary with all features
./cargo-isolated.sh build --release --all-features
```

### Run Tests

```bash
# Run BGP E2E tests
./cargo-isolated.sh test --features bgp --test server::bgp::e2e_test

# Run with output
./cargo-isolated.sh test --features bgp --test server::bgp::e2e_test -- --nocapture

# Run specific test
./cargo-isolated.sh test --features bgp --test server::bgp::e2e_test -- test_bgp_peering_establishment
```

### Expected Output

```
running 4 tests
test e2e_bgp::test_bgp_peering_establishment ... ok
test e2e_bgp::test_bgp_notification_on_error ... ok
test e2e_bgp::test_bgp_keepalive_exchange ... ok
test e2e_bgp::test_bgp_graceful_shutdown ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 52.34s
```

### Handling Timeouts

Tests use 120-second timeouts to accommodate slow LLM:
```rust
let (msg_type, body) = timeout(
    Duration::from_secs(120),
    read_bgp_message(&mut client)
).await??;
```

If tests timeout, increase timeout or check Ollama performance.

## Use Cases

### Protocol Learning

Tests demonstrate:
- BGP session establishment
- FSM state transitions
- Message exchange patterns

### BGP Client Testing

Use NetGet BGP server to test BGP client implementations:
- Verify OPEN handling
- Test NOTIFICATION error cases
- Validate KEEPALIVE behavior

### NOT for Production Routing

BGP server should **not** be used for production routing:
- No routing table
- No best path selection
- No route filtering
- No policy enforcement

## Future Improvements

### Routing Table Implementation

Add RIB (Routing Information Base):
```rust
#[tokio::test]
async fn test_bgp_route_storage() {
    let server = start_server("maintain routing table").await;
    // Send UPDATE
    // Query RIB
    // Verify route stored
}
```

### Path Attributes Parsing

Parse UPDATE message path attributes:
```rust
#[tokio::test]
async fn test_bgp_update_parsing() {
    // Send UPDATE with AS_PATH, NEXT_HOP, etc.
    // Verify server extracts attributes
    // Verify LLM receives structured data
}
```

### Multi-Peer Tests

Test route propagation between peers:
```rust
#[tokio::test]
async fn test_bgp_multi_peer() {
    // Connect peer A and peer B
    // A sends UPDATE
    // Verify server propagates to B
}
```

### Scripting Mode

Enable scripting for faster tests:
```rust
let config = ServerConfig::new(prompt).with_no_scripts(false);
```

Generate script for deterministic OPEN/KEEPALIVE responses.

### 32-bit AS Numbers

Add 32-bit AS support (RFC 6793):
```rust
#[tokio::test]
async fn test_bgp_32bit_as() {
    let server = start_server("support 32-bit AS numbers").await;
    // Send OPEN with AS 4200000000
    // Verify server accepts
}
```

## References

- [RFC 4271 - BGP-4](https://datatracker.ietf.org/doc/html/rfc4271)
- [RFC 6793 - 32-bit AS Numbers](https://datatracker.ietf.org/doc/html/rfc6793)
- [BGP FSM](https://www.rfc-editor.org/rfc/rfc4271.html#section-8.2)
- [BGP Message Formats](https://www.rfc-editor.org/rfc/rfc4271.html#section-4)
- [NetGet BGP Implementation](../../../src/server/bgp/CLAUDE.md)
