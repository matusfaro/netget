# RIP Client E2E Test Strategy

## Overview

End-to-end tests for RIP client that verify routing table queries and response parsing.

## Test Approach

### Unit Tests (No Ollama)

**test_rip_packet_encoding**
- Verifies RIP request packet construction
- Tests RIPv1 and RIPv2 encoding
- Validates packet structure (header + route entries)
- **LLM Calls**: 0
- **Runtime**: <100ms

**test_rip_packet_decoding**
- Verifies RIP response parsing
- Tests route entry extraction
- Validates IP, mask, next hop, metric parsing
- **LLM Calls**: 0
- **Runtime**: <100ms

### E2E Tests (Ollama Required)

**test_rip_client_query** (marked #[ignore])
- Mock RIP router on port 15520 (non-privileged)
- Client queries for routing table
- Router responds with 3 routes
- LLM analyzes routes and decides actions
- **LLM Calls**: 2-3 (connected event, response event, optional follow-up)
- **Runtime**: 10-15 seconds

## Mock RIP Router

Simple UDP server that:
1. Listens on 127.0.0.1:15520
2. Receives RIP Request (command=1, version=1 or 2)
3. Sends RIP Response with 3 hardcoded routes:
   - 10.0.0.0/8 via 192.168.1.254 metric 2
   - 172.16.0.0/16 via 192.168.1.253 metric 5
   - 192.168.2.0/24 via 192.168.1.1 metric 1

**Why not use real router?**
- Requires network access
- May not have RIP enabled
- Mock is faster and more reliable
- Can control exact responses

## LLM Call Budget

**Target**: < 5 LLM calls per test suite

**Actual**:
- Unit tests: 0 calls (packet encoding/decoding)
- E2E test: 2-3 calls (connect, response, optional cleanup)

**Total**: 2-3 calls (well under budget)

## Test Runtime

- **Unit tests**: <200ms
- **E2E test**: 10-15 seconds (Ollama processing)
- **Total**: ~15 seconds

## Running Tests

```bash
# Unit tests only (fast, no Ollama)
./cargo-isolated.sh test --no-default-features --features rip --test client::rip::e2e_test -- --skip test_rip_client_query

# E2E test (requires Ollama)
./cargo-isolated.sh test --no-default-features --features rip --test client::rip::e2e_test test_rip_client_query -- --ignored

# All tests (requires Ollama)
./cargo-isolated.sh test --no-default-features --features rip --test client::rip::e2e_test -- --include-ignored
```

## Known Issues

### Ollama Dependency
E2E test requires Ollama running on localhost:11434 with `qwen3-coder:30b` model. Test is marked `#[ignore]` to skip by default.

### Port Conflicts
Mock router uses port 15520. If port is in use, test will fail. Can be changed by modifying `rip_port` variable.

### UDP Reliability
UDP packets can be dropped. Test includes retries and timeouts to handle occasional packet loss.

### LLM Variability
LLM may choose different actions based on model/temperature. Test verifies general behavior (response received) rather than exact actions.

## Future Improvements

1. **Real Router Testing**: Optional test against actual RIP router (Quagga/FRRouting)
2. **RIPv1 vs RIPv2**: Separate tests for each version
3. **Authentication**: Test RIPv2 MD5 authentication (once implemented)
4. **Multiple Requests**: Test LLM sending follow-up queries
5. **Error Handling**: Test malformed packets, timeouts, router errors

## Success Criteria

- [x] Unit tests pass without Ollama
- [x] E2E test connects to mock router
- [x] E2E test receives RIP response
- [x] E2E test parses routes correctly
- [x] LLM call budget < 5
- [x] Total runtime < 20 seconds

## Test Coverage

**Protocol Features**:
- [x] RIPv1 packet encoding
- [x] RIPv2 packet encoding
- [x] RIP response decoding
- [x] Route entry parsing (IP, mask, next hop, metric)
- [ ] Authentication (RIPv2) - not yet implemented
- [ ] Specific route queries - not supported by protocol

**Client Features**:
- [x] UDP socket binding
- [x] Send RIP request
- [x] Receive RIP response
- [x] LLM integration (connected event)
- [x] LLM integration (response event)
- [x] State machine (Idle/Processing/Accumulating)

**Edge Cases**:
- [ ] Router timeout (no response)
- [ ] Malformed RIP packets
- [ ] Empty routing table
- [ ] Maximum routes (25 per packet)
- [ ] Multiple response packets
