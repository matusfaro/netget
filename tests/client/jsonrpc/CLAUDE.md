# JSON-RPC Client E2E Tests

## Test Strategy

Black-box E2E tests that spawn NetGet server and client instances to verify JSON-RPC 2.0 client functionality. Tests use local JSON-RPC server (also powered by NetGet) to avoid external dependencies.

## LLM Call Budget

**Target:** < 10 calls
**Actual:** ~6 calls
- 3 server startups (1 per test)
- 3 client connections (1 per test)

## Tests

1. **test_jsonrpc_client_single_request** (2 LLM calls)
   - Spawn JSON-RPC server with add/greet methods
   - Connect JSON-RPC client
   - Verify client can call add(5, 3)
   - Validate request/response flow

2. **test_jsonrpc_client_llm_controlled_request** (2 LLM calls)
   - Spawn JSON-RPC server with echo method
   - Client follows LLM instruction to call echo
   - Verify protocol detection (JSON-RPC)

3. **test_jsonrpc_client_batch_request** (2 LLM calls)
   - Spawn JSON-RPC server with add/multiply methods
   - Client sends batch request with 2 calls
   - Verify batch handling

## Runtime

**Expected:** < 10 seconds (including server/client startup)

## Known Issues

None

## Test Efficiency

All tests reuse the same pattern:
1. Start server with specific methods
2. Start client with instruction
3. Verify output
4. Cleanup

This minimizes LLM calls while providing good coverage of:
- Single requests
- Batch requests
- LLM-controlled method selection

## Future Tests

- Test notification (no response expected)
- Test error handling (method not found)
- Test complex parameter types (objects, arrays)
- Test request ID tracking
- Test HTTP connection failures
