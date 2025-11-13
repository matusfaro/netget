# Mock File Infrastructure Bug

## Summary

The recent refactor (commit 08be6ed "refactor: Replace environment variable with file-based mock config") introduced a bug where temporary mock config files become unreadable after initial server startup. This affects ALL E2E tests that use mocks and protocol events.

## Symptoms

1. Initial LLM call (e.g., `open_server`) successfully reads mock config file
2. Protocol event handlers (e.g., `cassandra_options`, `amqp_connection_received`) fail to read the same mock config file
3. Error: `Failed to read mock config file: No such file or directory (os error 2)`
4. Tests timeout waiting for protocol responses

## Example Error (from Cassandra tests)

```
[STDERR] [2m2025-11-13T05:14:33.353576Z[0m [34mDEBUG[0m 🔧 Successfully read mock config file (784 bytes)
[INFO] Cassandra server listening on 127.0.0.1:9042

[STDERR] [2m2025-11-13T05:14:35.672450Z[0m [34mDEBUG[0m 🔧 Mock config file present: "/var/folders/jk/0lz5v5jj33g0p8x5xmyhvwz80000gn/T/.tmpg3EBad"
[STDERR] [2m2025-11-13T05:14:35.674965Z[0m [31mERROR[0m 🔧 Failed to read mock config file: No such file or directory (os error 2)
```

## Root Cause Analysis

### Current Implementation

1. Test helper (`tests/helpers/netget.rs`) creates `NamedTempFile`
2. Writes mock config JSON to temp file
3. Converts to `TempPath` using `into_temp_path()`
4. Stores `TempPath` in `NetGetInstance._mock_temp_file` to keep file alive
5. Passes file path to child netget process via `--mock-config-file` CLI argument
6. Child process (`OllamaClient::generate_with_format()`) reads file on each LLM call

### The Bug

The temp file can be read successfully during initial LLM calls but becomes unreadable 2+ seconds later when protocol events fire. The file path is still present in the `OllamaClient` but reading fails with "No such file or directory".

### Possible Causes

1. **`TempPath` deletion timing**: `TempPath` is supposed to keep the file alive, but there may be a race condition or unexpected deletion behavior
2. **File handle closure**: The `NamedTempFile` is converted to `TempPath`, which may close the file handle prematurely
3. **Permission changes**: File permissions may change between creation and later access
4. **tempfile crate bug**: Version 3.x may have platform-specific issues

## Impact

**ALL E2E tests with mocks that use protocol events are affected**, including:

- ✅ Tests with ONLY instruction-based mocks (startup only) - WORK
- ❌ Tests with event-based mocks (protocol events) - FAIL
    - Cassandra (all 8 tests)
    - AMQP
    - HTTP
    - Redis
    - And many more...

## Investigation Needed

1. Check `tempfile` crate version and known issues
2. Test if `NamedTempFile::persist()` solves the issue
3. Consider alternative approaches:
    - Environment variable with base64-encoded JSON (original approach)
    - Named pipe or socket for mock data
    - HTTP endpoint for mock config (local server)
    - Embedded mock config in binary (compile-time)

## Workaround

Until fixed, E2E tests can use `--use-ollama` flag to bypass mocks and test with real Ollama. However, this:

- Requires Ollama running
- Is slower (real LLM calls)
- Uses more resources
- May be non-deterministic

## Fix Priority

**HIGH** - This blocks all E2E test development with mocks. The refactor was intended to enable parallel testing but inadvertently broke protocol event mocking.

## Related Files

- `tests/helpers/netget.rs:220-256` - Temp file creation
- `src/llm/ollama_client.rs:346-365` - Mock config file reading
- `tests/server/cassandra/e2e_test.rs` - Example failing tests
- Commit 08be6ed - Original refactor

## Next Steps

1. Investigate `tempfile` crate behavior
2. Test alternative temp file retention strategies
3. Consider reverting to environment variable approach with unique names per test
4. Add integration test specifically for mock file persistence
