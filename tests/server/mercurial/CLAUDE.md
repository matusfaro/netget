# Mercurial HTTP Protocol E2E Tests

## Test Overview

Tests Mercurial HTTP server implementation with real HTTP clients (reqwest). Validates that Mercurial wire protocol
commands work correctly over HTTP transport.

## Test Strategy

- **Multiple test scenarios**: Each test validates different Mercurial commands
- **HTTP client validation**: Uses reqwest to test protocol endpoints directly
- **Protocol compliance**: Validates response formats for capabilities, heads, branchmap, and listkeys
- **Error handling**: Tests non-existent repository handling
- **LLM-based**: All tests use LLM for flexibility and realistic responses

## LLM Call Budget

- `test_mercurial_capabilities()`: 1 LLM call (server startup)
- `test_mercurial_heads()`: 1 LLM call (server startup)
- `test_mercurial_branchmap()`: 1 LLM call (server startup)
- `test_mercurial_listkeys()`: 1 LLM call (server startup)
- `test_mercurial_repository_not_found()`: 1 LLM call (server startup)
- **Total: 5 LLM calls** (one per test)

**Why 5 calls?**: Each test spawns a separate server with different configuration. Tests are independent and validate
different aspects of the protocol.

**Within budget**: < 10 LLM calls as per protocol guidelines ✓

## Client Library

### Rust Libraries

- **reqwest v0.12** - HTTP client for direct endpoint testing
    - `Client::new()` - Create HTTP client
    - `.get()` - HTTP GET requests
    - `.send()` - Send request
    - Why: Test HTTP endpoints directly, simpler than full hg client

**Why reqwest over hg client?**:

1. **Direct protocol testing** - Tests wire protocol endpoints directly
2. **No hg client needed** - Mercurial client not readily available in Rust
3. **Simpler setup** - No need to install system hg command
4. **Protocol focus** - Tests protocol structure, not full clone operations

## Expected Runtime

- Model: qwen2.5-coder:32b (or similar)
- Per test runtime:
    - `test_mercurial_capabilities()`: ~15-20 seconds
        - Server startup: ~15-20s (1 LLM call)
        - HTTP request: <100ms
    - `test_mercurial_heads()`: ~15-20 seconds
        - Server startup: ~15-20s (1 LLM call)
        - HTTP request: <100ms
    - `test_mercurial_branchmap()`: ~15-20 seconds
        - Server startup: ~15-20s (1 LLM call)
        - HTTP request: <100ms
    - `test_mercurial_listkeys()`: ~15-20 seconds
        - Server startup: ~15-20s (1 LLM call)
        - HTTP request: <100ms
    - `test_mercurial_repository_not_found()`: ~15-20 seconds
        - Server startup: ~15-20s (1 LLM call)
        - HTTP request: <100ms

**Total test suite runtime**: ~75-100 seconds (5 tests in sequence)

**Optimization potential**: Running tests in parallel could reduce to ~15-20s (longest test), but requires Ollama lock
to prevent overload.

## Failure Rate

- **Low** (~5%) - Protocol responses are simple text format

**Common failure points**:

1. **Node ID format** - LLM must generate 40-character hex strings
    - Usually reliable, occasionally generates wrong format
2. **Response format** - LLM must follow newline or tab-separated format
    - Generally reliable with clear prompts
3. **Timeout** - Slow LLM responses
    - Rare with modern models

**Stable tests**: All tests are stable as they focus on protocol structure, not complex bundle generation.

## Test Cases

### Test 1: Capabilities (`test_mercurial_capabilities`)

**Protocol endpoint validation**

**Test Flow**:

1. Start NetGet Mercurial server with simple repository
2. HTTP GET request to `/?cmd=capabilities`
3. Validate HTTP 200 response
4. Verify newline-separated capability strings
5. Check for required capabilities (batch, branchmap, getbundle)

**What it validates**:

- Capabilities endpoint works
- Correct response format (newline-separated)
- Required capabilities present

**Why this test**:

- Foundation of Mercurial protocol
- Client's first request
- Simple text response, easy to validate

### Test 2: Heads (`test_mercurial_heads`)

**Repository heads validation**

**Test Flow**:

1. Start NetGet Mercurial server
2. HTTP GET request to `/?cmd=heads`
3. Validate HTTP 200 response
4. Verify head node IDs (40-character hex)
5. Check format compliance

**What it validates**:

- Heads endpoint works
- Node ID format (40-char hex)
- Newline-separated response

**Why this test**:

- Critical for repository discovery
- Tests node ID generation
- Simple validation

### Test 3: Branchmap (`test_mercurial_branchmap`)

**Branch mapping validation**

**Test Flow**:

1. Start NetGet Mercurial server with multiple branches
2. HTTP GET request to `/?cmd=branchmap`
3. Validate HTTP 200 response
4. Parse branch lines (format: `<branch> <node1> <node2>...`)
5. Verify node ID format for each branch

**What it validates**:

- Branchmap endpoint works
- Branch name and node ID parsing
- Multiple branch support
- Response format compliance

**Why this test**:

- Tests multi-branch scenarios
- More complex response format
- Important for branch discovery

### Test 4: Listkeys (`test_mercurial_listkeys`)

**Namespace keys validation**

**Test Flow**:

1. Start NetGet Mercurial server with bookmarks
2. HTTP GET request to `/?cmd=listkeys&namespace=bookmarks`
3. Validate HTTP 200 response
4. Parse key-value pairs (tab-separated)
5. Verify node ID format

**What it validates**:

- Listkeys endpoint works
- Namespace parameter handling
- Tab-separated format
- Bookmark to node ID mapping

**Why this test**:

- Tests query parameter handling
- Different response format (tab-separated)
- Can handle empty responses

### Test 5: Repository Not Found (`test_mercurial_repository_not_found`)

**Error handling validation**

**Test Flow**:

1. Start NetGet Mercurial server with one repository
2. Request capabilities for non-existent repository
3. Validate error response (4xx or appropriate handling)
4. Check error message

**What it validates**:

- Error handling works
- Appropriate HTTP status codes
- Server doesn't crash on invalid requests

**Why this test**:

- Important for robustness
- Tests error paths
- LLM decision-making

## Known Issues

### 1. Bundle Generation Not Tested

Mercurial bundle generation (getbundle) is complex and not included in MVP tests.

**Reason**: Bundles are binary format with changesets, manifests, and file data.

**Future**: Add bundle generation tests when implementation is more complete.

### 2. No Push Testing

Current tests only validate read operations (capabilities, heads, branchmap, listkeys).

**Reason**: MVP is read-only, push (unbundle) not implemented.

**Future**: Add push tests when push support added.

### 3. No hg Client Testing

Tests use HTTP client (reqwest) instead of Mercurial client (hg).

**Reason**: Mercurial client not readily available in Rust ecosystem.

**Impact**: Tests protocol structure but not full client compatibility.

**Future**: Could add system hg command tests if needed.

### 4. Node ID Generation

LLM must generate 40-character hex strings for node IDs.

**Issue**: Occasionally generates wrong format or length.

**Validation**: Tests check format before using.

**Impact**: Low - most prompts are clear enough.

### 5. No Authentication Testing

Tests don't validate authentication/authorization.

**Reason**: MVP has no authentication.

**Future**: Add auth tests when authentication implemented.

## Performance Notes

### HTTP Request Overhead

- First request: ~100ms (HTTP connection setup)
- Subsequent requests: ~50ms (reuse connection)
- Total per test: <200ms for HTTP operations

### Comparison to Other Protocol Tests

Mercurial tests are:

- **Faster than Git**: No complex pack generation
- **Similar to DNS**: Simple text-based protocol
- **Faster than SSH**: No complex handshake
- **Similar to HTTP**: Both use HTTP transport

## Future Enhancements

### Test Coverage Gaps

1. **Bundle operations** - When bundle support added
2. **Clone operations** - Full clone with system hg command
3. **Pull operations** - Incremental updates
4. **Multiple repositories** - Test repository routing
5. **Large responses** - Test with many branches/bookmarks
6. **Binary responses** - When bundle generation implemented
7. **Authentication** - When auth support added

### Additional Test Scenarios

```rust
#[tokio::test]
async fn test_mercurial_getbundle() {
    // Test bundle generation (when implemented)
}

#[tokio::test]
async fn test_mercurial_clone() {
    // Test full clone with system hg command (if hg available)
}

#[tokio::test]
async fn test_mercurial_multiple_repositories() {
    // Test multiple repositories on same server
}

#[tokio::test]
async fn test_mercurial_authentication() {
    // Test HTTP Basic Auth (when implemented)
}
```

### Consolidation Opportunity

Could consolidate into fewer tests:

```rust
#[tokio::test]
async fn test_mercurial_comprehensive() {
    // 1. Request capabilities (1 LLM call for server startup)
    // 2. Request heads (same server, reuse)
    // 3. Request branchmap (same server, reuse)
    // 4. Request listkeys (same server, reuse)
    // Total: 1 LLM call for comprehensive coverage
}
```

**Benefit**: Fewer server startups, faster total runtime, more efficient.

### System hg Client Integration

Future tests could use system hg command:

```rust
use std::process::Command;

let output = Command::new("hg")
    .args(&["clone", &url])
    .output()?;
```

**Benefits**:

- Real client compatibility
- Full protocol validation
- More realistic testing

**Challenges**:

- Requires hg installed
- System dependency
- More complex setup

## References

- [Mercurial Wire Protocol](https://www.mercurial-scm.org/wiki/WireProtocol)
- [HttpCommandProtocol](https://wiki.mercurial-scm.org/HttpCommandProtocol)
- [Mercurial Internals](https://hg.schlittermann.de/hg/once/help/internals.wireprotocol)
- [reqwest Documentation](https://docs.rs/reqwest/latest/reqwest/)

## Summary

The Mercurial E2E tests validate:
✅ Capabilities endpoint (server feature advertisement)
✅ Heads endpoint (repository tip discovery)
✅ Branchmap endpoint (branch to node mapping)
✅ Listkeys endpoint (bookmark/tag listing)
✅ Error handling (non-existent repositories)

Tests are designed to validate protocol flow and response formats. Bundle generation testing is deferred to future work.
This is acceptable for MVP - the important part is that the server responds correctly to the Mercurial HTTP wire
protocol.

**Total LLM calls**: 5 (one per test, within reasonable budget)
**Total runtime**: ~75-100 seconds
**Failure rate**: ~5% (low, due to simple text-based protocol)
**Test focus**: Protocol structure validation (not full clone/bundle operations)
