# SSH Agent Protocol Server E2E Tests

## Test Overview
Tests the SSH Agent server implementation using Unix domain sockets. SSH Agent is a binary protocol that manages SSH keys and performs signing operations for SSH clients.

## Test Strategy
- **Unix socket communication**: SSH Agent uses Unix domain sockets (socket files), not TCP/IP
- **Binary wire protocol**: SSH wire format with uint32 length prefix + message type byte + data
- **Minimal validation**: Tests basic connectivity and message parsing, not full protocol compliance
- **Future implementation**: Currently includes placeholder tests documenting expected behavior

## Current Status
⚠️ **TESTS ARE PLACEHOLDERS** - The test helpers (`tests/helpers.rs`) currently only support TCP/IP ports via `{AVAILABLE_PORT}` placeholder. They do not support Unix socket file paths needed for SSH Agent testing.

### What Works
- ✅ Unit test for message parsing (`test_ssh_agent_protocol_parsing`)
- ✅ Documentation of expected test behavior

### What Needs Implementation
- ❌ Test helpers support for Unix socket paths
- ❌ Integration tests requiring actual Unix socket connections
- ❌ Tests marked with `#[ignore]` until helper support is added

## LLM Call Budget
- `test_ssh_agent_request_identities()`: 1 LLM call (REQUEST_IDENTITIES received) - **NOT YET IMPLEMENTED**
- `test_ssh_agent_sign_request()`: 1 LLM call (SIGN_REQUEST received) - **NOT YET IMPLEMENTED**
- `test_ssh_agent_protocol_parsing()`: 0 LLM calls (pure unit test) - ✅ **IMPLEMENTED**
- **Total: 1 LLM call** (0 currently, 2 when fully implemented)

**Well under the 10 LLM call limit** even when fully implemented.

## Scripting Usage
❌ **Scripting Disabled** - Action-based responses only

**Rationale**: SSH Agent operations are stateful and require structured responses (binary protocol). The LLM should use actions like `send_identities_answer`, `send_sign_response`, etc., rather than scripts.

## Client Library
- **tokio::net::UnixStream** - Async Unix domain socket
- **tokio::io::{AsyncReadExt, AsyncWriteExt}** - For socket I/O
- **ssh-agent-lib** - Could be used for message parsing/construction, but currently using manual parsing for minimal dependencies

**Why manual parsing?**:
1. Educational - shows exact wire format
2. Minimal dependencies - just tokio + std
3. Tests don't need full SSH Agent client library
4. Direct control over message construction for edge case testing

## Expected Runtime
- Model: qwen3-coder:30b
- Runtime: ~20-30 seconds when implemented (2 tests × ~10s each)
- Each test includes: server startup (2-3s) + LLM response (5-8s) + validation (<1s)

## Failure Rate
- **Unknown** - Tests not yet fully implemented
- Expected: Low (~5%) similar to other protocols
- Potential issues: Binary format errors, Unix socket permissions, file cleanup

## Test Cases

### 1. REQUEST_IDENTITIES (`test_ssh_agent_request_identities`) - PLACEHOLDER
- **Status**: Marked with `#[ignore]`, not yet runnable
- **Prompt**: Respond to REQUEST_IDENTITIES (type 11) with IDENTITIES_ANSWER (type 12)
- **Client**: Sends binary message: [0,0,0,1,11]
- **Expected**: Response with type 12 and zero keys
- **Purpose**: Tests basic SSH Agent query/response cycle
- **Blocks**: Requires Unix socket support in test helpers

### 2. SIGN_REQUEST (`test_ssh_agent_sign_request`) - PLACEHOLDER
- **Status**: Marked with `#[ignore]`, not yet runnable
- **Prompt**: Respond to SIGN_REQUEST (type 13) with SIGN_RESPONSE (type 14)
- **Client**: Sends SIGN_REQUEST with dummy key blob and data
- **Expected**: Response with type 14 and dummy signature
- **Purpose**: Tests LLM's ability to handle signing requests
- **Blocks**: Requires Unix socket support in test helpers

### 3. Message Parsing (`test_ssh_agent_protocol_parsing`) - IMPLEMENTED ✅
- **Status**: Runnable, no LLM calls
- **Test**: Validates `build_request_identities()` helper
- **Verifies**:
  - Message is 5 bytes (4 length + 1 type)
  - Length field is correct (1 byte payload)
  - Message type is 11 (REQUEST_IDENTITIES)
- **Purpose**: Ensures test infrastructure correctly constructs SSH Agent messages

## Known Issues

### 1. Test Helper Limitations
The `tests/helpers.rs` module uses `{AVAILABLE_PORT}` placeholder for TCP port allocation. SSH Agent requires Unix socket file paths instead.

**Required changes**:
- Add `{AVAILABLE_SOCKET}` or `{SOCKET_PATH}` placeholder
- Support Unix socket server startup
- Handle socket file cleanup in test teardown

### 2. Binary Protocol Complexity
SSH Agent wire format is more complex than text protocols:
- Requires proper length prefixing
- Uses big-endian uint32 for lengths
- SSH string format: uint32 length + bytes
- Easy to get message structure wrong

**Mitigation**: Use `ssh-encoding` crate from `ssh-key` dependency for proper encoding.

### 3. Unix Socket Permissions
Unix sockets require proper file permissions. Tests must:
- Create sockets in writable temp directory
- Clean up socket files after tests
- Handle permission errors gracefully

### 4. No Full Protocol Coverage
Tests only cover 2 basic message types (REQUEST_IDENTITIES, SIGN_REQUEST). SSH Agent supports 12+ message types including:
- ADD_IDENTITY (17)
- REMOVE_IDENTITY (18)
- REMOVE_ALL_IDENTITIES (19)
- ADD_ID_CONSTRAINED (25)
- LOCK (22)
- UNLOCK (23)

**Rationale**: Testing all message types would exceed LLM budget and require extensive mock data.

## Implementation Roadmap

### Phase 1: Test Helper Updates (Required first)
1. Add Unix socket support to `tests/helpers.rs`
2. Implement `{SOCKET_PATH}` placeholder
3. Add socket file cleanup logic

### Phase 2: Basic Tests
1. Implement `test_ssh_agent_request_identities`
2. Implement `test_ssh_agent_sign_request`
3. Remove `#[ignore]` attributes

### Phase 3: Enhanced Coverage (Optional)
1. Add ADD_IDENTITY test
2. Add REMOVE_IDENTITY test
3. Add error handling tests (invalid messages)

## Alternative: Mock Socket Tests
If modifying test helpers is too complex, could use direct `tokio::net::UnixListener` in tests:

```rust
#[tokio::test]
async fn test_with_mock_socket() -> E2EResult<()> {
    let temp_dir = TempDir::new()?;
    let socket_path = temp_dir.path().join("test.sock");

    // Spawn server manually with socket path
    // Connect via UnixStream
    // Send/receive messages
    // Cleanup

    Ok(())
}
```

This would bypass the need for helper modifications but lose consistency with other protocol tests.

## References
- [IETF SSH Agent Draft](https://datatracker.ietf.org/doc/html/draft-ietf-sshm-ssh-agent-05)
- [SSH Wire Format](https://datatracker.ietf.org/doc/html/rfc4251#section-5)
- [Tokio UnixStream](https://docs.rs/tokio/latest/tokio/net/struct.UnixStream.html)
- [ssh-agent-lib crate](https://docs.rs/ssh-agent-lib/)
