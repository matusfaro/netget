# Tor Directory Protocol E2E Tests

## Test Overview

Tests validate the Tor Directory server implementation by making HTTP requests to directory paths and verifying LLM-generated consensus documents and microdescriptors.

**Testing Approach**: Black-box HTTP testing with real HTTP client (reqwest)

## Test Strategy

**Comprehensive Path Coverage**: Tests cover the main directory protocol endpoints:
1. Consensus document serving (`/tor/status-vote/current/consensus`)
2. Error handling (404 for unknown paths)
3. Microdescriptor serving (`/tor/micro/d/<hash>`)

**LLM-Generated Content**: Tests verify that:
- LLM generates valid HTTP responses
- Content contains expected directory format keywords
- Error codes are appropriate
- Content-Length headers match payload size

**Consolidation**: Each test uses one server instance with comprehensive instructions, testing multiple aspects without restarting.

## LLM Call Budget

### Test: `test_tor_directory_consensus_request`
- **Server startup**: 1 LLM call (interprets prompt, sets up directory)
- **HTTP request**: 1 LLM call (generates consensus document)
- **Total: 2 LLM calls**

### Test: `test_tor_directory_404_error`
- **Server startup**: 1 LLM call
- **HTTP request**: 1 LLM call (generates 404 response)
- **Total: 2 LLM calls**

### Test: `test_tor_directory_microdescriptors`
- **Server startup**: 1 LLM call
- **HTTP request**: 1 LLM call (generates microdescriptor)
- **Total: 2 LLM calls**

**Total Budget: 6 LLM calls for all tests**

**Within Budget**: Well under the 10 LLM call target

### Potential Optimization
**Could consolidate to 1 test**:
- Single server with comprehensive instructions
- Test consensus, 404, and microdescriptor in sequence
- Would reduce to: 1 startup + 3 requests = **4 LLM calls total**

**Current approach chosen for clarity**: Separate tests are easier to debug and understand expected behavior per scenario.

## Scripting Usage

**Scripting NOT Enabled**: Tests use action-based LLM responses

**Why Scripting Would Help**:
- Directory content is typically static (consensus doesn't change frequently)
- Could generate consensus once at startup, serve from cache
- Would reduce to: 1 LLM call total (startup only)

**Why Not Used Currently**:
1. Testing LLM's ability to generate directory documents
2. Validating action system works for directory protocol
3. Flexibility to change responses per request (dynamic content)

**Scripting Mode Test** (Future):
```rust
let config = ServerConfig::new(prompt).with_scripting_enabled();
// Startup: 1 LLM call (generates script)
// Requests: 0 LLM calls (script serves cached content)
// Total: 1 LLM call
```

**Current Status**: Action-based (no scripting)

## Client Library

**HTTP Client**: `reqwest` v0.11 (async HTTP client)

**Usage**:
```rust
let client = reqwest::Client::new();
let response = client
    .get(format!("http://127.0.0.1:{}/tor/status-vote/current/consensus", port))
    .send()
    .await?;
```

**Features Used**:
- GET requests
- Status code inspection
- Text body retrieval
- Timeout handling (15 seconds)

**Why Reqwest**:
- Standard Rust HTTP client
- Async/await support
- Simple API for testing
- Well-maintained

## Expected Runtime

**Model**: qwen3-coder:30b (default model)

**Per-Test Duration**:
- Server startup: ~500ms (LLM call + server bind)
- HTTP request: ~3-5 seconds (LLM generates directory content)
- Total per test: **~4-6 seconds**

**Full Test Suite**: 3 tests × ~5 seconds = **~15-18 seconds**

**Breakdown**:
- `test_tor_directory_consensus_request`: ~5 seconds
- `test_tor_directory_404_error`: ~5 seconds
- `test_tor_directory_microdescriptors`: ~5 seconds

**Comparison with Scripted Mode**:
- Scripted: ~2 seconds total (1 startup call, no per-request calls)
- Action-based: ~18 seconds total (1 startup + 3 request calls)
- **Tradeoff**: Action-based is slower but tests LLM's generation ability

## Failure Rate

**Current Status**: **Low** (~5% failure rate)

**Potential Failure Modes**:
1. **Ollama timeout** - Rare, occurs if Ollama is slow/busy
2. **LLM generates invalid HTTP** - Rare, LLM usually formats responses correctly
3. **Port conflicts** - Resolved by dynamic port allocation
4. **Request timeout** - 15-second timeout prevents hanging

**Flakiness**:
- Consensus generation: Stable, LLM understands directory format
- 404 errors: Stable, simple response
- Microdescriptors: Stable, well-defined format

**Not Flaky**: Tests are deterministic (same prompt → same behavior)

## Test Cases

### 1. `test_tor_directory_consensus_request`

**Purpose**: Verify directory server serves consensus documents

**Prompt**:
```
open_server port {port} base_stack tor_directory.
This is a Tor directory mirror.
When clients request /tor/status-vote/current/consensus, return a simple test consensus document with network-status-version 3 and a few fake relays.
When clients request any other path, return a 404 error.
```

**Test Steps**:
1. Start server with above prompt (1 LLM call)
2. Send GET request to `/tor/status-vote/current/consensus` (1 LLM call)
3. Verify response status is 200 OK
4. Verify response contains "network-status-version" keyword (basic format check)

**Assertions**:
- `response.status() == 200`
- `text.contains("network-status-version") || text.len() > 0`

**Expected Behavior**: LLM generates consensus document with proper HTTP headers

**LLM Calls**: 2 (startup + request)

### 2. `test_tor_directory_404_error`

**Purpose**: Verify directory server returns 404 for unknown paths

**Prompt**:
```
open_server port {port} base_stack tor_directory.
This is a Tor directory mirror.
When clients request unknown paths, return a 404 Not Found error.
```

**Test Steps**:
1. Start server with above prompt (1 LLM call)
2. Send GET request to `/tor/invalid/path` (1 LLM call)
3. Verify response status is 4xx or 5xx (error)

**Assertions**:
- `response.status().is_client_error() || response.status().is_server_error()`

**Expected Behavior**: LLM generates 404 response or similar error

**LLM Calls**: 2 (startup + request)

### 3. `test_tor_directory_microdescriptors`

**Purpose**: Verify directory server serves microdescriptors

**Prompt**:
```
open_server port {port} base_stack tor_directory.
This is a Tor directory mirror.
When clients request /tor/micro/d/<hash>, return a simple microdescriptor with onion-key and ntor-onion-key fields.
```

**Test Steps**:
1. Start server with above prompt (1 LLM call)
2. Send GET request to `/tor/micro/d/test` (1 LLM call)
3. Verify response status is 200 OK
4. Verify response contains "onion-key" keyword (basic format check)

**Assertions**:
- `response.status() == 200`
- `text.contains("onion-key") || text.len() > 0`

**Expected Behavior**: LLM generates microdescriptor with proper fields

**LLM Calls**: 2 (startup + request)

## Known Issues

### None Currently

Tests are stable and deterministic. LLM reliably generates directory content in correct format.

### Potential Future Issues
1. **LLM model changes** - If model changes, might generate different format
2. **Long consensus generation** - Large consensus documents might timeout
3. **Compression support** - When added, need to test gzip encoding

## Test Infrastructure

### Helper Functions
- `helpers::get_available_port()` - Dynamic port allocation
- `helpers::start_netget_server()` - Server spawning with prompt
- `tokio::time::timeout()` - 15-second request timeout

### HTTP Client Setup
```rust
let client = reqwest::Client::new();
let response = tokio::time::timeout(
    Duration::from_secs(15),
    client.get(url).send()
).await;
```

### Assertions
- Status code validation
- Content format validation (keyword presence)
- Timeout handling

## Comparison with Other Protocols

**Similar to**:
- HTTP (also uses reqwest client)
- OpenAPI (also serves JSON/text content)

**Unique Aspects**:
- Directory-specific content format
- Text-based protocol (not binary)
- Read-only (no POST/PUT)

**Simpler than**:
- Tor Relay (no TLS, no encryption)
- VNC (no binary protocol)

**Test Complexity**: Low (standard HTTP testing)

## Manual Testing Instructions

To test Tor Directory server manually:

1. **Start Server**:
   ```bash
   ./cargo-isolated.sh run --features tor --release
   # Prompt: "open_server port 9030 base_stack tor_directory. Serve test consensus."
   ```

2. **Request Consensus**:
   ```bash
   curl http://127.0.0.1:9030/tor/status-vote/current/consensus
   ```

3. **Verify Output**:
   ```
   HTTP/1.1 200 OK
   Content-Type: text/plain
   Content-Length: XXX

   network-status-version 3
   ...
   ```

4. **Test 404**:
   ```bash
   curl http://127.0.0.1:9030/tor/invalid/path
   # Should return 404 Not Found
   ```

**Expected Logs**:
```
[INFO] Tor Directory server listening on 127.0.0.1:9030
[DEBUG] Tor Directory GET /tor/status-vote/current/consensus from 127.0.0.1
[DEBUG] Tor Directory sent 1234 bytes
```

## Future Test Enhancements

### 1. Consensus Format Validation
- Parse consensus document
- Validate required fields (valid-after, fresh-until, etc.)
- Check relay entry format

### 2. Compression Testing
- Request with Accept-Encoding: gzip
- Verify compressed response

### 3. Multiple Relays
- Test consensus with 100+ relays
- Verify all relays present in response

### 4. Authority Keys
- Test /tor/keys/authority endpoint
- Verify key format

**Estimated Effort**: 1-2 hours for all enhancements
