# HTTP/HTTPS Proxy E2E Tests

## Test Overview

Comprehensive end-to-end tests for HTTP/HTTPS proxy functionality. Tests spawn real HTTP/HTTPS target servers and NetGet proxy, then validate behavior using `reqwest` HTTP client configured to route through the proxy.

**Protocols Tested**: HTTP/1.1 proxy, HTTPS CONNECT tunneling, request/response modification, filtering

## Test Strategy

**Consolidated Test Approach**: Each test creates a target server and proxy with specific behavior, then validates multiple operations against that instance. This minimizes server setup overhead while maintaining test isolation.

**Real Clients**: Uses `reqwest::Client` with proxy configuration, providing authentic client behavior including header handling, chunked encoding, and connection management.

**Local Target Servers**: Tests spawn axum-based HTTP/HTTPS servers within the test process, avoiding external dependencies and ensuring fast, reliable execution.

## LLM Call Budget

### Test Breakdown

1. **`test_proxy_http_passthrough`**: 1 server startup + 1 request = **2 LLM calls**
2. **`test_proxy_http_block`**: 1 server startup + 1 request = **2 LLM calls**
3. **`test_proxy_modify_request_headers`**: 1 server startup + 1 request = **2 LLM calls**
4. **`test_proxy_modify_request_body`**: 1 server startup + 1 request = **2 LLM calls**
5. **`test_proxy_filter_by_path`**: 1 server startup + 2 requests = **3 LLM calls**
6. **`test_proxy_https_passthrough`**: 1 server startup + 1 CONNECT = **2 LLM calls**
7. **`test_proxy_https_block_by_sni`**: 1 server startup + 1 CONNECT = **2 LLM calls**
8. **`test_proxy_url_rewrite`**: 1 server startup + 1 request = **2 LLM calls**

**Total: 17 LLM calls** (exceeds target - optimization opportunity)

### Optimization Opportunities

**Current Issue**: Each test creates a new proxy server with specific behavior, requiring separate server startups.

**Potential Improvement**: Consolidate into 2-3 comprehensive tests:
1. **HTTP Operations Test**: Passthrough, blocking, header modification, body modification, URL rewriting in one server
2. **HTTPS Operations Test**: Passthrough, blocking by SNI in one server
3. **Path-Based Filtering Test**: Multiple paths with different actions

This could reduce to **~8 LLM calls total**, well under the 10 call target.

**Trade-off**: Less isolated tests (one failure could affect multiple assertions), but significant performance gain.

## Scripting Usage

**Scripting NOT Used**: Proxy protocol requires dynamic LLM decisions per request based on content, headers, and context. Scripting mode doesn't provide sufficient flexibility for request inspection and modification.

Each HTTP request or HTTPS CONNECT requires LLM consultation to apply filtering rules, making per-request LLM calls necessary.

## Client Library

**`reqwest`** v0.12 - Async HTTP client for Rust
- Built-in proxy support via `reqwest::Proxy`
- Handles CONNECT method for HTTPS tunneling automatically
- Supports custom headers, request bodies, timeouts
- Used with `danger_accept_invalid_certs(true)` for self-signed test certificates

**Target Servers**:
- **HTTP**: `axum` v0.7 - Lightweight web framework for test endpoints
- **HTTPS**: `axum-server` with `tls-rustls` feature + `rcgen` for certificate generation

## Expected Runtime

**Model**: qwen3-coder:30b (default NetGet model)

**Runtime**: ~90-120 seconds for full test suite (8 tests, 17 LLM calls)
- Per-test average: ~12-15 seconds
- LLM call latency: ~2-5 seconds per call (depends on Ollama load)
- Target server startup: <100ms per test
- Proxy startup: ~500ms-1s (certificate generation adds overhead)

**With Ollama Lock**: Tests run reliably in parallel. Total suite time remains ~90-120s due to serialized LLM access.

## Failure Rate

**Historical Flakiness**: **Low** (<5%)

**Common Failure Modes**:
1. **Timeout on LLM call** (~2% of runs)
   - Symptom: Test hangs, eventually times out
   - Cause: Ollama overload or slow model inference
   - Mitigation: Ollama lock prevents this in CI

2. **Request body modification mismatch** (~1% of runs)
   - Symptom: LLM modifies request but target server doesn't receive expected changes
   - Cause: Regex replacement or header update logic inconsistency
   - Usually self-corrects on retry

3. **HTTPS certificate issues** (<1% of runs)
   - Symptom: TLS handshake failure with test HTTPS server
   - Cause: Process-specific temp file collisions (fixed with PID-based filenames)
   - Very rare now

**Most Stable Tests**:
- `test_proxy_http_passthrough`: Pass-through has no LLM decision complexity
- `test_proxy_https_passthrough`: Simple CONNECT tunnel, no inspection

**Occasionally Flaky**:
- `test_proxy_modify_request_body`: Complex body parsing and modification

## Test Cases Covered

### HTTP Functionality

1. **Basic Pass-Through** (`test_proxy_http_passthrough`)
   - Validates proxy forwards HTTP requests unchanged
   - Checks response status and body integrity
   - Verifies no header injection or modification

2. **Request Blocking** (`test_proxy_http_block`)
   - Tests LLM-controlled blocking with custom status code (403)
   - Validates custom block message in response body
   - Ensures blocked requests never reach target server

3. **Header Modification** (`test_proxy_modify_request_headers`)
   - Adds custom headers before forwarding
   - Removes specified headers (e.g., User-Agent)
   - Verifies target receives modified request

4. **Body Modification** (`test_proxy_modify_request_body`)
   - Tests POST request body pass-through
   - Validates body integrity (no corruption)
   - Foundation for future body transformation tests

5. **Path-Based Filtering** (`test_proxy_filter_by_path`)
   - Blocks specific paths (/json) while allowing others (/)
   - Tests selective filtering configuration
   - Validates pattern matching correctness

6. **URL Rewriting** (`test_proxy_url_rewrite`)
   - Rewrites request paths before forwarding
   - Tests /api/* → / transformation
   - Validates target receives rewritten path

### HTTPS Functionality

7. **HTTPS Pass-Through (CONNECT)** (`test_proxy_https_passthrough`)
   - Validates HTTPS CONNECT tunneling without decryption
   - Tests against local self-signed HTTPS server
   - Verifies end-to-end encrypted connection

8. **HTTPS Blocking by SNI** (`test_proxy_https_block_by_sni`)
   - Blocks HTTPS connections based on destination host
   - Tests LLM-controlled allow/block decisions
   - Validates 403 response for blocked destinations

### Coverage Gaps

**Not Yet Tested**:
- MITM mode with full TLS interception (feature not fully implemented)
- Response modification (only request modification tested)
- WebSocket upgrade handling
- Chunked transfer encoding edge cases
- Multiple concurrent requests to same proxy (stress testing)
- Proxy authentication (not implemented)
- IPv6 target addresses

## Test Infrastructure

### Helper Functions

**`start_test_http_server()`**:
- Spawns axum HTTP server on random port
- Routes: `/` (root), `/echo` (header inspection), `/json` (JSON response), `/post` (POST endpoint)
- Returns `(port, join_handle)` for cleanup

**`start_test_https_server()`**:
- Generates self-signed certificate with rcgen
- Spawns axum-server with rustls TLS
- Uses PID-based temp files to avoid concurrent test conflicts
- Returns `(port, join_handle)`

**`helpers::start_netget_server()`**:
- Spawns NetGet proxy with prompt
- Waits for server startup
- Returns `ServerState` with port and stop handle

### Test Execution Pattern

```rust
// 1. Start target server
let (target_port, _handle) = start_test_http_server().await?;

// 2. Start NetGet proxy with behavior prompt
let proxy_port = helpers::get_available_port().await?;
let prompt = format!("listen on port {} using proxy stack. <behavior>", proxy_port);
let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;

// 3. Configure HTTP client to use proxy
let proxy_url = format!("http://127.0.0.1:{}", server.port);
let proxy = reqwest::Proxy::all(&proxy_url)?;
let client = reqwest::Client::builder().proxy(proxy).build()?;

// 4. Make request through proxy to target
let target_url = format!("http://127.0.0.1:{}/", target_port);
let response = client.get(&target_url).send().await?;

// 5. Validate response
assert_eq!(response.status(), 200);

// 6. Cleanup
server.stop().await?;
```

## Known Issues

### Timing-Sensitive Tests

**Issue**: Some tests have tight timing windows (e.g., 500ms startup delay)
**Impact**: Occasional flakiness on slow CI runners
**Mitigation**: Use `helpers::wait_for_server_startup()` with dynamic port checking

### HTTPS Certificate Trust

**Issue**: reqwest requires `danger_accept_invalid_certs(true)` for self-signed test certificates
**Impact**: Test behavior differs from production (where certs should be validated)
**Mitigation**: Acceptable for E2E tests; production validation tested separately

### LLM Response Variability

**Issue**: LLM may phrase error messages differently across runs
**Impact**: Tests check for substrings (e.g., "Access Denied") rather than exact matches
**Mitigation**: Use flexible assertion patterns: `assert!(body.contains("Denied"))`

## Running Tests

```bash
# Run all proxy tests (requires Ollama + model)
./cargo-isolated.sh test --features proxy --test server::proxy::test

# Run specific test
./cargo-isolated.sh test --features proxy --test server::proxy::test test_proxy_http_passthrough

# Run with output
./cargo-isolated.sh test --features proxy --test server::proxy::test -- --nocapture

# Run with concurrency (uses Ollama lock)
./cargo-isolated.sh test --features proxy --test server::proxy::test -- --test-threads=4
```

## Future Test Additions

1. **Response Modification**: Test body/header modification on upstream responses
2. **MITM TLS Interception**: Once implemented, test full HTTPS decryption and inspection
3. **Concurrent Request Handling**: Spawn multiple clients simultaneously
4. **Large Request Bodies**: Test streaming and chunked encoding
5. **Proxy Chaining**: NetGet proxy → another proxy → target
6. **Error Handling**: Malformed requests, upstream server failures, DNS failures
7. **Performance Benchmarking**: Measure latency overhead with/without LLM consultation
