# WebDAV Protocol E2E Tests

## Test Overview

Tests WebDAV (Web Distributed Authoring and Versioning) file server using HTTP methods (PROPFIND, PUT, MKCOL) with reqwest client. Validates that NetGet can serve files over WebDAV protocol.

**Protocol**: WebDAV (RFC 4918) over HTTP
**Test Scope**: WebDAV methods, filesystem operations, server startup
**Test Type**: Black-box, prompt-driven

## Test Strategy

### Consolidated Approach
Tests organized by WebDAV operation type:
1. **Server Start** - Verify server initialization
2. **PROPFIND** - Directory listing
3. **PUT File** - File upload
4. **MKCOL** - Directory creation

Each test:
- Starts one server with specific behavior
- Sends WebDAV HTTP request (custom method)
- Validates HTTP status code (207 Multi-Status, 201 Created, etc.)

**IMPORTANT**: Current implementation does NOT involve LLM - all operations handled by dav-server library.

## LLM Call Budget

**Total Budget**: **4 LLM calls** (4 server startups only)

### Breakdown by Test

1. **test_webdav_server_start**: 1 server startup = **1 LLM call**
   - Prompt: Start WebDAV server
   - No file operations (just verify startup)

2. **test_webdav_propfind**: 1 server startup = **1 LLM call**
   - Prompt: WebDAV server with directory listings
   - Operation: PROPFIND / (handled by dav-server library)

3. **test_webdav_put_file**: 1 server startup = **1 LLM call**
   - Prompt: Accept PUT requests for file creation
   - Operation: PUT /test.txt (handled by dav-server library)

4. **test_webdav_mkcol**: 1 server startup = **1 LLM call**
   - Prompt: Accept MKCOL requests for directory creation
   - Operation: MKCOL /newdir/ (handled by dav-server library)

**CRITICAL**: LLM only consulted at server startup. All WebDAV operations handled by dav-server library without LLM involvement.

**LLM Usage**: Server startup parses prompt and initializes WebDAV stack, but all subsequent operations bypass LLM.

## Scripting Usage

**Scripting Mode**: ❌ **NOT APPLICABLE**

WebDAV operations do NOT consult LLM:
- All file operations handled by dav-server library
- MemFs provides in-memory filesystem
- No LLM calls after server startup

**Why no LLM?** Current implementation is library-driven, not LLM-controlled.

**Future Enhancement**: Implement LLM-controlled filesystem (like NFS) to enable:
- LLM-generated file content
- Dynamic directory listings
- Scripting mode for fast responses

## Client Library

**HTTP Client**: `reqwest` v0.11
- Used for custom HTTP methods (PROPFIND, MKCOL)
- Supports WebDAV-specific headers (Depth, If-None-Match)
- No dedicated WebDAV client library

**Method Construction**:
```rust
let response = client
    .request(reqwest::Method::from_bytes(b"PROPFIND")?, &url)
    .header("Depth", "1")
    .send()
    .await?;
```

**Why reqwest?** Only Rust HTTP client supporting custom methods needed for WebDAV.

## Expected Runtime

**Model**: qwen3-coder:30b
**Total Runtime**: ~40 seconds for full test suite

### Per-Test Breakdown
- **test_webdav_server_start**: ~10s (startup only, no operations)
- **test_webdav_propfind**: ~10s (startup + PROPFIND, no LLM call for operation)
- **test_webdav_put_file**: ~10s (startup + PUT, no LLM call for operation)
- **test_webdav_mkcol**: ~10s (startup + MKCOL, no LLM call for operation)

**Fast Operations**: WebDAV methods execute immediately (library-handled), no LLM latency.

## Failure Rate

**Failure Rate**: **Very Low** (<1%)

### Why So Stable?
- No LLM involvement after startup
- dav-server library handles protocol correctly
- MemFs filesystem is deterministic
- No network dependencies

### Rare Failure Modes
1. **Server startup timeout** - Ollama slow or overloaded (~1% chance)
2. **Port allocation conflict** - Rare race condition (<0.1%)

### No Known Flaky Tests
All tests are stable and deterministic.

## Test Cases

### 1. Server Start
**Purpose**: Validate WebDAV server initialization

**Test Flow**:
1. Start WebDAV server with basic prompt
2. Verify server stack is "WebDAV"
3. Stop server gracefully

**Expected Result**:
- Server starts successfully
- Stack name is "WebDAV"
- No crashes

### 2. PROPFIND (Directory Listing)
**Purpose**: Validate WebDAV PROPFIND method for directory browsing

**Test Flow**:
1. Start WebDAV server
2. Send PROPFIND request to / with Depth: 1
3. Validate response status (207 Multi-Status or 200 OK)

**Expected Result**:
- HTTP 207 Multi-Status or 200 OK
- Server responds to PROPFIND method

**Note**: Response body (XML directory listing) not validated - only HTTP status.

### 3. PUT File (File Upload)
**Purpose**: Validate WebDAV PUT method for file creation

**Test Flow**:
1. Start WebDAV server
2. Send PUT request to /test.txt with body "Hello WebDAV!"
3. Validate response status (201 Created or 204 No Content)

**Expected Result**:
- HTTP 201 Created or 204 No Content
- File stored in MemFs (in-memory)

### 4. MKCOL (Directory Creation)
**Purpose**: Validate WebDAV MKCOL method for creating directories

**Test Flow**:
1. Start WebDAV server
2. Send MKCOL request to /newdir/
3. Validate response status (201 Created)

**Expected Result**:
- HTTP 201 Created
- Directory created in MemFs

## Known Issues

### No LLM Control
- **LIMITATION**: Current implementation does NOT involve LLM in file operations
- Tests validate library functionality, not LLM integration
- LLM only parses server startup prompt

**Impact**: These tests validate WebDAV protocol stack, but not LLM-controlled filesystem.

**Future**: Implement custom filesystem (like NFS) to enable LLM control.

### Limited Response Validation
- Tests only check HTTP status codes
- XML response bodies not parsed
- File content not verified after upload

**Workaround**: Sufficient for protocol validation, not filesystem correctness.

### No Authentication Testing
- WebDAV server accepts all requests (no auth)
- No user/password validation
- Suitable for testing, not production

**Future**: Add authentication tests when auth implemented.

## Running Tests

```bash
# Build release binary with all features
./cargo-isolated.sh build --release --all-features

# Run WebDAV E2E tests
./cargo-isolated.sh test --features webdav --test server::webdav::test

# Run specific test
./cargo-isolated.sh test --features webdav --test server::webdav::test test_webdav_propfind
```

**IMPORTANT**: Always build release binary before running tests.

## Future Enhancements

### LLM-Controlled Filesystem
- Implement custom filesystem trait (like NFS LlmNfsFileSystem)
- Consult LLM for file read/write operations
- Enable dynamic file generation

### Extended Operations
- Test COPY, MOVE, DELETE methods
- Test locking (LOCK, UNLOCK)
- Test property operations (PROPPATCH)

### Response Validation
- Parse XML PROPFIND responses
- Verify file content after PUT/GET
- Check directory structure after MKCOL

### Authentication Tests
- Test Basic authentication
- Test Digest authentication
- Test 401 Unauthorized responses

### Real WebDAV Clients
- Test with Windows Explorer WebDAV
- Test with macOS Finder WebDAV mounting
- Test with davfs2 (Linux)

## References

- [RFC 4918: WebDAV](https://tools.ietf.org/html/rfc4918)
- [dav-server Rust crate](https://docs.rs/dav-server)
- [reqwest HTTP client](https://docs.rs/reqwest)
- [WebDAV Resources](http://www.webdav.org/)
