# NFS Protocol E2E Tests

## Test Overview

Tests NFSv3 server using TCP connection tests. Validates that NetGet can start NFS server and accept connections. Full NFS protocol tests are placeholders (marked #[ignore]).

**Protocol**: NFSv3 (RFC 1813) over TCP
**Test Scope**: Server startup, TCP connection handling, connection lifecycle
**Test Type**: Black-box, prompt-driven

## Test Strategy

### Basic Connectivity Tests
Current tests focus on TCP-level validation:
1. **Server Start** - Verify NFS server initialization
2. **TCP Connection** - Establish connection to NFS port
3. **Multiple Connections** - Concurrent client connections
4. **Connection Lifecycle** - Connect, hold, disconnect, reconnect
5. **Port Configuration** - Verify custom port binding
6. **Server Stop** - Graceful shutdown

**IMPORTANT**: Tests do NOT send real NFS/RPC protocol messages. Only TCP connectivity validated.

### Placeholder Tests
Full NFS protocol tests exist but are marked `#[ignore]`:
- `test_nfs_mount_export` - MOUNT protocol testing (requires RPC client)
- `test_nfs_file_lookup` - NFS LOOKUP procedure (requires XDR encoding)
- `test_nfs_read_write` - NFS READ/WRITE operations (requires filesystem client)

**Why ignored?** No Rust NFS client library suitable for E2E testing.

## LLM Call Budget

**Total Budget**: **6 LLM calls** (6 server startups only)

### Breakdown by Test

1. **test_nfs_server_start**: 1 server startup = **1 LLM call**
   - Prompt: Start NFS server
   - Validation: Server stack is "NFS"

2. **test_nfs_tcp_connection**: 1 server startup = **1 LLM call**
   - Prompt: Accept NFS client connections
   - Validation: TCP connection succeeds

3. **test_nfs_multiple_connections**: 1 server startup = **1 LLM call**
   - Prompt: Support concurrent NFS clients
   - Validation: 3 TCP connections succeed

4. **test_nfs_connection_lifecycle**: 1 server startup = **1 LLM call**
   - Prompt: Handle connection lifecycle events
   - Validation: Connect, disconnect, reconnect

5. **test_nfs_port_configuration**: 1 server startup = **1 LLM call**
   - Prompt: NFS on custom port
   - Validation: Server listens on requested port

6. **test_nfs_server_stop**: 1 server startup = **1 LLM call**
   - Prompt: Support clean shutdown
   - Validation: Server stops gracefully

**CRITICAL**: LLM only consulted at server startup. No NFS operations performed (no LLM calls after startup).

**Future**: Add real NFS client tests requiring ~10+ LLM calls (MOUNT, LOOKUP, READ operations).

## Scripting Usage

**Scripting Mode**: ❌ **NOT APPLICABLE**

Tests do NOT exercise NFS protocol operations:
- Only TCP connection tests
- No NFS LOOKUP, READ, WRITE calls
- No LLM consultation after startup

**Future**: When real NFS client tests added, scripting mode could optimize:
- Script handles LOOKUP responses (deterministic file IDs)
- Script handles READ responses (static file content)
- Reduce LLM calls from ~15 to ~6 (startups + setup only)

## Client Library

**TCP Client**: `tokio::net::TcpStream`
- Used for raw TCP connection testing
- No NFS/RPC protocol encoding
- Simple connect/disconnect validation

**Why TcpStream?** No Rust NFSv3 client library exists for testing.

**Missing Library**: Ideal test infrastructure would use:
- `nfs-client` (doesn't exist in Rust)
- RPC/XDR client for MOUNT and NFS procedures
- File handle management

**Current Limitation**: Tests validate server accepts connections, but not NFS protocol correctness.

## Expected Runtime

**Model**: qwen3-coder:30b
**Total Runtime**: ~60 seconds for full test suite

### Per-Test Breakdown
- **test_nfs_server_start**: ~10s (startup + validation)
- **test_nfs_tcp_connection**: ~10s (startup + 1 TCP connect)
- **test_nfs_multiple_connections**: ~10s (startup + 3 TCP connects)
- **test_nfs_connection_lifecycle**: ~10s (startup + connect/reconnect)
- **test_nfs_port_configuration**: ~10s (startup + port check)
- **test_nfs_server_stop**: ~10s (startup + graceful stop)

**Fast Operations**: TCP connections complete in milliseconds. No NFS protocol overhead.

## Failure Rate

**Failure Rate**: **Very Low** (<1%)

### Why So Stable?
- Only TCP connectivity tested (simple)
- No NFS protocol complexity
- No LLM involvement after startup
- Deterministic connection handling

### Rare Failure Modes
1. **Server startup timeout** - Ollama slow or overloaded (~1% chance)
2. **Port allocation conflict** - Rare race condition (<0.1%)
3. **Connection refused** - nfsserve binding failure (very rare)

### No Known Flaky Tests
All tests are stable and deterministic.

## Test Cases

### 1. Server Start
**Purpose**: Validate NFS server initialization

**Test Flow**:
1. Start NFS server with basic prompt
2. Verify server stack is "NFS"
3. Verify server is running
4. Stop server gracefully

**Expected Result**:
- Server starts successfully
- Stack name is "NFS"

### 2. TCP Connection
**Purpose**: Validate NFS server accepts TCP connections

**Test Flow**:
1. Start NFS server
2. Establish TCP connection to NFS port
3. Verify connection succeeds (no immediate close)
4. Try non-blocking read (should WouldBlock or succeed)

**Expected Result**:
- TCP connection established
- Server keeps connection open

**Note**: No NFS protocol messages sent - pure TCP test.

### 3. Multiple Connections
**Purpose**: Validate NFS server handles concurrent clients

**Test Flow**:
1. Start NFS server
2. Open 3 TCP connections concurrently
3. Verify all connections maintained
4. Close all connections

**Expected Result**:
- All 3 connections succeed
- No connection refused errors

### 4. Connection Lifecycle
**Purpose**: Validate connection management (connect, disconnect, reconnect)

**Test Flow**:
1. Start NFS server
2. Connect → hold → close
3. Reconnect to verify server still accepting

**Expected Result**:
- Graceful close without errors
- Reconnection succeeds (server not broken)

### 5. Port Configuration
**Purpose**: Validate NFS server listens on custom port

**Test Flow**:
1. Request custom port in prompt
2. Verify server binds to that port
3. Connect to verify listening

**Expected Result**:
- Server listens on requested port (not standard 2049)

### 6. Server Stop
**Purpose**: Validate graceful shutdown

**Test Flow**:
1. Start NFS server
2. Establish connection
3. Stop server
4. Verify port released (connection refused)

**Expected Result**:
- Server stops cleanly
- Port no longer accepting connections

## Known Issues

### No Real NFS Protocol Testing
**LIMITATION**: Tests do NOT exercise NFS/RPC protocol:
- No MOUNT procedure calls
- No NFS LOOKUP, READ, WRITE operations
- No XDR encoding/decoding validation

**Impact**: Tests validate TCP infrastructure, not NFS protocol correctness.

**Why?** No Rust NFS client library exists for testing.

**Workaround**: Placeholder tests marked `#[ignore]` for future implementation.

### Ignored Tests
Three tests exist but are not run:
- `test_nfs_mount_export` - Requires RPC MOUNT client
- `test_nfs_file_lookup` - Requires NFS LOOKUP client
- `test_nfs_read_write` - Requires NFS READ/WRITE client

These tests have skeleton implementations with TODOs.

**Future**: Implement manual RPC/XDR client or wait for Rust NFS client library.

### No Filesystem Validation
- Cannot test LLM-generated file content
- Cannot test directory listings
- Cannot test file attributes (mode, size, timestamps)

**Workaround**: Unit tests could validate action parsing in isolation.

## Running Tests

```bash
# Build release binary with all features
./cargo-isolated.sh build --release --all-features

# Run NFS E2E tests (only connectivity tests, ignores NFS protocol tests)
./cargo-isolated.sh test --features e2e-tests,nfs --test server::nfs::test

# Run specific test
./cargo-isolated.sh test --features e2e-tests,nfs --test server::nfs::test test_nfs_tcp_connection

# Run ignored tests (will fail - not implemented)
./cargo-isolated.sh test --features e2e-tests,nfs --test server::nfs::test -- --ignored
```

**IMPORTANT**: Always build release binary before running tests.

## Future Enhancements

### Real NFS Client Implementation
**Highest Priority**: Implement NFS/RPC client for protocol testing:
- Manual RPC/XDR encoding (no library exists)
- MOUNT procedure to get root file handle
- NFS LOOKUP to resolve paths
- NFS READ/WRITE to test file operations
- NFS READDIR to test directory listings

**Estimated Effort**: 2-3 days for basic RPC/XDR client.

### LLM-Driven Filesystem Tests
Once client exists, add tests for:
- File content generation (LLM returns dynamic content)
- Directory structure (LLM defines folders and files)
- File attributes (LLM sets permissions, sizes, timestamps)
- Error handling (LLM returns NFS3ERR_NOENT, NFS3ERR_ACCES)

**Estimated LLM Calls**: ~15-20 for comprehensive NFS test suite.

### Scripting Mode Optimization
Enable scripting for NFS operations:
- Script handles LOOKUP (deterministic file IDs)
- Script handles READ (static file content)
- Script handles READDIR (fixed directory listings)
- Reduce LLM calls from ~20 to ~6 (startups only)

### Performance Tests
- Measure LLM call latency per NFS operation
- Test concurrent file reads/writes
- Stress test with many file handles

## References

- [RFC 1813: NFS Version 3 Protocol](https://tools.ietf.org/html/rfc1813)
- [RFC 1831: RPC Version 2](https://tools.ietf.org/html/rfc1831)
- [nfsserve Rust crate](https://docs.rs/nfsserve)
- [Linux NFS utils](https://linux-nfs.org/)
