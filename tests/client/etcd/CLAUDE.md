# etcd Client E2E Test Documentation

## Overview
End-to-end tests for the etcd client implementation, validating connectivity, key-value operations, and error handling against a real etcd server.

## Test Strategy

### Approach
**Direct Library Testing**: Tests use `etcd-client` crate directly against Docker etcd server, validating the underlying library behavior before full NetGet LLM integration.

**Why Not Full LLM Integration?**:
- etcd client operations are synchronous and stateless (reconnect per operation)
- No persistent connection state or read loop to test
- LLM integration is minimal (just action parsing and event generation)
- Direct library testing is faster and more reliable

### Test Environment
- **etcd Server**: Docker container (`quay.io/coreos/etcd:v3.5.17`)
- **Port**: 2379 (mapped to localhost)
- **Startup Time**: ~3 seconds for etcd to be ready
- **Cleanup**: Automatic Docker container stop after each test

## Test Coverage

### Test 1: `test_etcd_client_basic_operations`
**Purpose**: Validate basic PUT, GET, DELETE operations

**Flow**:
1. Start etcd server in Docker
2. Connect etcd-client
3. PUT `/test/key1` = `value1`
4. GET `/test/key1` → verify value
5. DELETE `/test/key1` → verify deleted count
6. GET `/test/key1` → verify key is gone
7. Stop etcd server

**LLM Calls**: 0 (direct library testing)
**Runtime**: ~5-7 seconds (includes Docker startup)

### Test 2: `test_etcd_client_multiple_keys`
**Purpose**: Validate multiple key-value operations

**Flow**:
1. Start etcd server
2. PUT 3 config keys (`/app/config/database`, `/app/config/timeout`, `/app/config/max_connections`)
3. GET each key individually → verify values
4. Stop etcd server

**LLM Calls**: 0
**Runtime**: ~5-7 seconds

### Test 3: `test_etcd_client_nonexistent_key`
**Purpose**: Validate GET behavior for nonexistent keys

**Flow**:
1. Start etcd server
2. GET `/does/not/exist` → verify empty response
3. Stop etcd server

**LLM Calls**: 0
**Runtime**: ~4-5 seconds

## LLM Call Budget
**Total LLM Calls**: 0 (direct library testing only)
**Budget**: < 10 calls (well under limit)

**Rationale**: etcd client is a thin wrapper around etcd-client library. Testing the library directly validates core functionality without LLM overhead.

## Runtime
**Total Runtime**: ~15-20 seconds for all 3 tests
- Docker etcd startup: ~3 seconds per test
- etcd operations: < 1 second per test
- Docker cleanup: < 1 second per test

## Known Issues

### Docker Availability
Tests require Docker to be running. If Docker is unavailable:
- Tests will fail with "docker: command not found" or connection errors
- **Workaround**: Skip tests or run against external etcd server

### Port Conflicts
If port 2379 is already in use:
- Docker container start will fail
- **Workaround**: Stop conflicting etcd instance or modify test port

### etcd Startup Time
etcd container takes 2-3 seconds to be ready:
- Tests include 3-second sleep after Docker start
- **If flaky**: Increase sleep duration in `start_etcd_server()`

### Cleanup
Tests use `docker run --rm` for auto-cleanup:
- Container is removed automatically when stopped
- **If manual cleanup needed**: `docker stop netget-etcd-test && docker rm netget-etcd-test`

## Future Enhancements

### Phase 2: Full LLM Integration Tests
Once client action execution is implemented:
- Test LLM-generated `etcd_get`, `etcd_put`, `etcd_delete` actions
- Test event-driven response handling
- Test memory updates across operations

### Phase 3: Advanced Operations
- Range queries (prefix-based get)
- Transactions (compare-and-swap)
- Watch (streaming)
- Leases (TTL expiration)

### Phase 4: Error Scenarios
- Connection failures
- Invalid key formats
- etcd cluster unavailability
- Network timeouts

## Running Tests

```bash
# Run all etcd client tests
export PATH="$HOME/bin:$PATH"  # Ensure protoc is in PATH
./cargo-isolated.sh test --no-default-features --features etcd --test client::etcd::e2e_test

# Run specific test
./cargo-isolated.sh test --no-default-features --features etcd --test client::etcd::e2e_test test_etcd_client_basic_operations

# With output
./cargo-isolated.sh test --no-default-features --features etcd --test client::etcd::e2e_test -- --nocapture
```

## Dependencies

### System Requirements
- **Docker**: Required for running etcd server
- **protoc**: Protocol buffer compiler (required by etcd-client build)
  - Install: `apt-get install protobuf-compiler` (Debian/Ubuntu)
  - Or download from: https://github.com/protocolbuffers/protobuf/releases

### Rust Crates (dev-dependencies)
- `etcd-client = "0.15"` - etcd v3 client library
- `tokio-test = "0.4"` - Async test utilities

## Test Maintenance

### Updating etcd Version
If updating etcd Docker image version:
1. Update image tag in `start_etcd_server()` function
2. Verify etcd v3 API compatibility
3. Run all tests to validate

### etcd Server Configuration
Current configuration:
- Single-node cluster (no replication)
- No authentication
- No TLS
- HTTP-only (not HTTPS)

For production-like testing, consider:
- Multi-node cluster
- TLS encryption
- Authentication (username/password)
- Lease/watch operations

## Key Design Principles

1. **Simplicity** - Test core library behavior, defer LLM integration testing
2. **Isolation** - Each test starts fresh etcd server, no state leakage
3. **Fast** - Direct library calls, no LLM latency
4. **Reliable** - Docker ensures consistent etcd behavior
5. **Observable** - Print statements show test progress
