# HTTP Protocol E2E Tests

## Test Overview
Tests the HTTP server implementation with comprehensive coverage of HTTP features: methods, routing, headers, status codes, JSON APIs, and error handling. Validates that the LLM can generate correct HTTP responses for various web application scenarios.

## Test Strategy
- **Isolated test servers**: Each test spawns a separate NetGet instance with specific instructions
- **Real HTTP client**: Uses `reqwest` async HTTP client (industry standard)
- **Comprehensive scenarios**: Tests web apps, REST APIs, routing, error codes, custom headers
- **One server per test**: Each test validates a different aspect of HTTP functionality
- **Fast validation**: 10-second default timeout per request

## LLM Call Budget
- `test_http_simple_get()`: 1 LLM call (GET request)
- `test_http_json_api()`: 1 LLM call (POST request)
- `test_http_routing()`: 3 LLM calls (3 different routes: /home, /about, /unknown)
- `test_http_headers()`: 1 LLM call (GET with custom headers)
- `test_http_methods()`: 4 LLM calls (GET, POST, PUT, DELETE)
- `test_http_error_responses()`: 3 LLM calls (403, 500, 301 redirects)
- `test_http_simple_get_with_logging()`: 1 LLM call (GET with file logging)
- **Total: 14 LLM calls** (above 10 target, but necessary for comprehensive HTTP testing)

**Optimization Opportunity**: Could consolidate into 2-3 comprehensive servers:
1. **Basic server**: Methods, routing, simple responses (6 calls → 1 server setup + 6 requests = 7 calls)
2. **Advanced server**: Custom headers, error codes, JSON API (5 calls → 1 server setup + 5 requests = 6 calls)
3. **Logging server**: File logging test (1 call)

This would reduce to ~14 calls (same total but better organized). However, current approach provides better isolation and clearer failure diagnosis per feature.

**Rationale for 14 calls**: HTTP is a Beta protocol and cornerstone of NetGet. Comprehensive testing of all HTTP features (methods, headers, status codes, routing, JSON) is critical for stability. Slightly exceeding the 10-call guideline is acceptable for such a fundamental protocol.

## Scripting Usage
❌ **Scripting Disabled** - Action-based responses only

**Rationale**: HTTP tests use diverse prompts testing different features. Action-based responses provide flexibility for the LLM to interpret varied instructions (routing, headers, status codes, etc.). Scripting would require more complex setup for each test scenario.

**Future Consideration**: Scripting could be valuable for high-throughput HTTP tests (e.g., load testing) where the same endpoints are hit repeatedly. Current tests focus on functionality breadth, not throughput.

## Client Library
- **reqwest v0.11** - Async HTTP client built on hyper
- **Features used**:
  - GET, POST, PUT, DELETE methods
  - JSON request/response parsing
  - Custom header handling
  - Redirect policy control (for 301/302 testing)
  - Status code validation

**Why reqwest?**:
1. Industry-standard HTTP client for Rust
2. Excellent async/await support with Tokio
3. Built-in JSON serialization/deserialization
4. Convenient header and redirect handling
5. Same underlying hyper library as server (protocol correctness)

## Expected Runtime
- Model: qwen3-coder:30b
- Runtime: ~2-3 minutes for full test suite (7 tests × 14 LLM calls)
- Each test includes:
  - Server startup: 2-3 seconds
  - LLM response per request: 5-8 seconds
  - Network I/O and validation: <1 second per request
- Longest test: `test_http_methods()` (~40s for 4 requests)

## Failure Rate
- **Low** (~5%) - Occasional LLM response format issues
- Common failure modes:
  - LLM returns incorrect status code (e.g., 200 instead of 404)
  - LLM adds extra text to body (test checks with `contains()` to tolerate this)
  - LLM forgets to include custom headers
  - LLM misinterprets routing instructions
- Timeout failures: Rare (<1%) - usually indicates Ollama overload

## Test Cases

### 1. Simple GET (`test_http_simple_get`)
- **Prompt**: Return HTML "Hello World" for any GET request
- **Client**: `GET /`
- **Expected**: Status 200, body contains "Hello World"
- **Purpose**: Basic HTTP GET and HTML response

### 2. JSON API (`test_http_json_api`)
- **Prompt**: POST to /api/data returns 201 with JSON body
- **Client**: `POST /api/data` with JSON payload
- **Expected**:
  - Status 201 Created
  - Content-Type: application/json
  - Body: `{"status": "created", "id": 123}`
- **Purpose**: REST API with JSON request/response

### 3. Routing (`test_http_routing`)
- **Prompt**: Different responses for /home, /about, and 404 for others
- **Client**:
  - `GET /home` → expect "Welcome" or "Home"
  - `GET /about` → expect "About"
  - `GET /unknown` → expect 404 "Not Found"
- **Expected**: Correct routing and 404 handling
- **Purpose**: Path-based routing and error responses

### 4. Custom Headers (`test_http_headers`)
- **Prompt**: Return custom headers X-API-Version and X-Custom
- **Client**: `GET /api`
- **Expected**:
  - Status 200
  - Header: `X-API-Version: 1.0`
  - Header: `X-Custom: test-value`
  - Body contains "API Response"
- **Purpose**: Custom response header generation

### 5. HTTP Methods (`test_http_methods`)
- **Prompt**: Different responses for GET, POST, PUT, DELETE
- **Client**:
  - `GET /` → expect "GET Response"
  - `POST /` → expect "POST Response"
  - `PUT /` → expect "PUT Response"
  - `DELETE /` → expect "DELETE Response"
- **Expected**: Method-based routing
- **Purpose**: HTTP method handling (REST verbs)

### 6. Error Responses (`test_http_error_responses`)
- **Prompt**: Return 403, 500, and 301 for different paths
- **Client** (with no-redirect policy):
  - `GET /forbidden` → expect 403 with "Access Denied"
  - `GET /error` → expect 500 with "Server Error"
  - `GET /redirect` → expect 301 with Location: /home
- **Expected**: Various status codes and redirect headers
- **Purpose**: Error handling, status codes, redirects

### 7. Access Logging (`test_http_simple_get_with_logging`)
- **Prompt**: Serve HTML and log requests to file "access_logs"
- **Client**: `GET /`
- **Expected**:
  - Status 200, body "Hello World"
  - Log file created: `netget_access_logs_*.log`
  - Log file contains at least one line
- **Purpose**: File I/O action, logging capability
- **Note**: Lenient validation - LLM may interpret logging differently

## Known Issues

### 1. LLM Response Variability
The LLM may add extra text, formatting, or explanations beyond what's requested. Tests use `contains()` checks rather than exact matching.

**Example**: Prompt says return "Welcome Home", LLM might return "Welcome Home! We're glad you're here."

**Mitigation**: Tests check for key phrases rather than exact strings.

### 2. Header Case Sensitivity
HTTP headers are case-insensitive, but LLM may use different casing:
- Prompt: `X-API-Version`, LLM might return `x-api-version` or `X-Api-Version`

**Mitigation**: Tests use lowercase header lookup (`headers.get("x-api-version")`)

### 3. Logging Test Leniency
`test_http_simple_get_with_logging` doesn't fail if log file isn't created. This is intentional - the LLM might interpret "log to file" differently (e.g., log to stderr, use show_message, etc.).

**Rationale**: File logging is a secondary feature. Core HTTP functionality is more critical.

**Future Improvement**: Make logging requirements more explicit in prompt.

### 4. No Connection Cleanup Validation
Tests don't verify that HTTP connections are properly closed. They just stop the server process, which forcibly closes all connections.

**Future Improvement**: Add test for Connection: close header handling.

### 5. No Request Body Validation for GET
Tests don't verify that GET requests have empty bodies (best practice). LLM might accidentally set a body on GET responses.

**Impact**: Minimal - HTTP clients ignore GET response bodies anyway.

## Performance Notes

### Why reqwest Over Raw Sockets?
Original consideration was to use raw TCP sockets like TCP tests. Reasons for choosing reqwest:
1. **HTTP parsing complexity**: HTTP protocol is complex (headers, chunking, keep-alive)
2. **Faster test development**: reqwest handles protocol details
3. **Better error messages**: reqwest provides clear HTTP errors
4. **Industry standard**: Tests use same client that users would use
5. **JSON support**: Built-in JSON serialization saves test code

**Trade-off**: Slightly slower than raw sockets due to client overhead, but still fast enough (~5-10ms per request).

### Test Isolation vs. Consolidation
Current approach: 7 separate servers, 14 LLM calls

**Pros**:
- Clear failure isolation (if routing fails, only routing test fails)
- Easy to understand what each test validates
- Parallel test execution spreads load across tests

**Cons**:
- Slightly more LLM calls (7 server startups instead of 2-3)
- Longer total runtime (more server startup overhead)

**Verdict**: Isolation is worth the cost for a Beta protocol. Clear diagnostics are critical for stability.

### Timeout Strategy
Default 10-second timeout per request provides good balance:
- Allows for slow LLM responses (5-8s typical)
- Catches hung connections quickly
- Fails fast on misconfigured servers

## Future Enhancements

### Test Coverage Gaps
1. **Large request bodies**: No tests for large POST/PUT bodies (e.g., 10MB upload)
2. **Chunked encoding**: No tests for chunked transfer encoding
3. **Keep-alive**: No tests verifying multiple requests on same TCP connection
4. **Concurrent requests**: No tests for multiple simultaneous clients
5. **Binary responses**: No tests for image/file downloads
6. **Query parameters**: No tests for URL query string parsing
7. **Request headers**: No tests verifying LLM sees all request headers
8. **WebSocket upgrade**: No tests for Upgrade header (out of scope)
9. **HTTP/2**: No tests for HTTP/2 (not implemented)
10. **HTTPS/TLS**: No tests for TLS (not implemented)

### Consolidation Opportunity
All tests could be consolidated into 2 comprehensive servers:

**Server 1 - Basic HTTP**:
```
listen on port {} via http
GET / → "Hello World"
POST / → "POST Response"
PUT / → "PUT Response"
DELETE / → "DELETE Response"
GET /home → "Welcome Home"
GET /about → "About Us"
GET /unknown → 404 "Not Found"
```
7 requests = 1 startup + 7 calls = 8 total (vs. current 11 calls for these tests)

**Server 2 - Advanced HTTP**:
```
listen on port {} via http
POST /api/data → 201 with JSON {"status": "created", "id": 123}
GET /api → 200 with X-API-Version: 1.0, X-Custom: test-value
GET /forbidden → 403 "Access Denied"
GET /error → 500 "Server Error"
GET /redirect → 301 Location: /home
```
5 requests = 1 startup + 5 calls = 6 total (vs. current 6 calls for these tests)

**Total: 14 calls** (same as current, but better organized into comprehensive servers)

### Potential New Tests
1. **Stress test**: 100 concurrent requests to same endpoint (with scripting)
2. **File upload**: Multipart form data upload
3. **Range requests**: Partial content with 206 status
4. **Caching headers**: ETag, Cache-Control, If-None-Match
5. **CORS**: Cross-origin headers and preflight requests
6. **Compression**: Accept-Encoding: gzip, Content-Encoding: gzip
7. **Authentication**: Basic auth, Bearer tokens
8. **Cookie handling**: Set-Cookie and Cookie headers

## Comparison with Other Protocol Tests

| Protocol | Tests | LLM Calls | Runtime | Rationale |
|----------|-------|-----------|---------|-----------|
| TCP | 5 | 5 | ~60s | Core protocol, raw bytes |
| UDP | 1 | 1 | ~10s | Minimal (protocols like DNS tested separately) |
| **HTTP** | **7** | **14** | **2-3m** | **Beta protocol, comprehensive feature testing** |
| DNS | 2 | 2 | ~25s | Scripting enabled, fast |
| SSH | 3 | ~8-10 | ~2m | Authentication, shell, commands |

HTTP has the highest LLM call count because it's:
1. **Beta status** - Production-ready, needs thorough testing
2. **Widely used** - Most common protocol for NetGet users
3. **Feature-rich** - Methods, headers, status codes, routing, JSON, etc.
4. **Foundation for APIs** - Many use cases depend on HTTP correctness

## References
- [RFC 7230: HTTP/1.1 Message Syntax and Routing](https://datatracker.ietf.org/doc/html/rfc7230)
- [RFC 7231: HTTP/1.1 Semantics and Content](https://datatracker.ietf.org/doc/html/rfc7231)
- [reqwest Documentation](https://docs.rs/reqwest/latest/reqwest/)
- [Hyper Documentation](https://docs.rs/hyper/latest/hyper/)
- Related tests: TCP (raw), HTTP Proxy (proxy protocol)
