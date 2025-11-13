# DataLink Client E2E Tests

## Test Strategy

Black-box E2E tests using the NetGet binary with mock LLM responses. Tests verify that the DataLink client can:
- Inject raw Ethernet frames onto network interfaces
- Capture frames in promiscuous mode
- Handle frame injection and capture workflows

## LLM Call Budget

**Target:** < 10 calls total per test
**Actual:** 1-3 calls per test (client startup, frame events)

## Tests

1. **test_datalink_client_inject_frame_with_mocks** (2 LLM calls)
   - Client startup to inject ARP frame
   - Frame injected event handling
   - Verifies basic frame injection works

2. **test_datalink_client_promiscuous_capture_with_mocks** (2 LLM calls)
   - Client startup in promiscuous mode
   - Frame captured event handling
   - Verifies frame capture works

3. **test_datalink_client_inject_and_respond_with_mocks** (3 LLM calls)
   - Client sends ARP request
   - Frame injected event
   - Frame captured event (simulated reply)
   - Verifies inject-response pattern

4. **test_datalink_client_disconnect_with_mocks** (2 LLM calls)
   - Client startup
   - Disconnect after frame injection
   - Verifies graceful disconnection

## Runtime

**Expected:** < 30 seconds total (with mocks)
**With real Ollama:** 1-2 minutes (add 5-10s per LLM call)

## Mock Mode vs Real Mode

### Mock Mode (Default)
- Tests verify LLM action generation logic
- No actual network traffic
- No root privileges required
- Fast execution (~5-10s per test)

### Real Mode (--use-ollama flag)
- Requires real Ollama running
- Still no actual network traffic (test mode)
- Tests LLM's ability to construct proper DataLink actions
- Slower execution (~30-60s per test)

## Known Limitations

1. **No Real Frame Injection**: Tests don't actually inject frames (would require root)
2. **No Real Capture**: Tests don't capture real network traffic (would require root and actual traffic)
3. **Interface Availability**: Tests use common interface names (lo0, eth0) that may not exist on all systems

## Running Tests

```bash
# With mocks (default, fast)
./test-e2e.sh datalink

# With real Ollama
./test-e2e.sh --use-ollama datalink

# Or with cargo
cargo test --features datalink --test client -- datalink
cargo test --features datalink --test client -- datalink --use-ollama
```

## Why Not Root Testing?

Running tests with root privileges in CI/CD is:
- Security risk (untrusted code with elevated privileges)
- CI limitation (most CI environments don't provide root)
- Flaky (depends on network interfaces availability)

Instead, these tests validate:
- Mock LLM response handling
- Action parsing and execution logic
- Client startup and teardown flow
- Error handling paths

For manual integration testing with real packet injection, see manual testing guide in `src/client/datalink/CLAUDE.md`.
