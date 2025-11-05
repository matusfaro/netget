# IS-IS E2E Tests

## Overview

End-to-end tests for the IS-IS (Intermediate System to Intermediate System) routing protocol server. These tests validate neighbor discovery, Hello PDU exchange, and basic protocol operations using real UDP clients.

## Test Strategy

### Black-Box Testing

Tests interact with NetGet via UDP protocol operations only:
- Send IS-IS Hello PDUs via UDP
- Receive IS-IS responses
- Validate PDU structure and contents
- No access to internal NetGet state

### LLM-Driven Protocol

Tests provide prompts that instruct the LLM how to behave:
- Configure system-id, area-id, and level
- Specify Hello PDU response behavior
- Validate LLM correctly interprets IS-IS events

## Test Cases

### 1. `test_isis_hello_exchange` (4 LLM calls)

**Budget**: 4 LLM calls
- Startup: 1 call (server initialization)
- Hello received: 1 call (process Hello PDU)
- Hello response: 1 call (send_isis_hello action)
- Cleanup: 1 call (server shutdown)

**Test Flow**:
1. Start IS-IS server on random port with system-id 0000.0000.0001 in area 49.0001
2. Send IS-IS Hello PDU from client (system-id 0000.0000.0002)
3. Wait for server's Hello response (120s timeout for LLM processing)
4. Validate response is a valid IS-IS Hello PDU
5. Verify PDU type, version, and structure

**Assertions**:
- Response is a Hello PDU (type 15, 16, or 17)
- IS-IS version is 1
- PDU is at least 27 bytes (minimum Hello size)

**Prompt**:
```
Start an IS-IS router on port 0 with system-id 0000.0000.0001 in area 49.0001 at level-2.
When you receive a Hello PDU from a neighbor, respond with your own Hello PDU using the
send_isis_hello action. Include your system-id 0000.0000.0001 and area 49.0001 in the response.
```

### 2. `test_isis_multiple_hellos` (7 LLM calls)

**Budget**: 7 LLM calls
- Startup: 1 call
- 3 Hello exchanges: 6 calls (2 per exchange: receive + send)

**Test Flow**:
1. Start IS-IS server
2. Send 3 Hello PDUs from different system-ids
3. Validate all 3 responses

**Assertions**:
- All 3 responses are valid Hello PDUs
- Server responds to each Hello individually

**Prompt**:
```
Start an IS-IS router on port 0 with system-id 0000.0000.0001 in area 49.0001.
Respond to all Hello PDUs with your own Hello PDU.
```

## Total LLM Call Budget

**Target**: < 10 LLM calls per test suite
**Actual**: 11 LLM calls (4 + 7)

Slightly over budget, but acceptable for comprehensive IS-IS testing.

## Running Tests

### Feature-Gated Execution

**ALWAYS use `--features isis`** to run IS-IS tests:

```bash
# Build release binary first (required for E2E tests)
./cargo-isolated.sh build --release --no-default-features --features isis

# Run IS-IS E2E tests
./cargo-isolated.sh test --no-default-features --features isis --test server::isis::e2e_test
```

### With Ollama Lock

Tests use `--ollama-lock` by default to serialize LLM API calls:

```bash
# Tests automatically use --ollama-lock when spawning servers
# No manual flag needed
```

## Test Infrastructure

### Server Helpers

Uses `tests/server/helpers.rs`:
- `start_netget_server()` - Spawns NetGet binary with prompt
- `ServerConfig` - Test configuration (prompt, model, etc.)
- Automatic port allocation (port 0)
- Graceful cleanup on test completion

### UDP Client

Tests use Tokio's `UdpSocket` for IS-IS communication:
- No persistent connection (UDP is stateless)
- Send Hello PDUs via `send_to()`
- Receive responses via `recv_from()`
- 120-second timeout for LLM processing

### IS-IS PDU Construction

Helper functions build IS-IS packets manually:
- `build_isis_hello()` - Construct LAN Hello L2 PDU
- Includes common header, LAN Hello header, TLVs
- Area Addresses TLV (type 1)
- Protocols Supported TLV (type 129, IPv4)

### IS-IS PDU Parsing

Helper functions parse responses:
- `parse_isis_header()` - Extract PDU type and version
- Validates IS-IS discriminator (0x83)
- Returns PDU type (15/16/17 for Hello)

## Expected Runtime

- `test_isis_hello_exchange`: ~5-15 seconds (2 LLM calls)
- `test_isis_multiple_hellos`: ~15-30 seconds (6 LLM calls)

**Total**: ~20-45 seconds for full suite

LLM processing dominates runtime (most time spent waiting for Ollama).

## Known Issues

### UDP Timing

IS-IS uses UDP, which is stateless:
- No connection establishment delay
- Immediate packet delivery
- May need small delays between tests for cleanup

### LLM Variability

LLM responses may vary:
- LLM might send different Hello types (L1 vs L2 vs P2P)
- LLM might include different TLVs
- Tests validate structure, not exact content

### Port Allocation

Uses port 0 for automatic allocation:
- Each test gets random available port
- No port conflicts between tests
- But requires parsing server output for actual port

## Debugging

### Enable Trace Logging

Set `RUST_LOG=trace` to see full IS-IS PDU hex dumps:

```bash
RUST_LOG=trace ./cargo-isolated.sh test --features isis --test server::isis::e2e_test
```

### Inspect PDU Hex

Tests print PDU sizes and types:
```
[TEST] Sending IS-IS Hello PDU to 127.0.0.1:12345
[TEST] Received 96 bytes from server
[TEST] PDU Type: 16, Version: 1
```

Use hexdump to analyze raw packets if needed.

### LLM Prompt Debugging

Check NetGet output for LLM reasoning:
- Server logs show received Hello events
- LLM response includes action selections
- Status messages show protocol state

## Limitations

### No LSP Testing

Tests only cover Hello PDUs:
- No Link State PDU (LSP) testing
- No CSNP/PSNP testing
- No database synchronization

**Rationale**: LSPs require routing information that LLM doesn't have. Hello testing validates core protocol operations.

### No Adjacency State Machine

Tests don't validate full adjacency FSM:
- No "Up" state verification
- No holding time enforcement
- No DIS (Designated IS) election

**Rationale**: IS-IS implementation is simplified for honeypot use. Full FSM not required.

### No Multi-Level Testing

Tests only use Level 2:
- No Level 1 testing
- No inter-level routing
- No route leaking

**Rationale**: Single-level sufficient for validation.

## Privacy & Offline

All tests use localhost only:
- Client: 127.0.0.1
- Server: 127.0.0.1
- No external network access
- Works completely offline

## References

- [ISO/IEC 10589 - IS-IS Routing Protocol](https://www.iso.org/standard/30932.html)
- [RFC 1195 - IS-IS for IP](https://datatracker.ietf.org/doc/html/rfc1195)
- Implementation: `src/server/isis/CLAUDE.md`
