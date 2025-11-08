# Socket File Client Testing

## Test Strategy

The Socket File client tests use a **hybrid approach**:

1. **Unit Tests:** Test protocol metadata, actions, and event definitions without LLM
2. **Minimal E2E:** Basic connection test with mock Unix socket server (no LLM calls)
3. **Fast Execution:** < 1 second total runtime

## LLM Call Budget

**Target:** 0 LLM calls per test run

**Rationale:**
- Socket File is essentially TCP over Unix domain sockets
- Same patterns as TCP client (already validated)
- Unit tests provide sufficient coverage for protocol implementation
- E2E would require complex test setup with Unix socket servers

**Alternative Testing:**
- Manual testing with real services (Docker, Redis, PostgreSQL)
- Integration tests in larger test suites (if needed)

## Expected Runtime

**Total:** < 1 second

**Breakdown:**
- `test_socket_file_metadata`: < 10ms (protocol metadata verification)
- `test_socket_file_actions`: < 10ms (action parsing and execution)
- `test_socket_file_events`: < 10ms (event type validation)
- `test_socket_file_connect`: < 500ms (basic connection with mock server)

## Test Coverage

### What We Test

**✅ Protocol Metadata:**
- Protocol name ("SocketFile")
- Stack name ("UnixSocket")
- Keywords (socket file, unix socket, domain socket)
- Description and example prompt
- Development state (Experimental)

**✅ Action Definitions:**
- Async actions (send_socket_file_data, disconnect)
- Sync actions (send_socket_file_data, wait_for_more)
- Action parameter validation
- Action execution (hex decoding, result types)

**✅ Event Types:**
- socket_file_connected event
- socket_file_data_received event
- Event parameter definitions

**✅ Basic Connectivity:**
- Connection to Unix socket server
- Socket path handling
- Error handling (missing socket file)

### What We Don't Test (Yet)

**❌ Full LLM Integration:**
- LLM interpreting received data
- LLM generating response actions
- Memory updates across interactions

**❌ Real-World Services:**
- Docker daemon (/var/run/docker.sock)
- PostgreSQL socket
- Redis socket

**❌ Advanced Features:**
- Credential passing (SCM_CREDENTIALS)
- File descriptor passing
- Abstract namespace sockets (Linux)

## Known Issues

### Platform Dependencies

**Issue:** Unix sockets are not available on all platforms

**Impact:** Tests will fail on Windows (except Windows 10+ with AF_UNIX support)

**Mitigation:**
- Feature-gated tests (`#[cfg(all(test, feature = "socket_file"))]`)
- Skip tests on unsupported platforms

### Socket File Cleanup

**Issue:** Test socket files may persist if tests crash

**Impact:** Subsequent test runs may fail if socket file already exists

**Mitigation:**
- Always clean up socket files in test teardown
- Use unique socket paths per test (include PID)
- `let _ = std::fs::remove_file(&socket_path);` before and after tests

### Permissions

**Issue:** Socket file permissions may prevent connection

**Impact:** Tests may fail in restricted environments

**Mitigation:**
- Use /tmp directory (world-writable)
- Fall back to test-specific directory if /tmp unavailable

## Test Expansion Strategy

### Phase 1: Current (Unit Tests Only)

**Status:** ✅ Implemented

- Protocol metadata validation
- Action definition verification
- Basic connection test (mock server)
- Zero LLM calls, < 1 second runtime

### Phase 2: Real Service Integration (Optional)

**Status:** Not implemented

**If Needed:**
- Test with Docker socket (if Docker installed)
- Test with Redis socket (if Redis running)
- Requires conditional test execution (`#[ignore]` or environment checks)
- Budget: 3-5 LLM calls for real service interactions

### Phase 3: LLM Integration Tests (Optional)

**Status:** Not implemented

**If Needed:**
- Full E2E test with LLM controlling socket communication
- Test protocol-specific interactions (HTTP over Unix socket)
- Budget: 5-10 LLM calls
- Runtime: 30-60 seconds (due to LLM latency)

## Running Tests

### Run Socket File Tests Only

```bash
./cargo-isolated.sh test --no-default-features --features socket_file --test client::socket_file::e2e_test
```

### Run All Client Tests

```bash
./cargo-isolated.sh test --all-features
```

### Run Without Building (if already built)

```bash
./cargo-isolated.sh test --no-fail-fast --no-default-features --features socket_file
```

## Manual Testing Examples

For manual validation with real services:

### Docker Daemon

```bash
# Start NetGet
./cargo-isolated.sh run --no-default-features --features socket_file

# In TUI, open socket file client:
open_client socket_file /var/run/docker.sock "GET /containers/json HTTP/1.1\r\nHost: localhost\r\n\r\n"
```

### PostgreSQL

```bash
# Connect to PostgreSQL socket (if running)
open_client socket_file /var/run/postgresql/.s.PGSQL.5432 "Send PostgreSQL protocol handshake"
```

### Redis

```bash
# Connect to Redis socket
open_client socket_file /var/run/redis/redis.sock "Send PING command"
```

## Success Criteria

Tests pass if:

1. ✅ All unit tests pass (metadata, actions, events)
2. ✅ Basic connection test completes without errors
3. ✅ Total runtime < 1 second
4. ✅ Zero LLM API calls made
5. ✅ No socket file leaks (cleanup successful)

## Future Improvements

### Automated Service Detection

Detect if Docker/Redis/PostgreSQL sockets exist and run conditional tests:

```rust
#[test]
fn test_docker_socket() {
    if std::path::Path::new("/var/run/docker.sock").exists() {
        // Run Docker socket test
    }
}
```

### Cross-Platform Socket Simulation

Use `tempfile` crate to create platform-appropriate socket paths:

```rust
use tempfile::TempDir;
let tmp_dir = TempDir::new()?;
let socket_path = tmp_dir.path().join("test.sock");
```

### Benchmarking

Add criterion benchmarks for:
- Connection latency
- Throughput (bytes/second)
- Comparison with TCP loopback
