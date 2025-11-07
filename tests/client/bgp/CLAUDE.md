# BGP Client E2E Testing

## Test Strategy

The BGP client tests use a black-box approach by spawning the actual NetGet binary. Tests verify:

1. **Connection**: BGP client can connect to BGP server
2. **Session Establishment**: OPEN handshake completes successfully
3. **Parameter Handling**: Custom AS numbers and router IDs work correctly

## Test Organization

All tests are in `tests/client/bgp/e2e_test.rs` with feature gate `#[cfg(all(test, feature = "bgp"))]`.

### Test Coverage

| Test | Description | LLM Calls | Runtime |
|------|-------------|-----------|---------|
| `test_bgp_client_connect_to_server` | Basic connection to BGP server | 2 | ~3s |
| `test_bgp_client_session_establishment` | Full BGP OPEN handshake | 3 | ~4s |
| `test_bgp_client_custom_params` | Custom AS/router ID params | 2 | ~3s |

**Total LLM calls**: 7 (well under 10 budget)
**Total runtime**: ~10s

## LLM Call Budget

Following the efficiency guidelines from CLAUDE.md, these tests minimize LLM calls:

1. **Reuse setup**: Each test starts fresh server/client (no state pollution)
2. **Simple prompts**: Clear, direct instructions to LLM
3. **Minimal verification**: Check connection, not full protocol compliance

Budget breakdown:
- Server startup: 1 LLM call per test
- Client startup: 1-2 LLM calls per test
- Session handling: 0-1 LLM calls per test

Total: **7 calls across 3 tests** (< 10 budget)

## Test Approach

### Black-Box Testing

Tests treat NetGet as a black box:
- Spawn binary via helpers (`start_netget_server`, `start_netget_client`)
- Provide prompts describing desired behavior
- Verify output contains expected strings
- No direct access to internal state

### Server Setup

Each test starts a BGP server:
```rust
let server_config = NetGetConfig::new(
    "Start BGP server on port {AVAILABLE_PORT} with AS 65000..."
);
let mut server = start_netget_server(server_config).await?;
```

The `{AVAILABLE_PORT}` placeholder is replaced by the test harness with an available port.

### Client Setup

BGP clients are started with:
```rust
let client_config = NetGetConfig::new(format!(
    "Connect to 127.0.0.1:{} via BGP...",
    server.port
))
.with_startup_params(serde_json::json!({
    "local_as": 65001,
    "router_id": "192.168.1.100"
}));

let mut client = start_netget_client(client_config).await?;
```

### Verification

Tests verify client behavior by checking output:
```rust
assert!(
    client.output_contains("connected").await,
    "Client should show connection"
);
```

### Cleanup

All tests clean up servers and clients:
```rust
server.stop().await?;
client.stop().await?;
```

## Running Tests

### Run all BGP client tests
```bash
./cargo-isolated.sh test --no-default-features --features bgp --test client::bgp::e2e_test
```

### Run specific test
```bash
./cargo-isolated.sh test --no-default-features --features bgp test_bgp_client_connect_to_server
```

### Run with logging
```bash
RUST_LOG=debug ./cargo-isolated.sh test --no-default-features --features bgp
```

## Expected Runtime

- **Compilation**: 10-30s (with `--no-default-features --features bgp`)
- **Test execution**: ~10s total
  - `test_bgp_client_connect_to_server`: ~3s
  - `test_bgp_client_session_establishment`: ~4s
  - `test_bgp_client_custom_params`: ~3s

## Known Issues

1. **Timing Sensitivity**: BGP session establishment requires time for OPEN/KEEPALIVE exchange. Tests use `tokio::time::sleep()` to allow handshake completion.

2. **Port Availability**: Tests use `{AVAILABLE_PORT}` to avoid port conflicts. On busy systems, port allocation may occasionally fail.

3. **LLM Variability**: LLM responses may vary. Tests check for flexible output patterns (e.g., "connected" OR "OPEN").

4. **BGP Session State**: Tests don't verify full BGP FSM transitions, only basic connectivity and message exchange.

## Test Infrastructure

Tests rely on:
- `tests/server/helpers.rs` - Test helper functions
- `NetGetConfig` - Configuration builder for test instances
- `start_netget_server()` - Spawn server process
- `start_netget_client()` - Spawn client process
- `E2EResult` - Test result type

## Future Test Improvements

Potential enhancements (not currently implemented):

1. **UPDATE Message Handling**: Verify client receives and processes UPDATE messages
2. **NOTIFICATION Testing**: Test error scenarios (invalid AS, bad router ID)
3. **Hold Timer**: Verify session timeout behavior
4. **Concurrent Connections**: Multiple clients to same server
5. **Real BGP Peer**: Test against actual BGP router (e.g., GoBGP, BIRD)

## Debugging Failed Tests

If a test fails:

1. **Check logs**: `netget.log` contains detailed protocol traces
2. **Increase timeouts**: `tokio::time::sleep()` may need adjustment
3. **Verify Ollama**: Ensure Ollama is running and responsive
4. **Check output**: Print `client.get_output().await` to see what LLM produced
5. **Run isolated**: Use `./cargo-isolated.sh` to avoid build conflicts

## Test Maintenance

When modifying BGP client:

1. **Update tests** if behavior changes (e.g., different output messages)
2. **Keep LLM budget** under 10 calls
3. **Maintain runtime** under 15s total
4. **Document changes** in this file

## References

- Main implementation: `src/client/bgp/mod.rs`
- Actions: `src/client/bgp/actions.rs`
- Implementation docs: `src/client/bgp/CLAUDE.md`
- Test infrastructure: `TEST_INFRASTRUCTURE_FIXES.md`
