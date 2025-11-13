# DataLink Server E2E Tests

## Test Strategy

Black-box E2E tests using the NetGet binary with mock LLM responses. Tests verify that the DataLink server can:
- Start packet capture on network interfaces
- Set up BPF packet filters
- Handle different interface configurations

## LLM Call Budget

**Target:** < 10 calls total per test
**Actual:** 1 call per test (server startup only)

## Tests

1. **test_datalink_arp_capture_with_mocks** (1 LLM call)
   - Server startup on lo0 interface with ARP filter
   - Verifies server can start with BPF filter

2. **test_datalink_custom_protocol_with_mocks** (1 LLM call)
   - Server startup monitoring custom EtherType (0x88B5)
   - Verifies BPF filter for custom protocols

3. **test_datalink_ignore_packet_with_mocks** (1 LLM call)
   - Server startup to ignore IPv6 packets
   - Verifies server configuration handling

## Runtime

**Expected:** < 15 seconds total (with mocks)
**With real Ollama:** 30-45 seconds (add 5-10s per LLM call)

## Mock Mode vs Real Mode

### Mock Mode (Default)
- Tests verify LLM action generation for server startup
- No actual packet capture
- No root privileges required
- Fast execution (~3-5s per test)

### Real Mode (--use-ollama flag)
- Requires real Ollama running
- Still no actual packet capture (would require root)
- Tests LLM's ability to construct proper `open_server` actions
- Slower execution (~15-30s per test)

## Known Limitations

1. **No Real Packet Capture**: Tests don't actually capture packets (would require root)
2. **No Event Testing**: Tests don't simulate `datalink_packet_captured` events
3. **Interface Availability**: Tests assume lo0 interface exists (common on macOS/Linux)

## Why Only Server Startup?

DataLink packet capture events (`datalink_packet_captured`) are not tested because:
1. Would require root privileges for promiscuous mode
2. Would need actual network traffic to trigger events
3. CI environments typically lack necessary permissions
4. Tests would be flaky (depend on network conditions)

Instead, these tests validate:
- Server startup action parsing
- Interface and filter parameter handling
- Protocol registration and initialization
- Basic server lifecycle

For testing packet analysis with real LLM:
- Use manual testing with real network interfaces
- Run with `sudo` and actual traffic generation
- See manual testing guide in `src/server/datalink/CLAUDE.md`

## Running Tests

```bash
# With mocks (default, fast)
./test-e2e.sh datalink

# With real Ollama
./test-e2e.sh --use-ollama datalink

# Or with cargo
cargo test --features datalink --test server -- datalink_server_tests
cargo test --features datalink --test server -- datalink_server_tests --use-ollama
```

## Test Design Rationale

Unlike TCP/HTTP protocols where tests can simulate full request-response cycles:
- DataLink requires kernel-level packet capture (root access)
- Packet events depend on actual network traffic
- Mock mode can only test action generation, not execution

These tests focus on what can be reliably tested:
- Server startup with various configurations
- LLM action generation for server creation
- Parameter validation (interface names, BPF filters)
