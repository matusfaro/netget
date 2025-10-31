# IPP Protocol E2E Tests

## Test Overview

Tests IPP (Internet Printing Protocol) server using HTTP POST requests with binary IPP payloads. Validates that NetGet can act as an IPP printer with LLM-controlled responses.

**Protocol**: IPP/1.1 and IPP/2.0 over HTTP
**Test Scope**: IPP operations (Get-Printer-Attributes, Print-Job), HTTP transport
**Test Type**: Black-box, prompt-driven

## Test Strategy

### Consolidated Approach
Tests organized by IPP operation type:
1. **Get-Printer-Attributes** - Query printer capabilities
2. **Print-Job** - Submit print job
3. **Basic HTTP** - Verify HTTP-level connectivity

Each test:
- Starts one server with specific IPP operation behavior
- Sends crafted binary IPP request
- Validates HTTP response and IPP status code

### Manual IPP Request Construction
Tests manually build IPP binary format:
- Version (2 bytes): 0x02 0x00 (IPP 2.0)
- Operation ID (2 bytes): 0x000B (Get-Printer-Attributes), 0x0002 (Print-Job)
- Request ID (4 bytes): Sequential
- Attribute groups: charset, language, printer-uri, etc.

**Why manual?** No Rust IPP client library suitable for testing.

## LLM Call Budget

**Total Budget**: **6 LLM calls** (2 servers × 3 operations)

### Breakdown by Test

1. **test_ipp_get_printer_attributes**: 1 server startup + 1 request = **2 LLM calls**
   - Prompt: Act as IPP printer, respond to Get-Printer-Attributes
   - Request: Single Get-Printer-Attributes operation

2. **test_ipp_print_job**: 1 server startup + 1 request = **2 LLM calls**
   - Prompt: Accept Print-Job requests, return job attributes
   - Request: Single Print-Job operation with document data

3. **test_ipp_basic_http**: 1 server startup + 1 request = **2 LLM calls**
   - Prompt: Respond to all IPP requests with status 200
   - Request: Simple HTTP GET (not proper IPP, tests HTTP layer)

**CRITICAL**: No scripting mode used - each IPP request requires LLM call.

**Future optimization**: Add scripting mode for IPP to reduce calls to 3 (server startups only).

## Scripting Usage

**Scripting Mode**: ❌ **NOT USED**

IPP operations currently require LLM call per request. Action-based responses used.

**Future Enhancement**: Implement scripting for IPP operations:
- Script handles Get-Printer-Attributes with fixed attributes
- Script handles Print-Job with fixed job ID assignment
- Reduce per-request LLM calls to zero

## Client Library

**HTTP Client**: `reqwest` v0.11
- Used for HTTP POST requests to IPP server
- Supports custom headers (Content-Type: application/ipp)
- No dedicated IPP client library

**Manual IPP Encoding**: Tests build IPP packets manually
- Binary format construction (version, operation, attributes)
- Attribute encoding with tag-length-value format
- No parsing library (inspect raw response bytes)

**Why reqwest?** Simple HTTP client, sufficient for IPP-over-HTTP testing.

## Expected Runtime

**Model**: qwen3-coder:30b
**Total Runtime**: ~60 seconds for full test suite

### Per-Test Breakdown
- **test_ipp_get_printer_attributes**: ~20s (startup + 1 request)
- **test_ipp_print_job**: ~20s (startup + 1 request)
- **test_ipp_basic_http**: ~20s (startup + 1 request)

**Factors**:
- No scripting = LLM call per request (slow)
- IPP binary format parsing adds minimal overhead
- HTTP transport is fast (milliseconds)

## Failure Rate

**Failure Rate**: **Low to Medium** (~5-10%)

### Common Failure Modes
1. **LLM doesn't return ipp_response action** - Missing or incorrect action type
2. **Malformed hex body** - LLM generates invalid hex string
3. **Timeout on LLM call** - Ollama overload or slow model

### Known Flaky Tests
- **test_ipp_get_printer_attributes** - Sometimes LLM returns empty body (5% failure)
- **test_ipp_print_job** - Occasionally times out on larger documents (2% failure)

### Mitigation
- Use clear prompts specifying exact action format
- 10-second timeout on HTTP requests
- Tests validate HTTP 200 even if IPP body is empty (graceful degradation)

## Test Cases

### 1. Get-Printer-Attributes
**Purpose**: Validate IPP printer query operation

**Test Flow**:
1. Start IPP server with printer attributes
2. Send Get-Printer-Attributes request (operation 0x000B)
3. Validate HTTP 200 response
4. Check IPP version and status code in response body

**Expected Result**:
- HTTP 200 OK
- IPP version 0x02 0x00
- IPP status 0x0000 (successful-ok)

### 2. Print-Job
**Purpose**: Validate IPP job submission operation

**Test Flow**:
1. Start IPP server accepting print jobs
2. Send Print-Job request (operation 0x0002) with document data
3. Validate HTTP 200 response
4. Check job attributes in response

**Expected Result**:
- HTTP 200 OK
- Job attributes (job-id, job-state)

### 3. Basic HTTP Communication
**Purpose**: Validate HTTP-level connectivity

**Test Flow**:
1. Start IPP server responding to all requests
2. Send HTTP GET request (not proper IPP)
3. Validate server responds (200 or 405 Method Not Allowed)

**Expected Result**:
- HTTP response (success or method not allowed)
- Server is listening and responsive

## Known Issues

### IPP Binary Format Complexity
- Manual encoding error-prone
- No IPP library for validation
- Tests rely on HTTP status, not full IPP compliance

### LLM Response Format
- LLM must generate hex-encoded IPP response body
- Difficult for LLM to produce valid binary IPP structures
- Often results in empty or malformed bodies

**Workaround**: Tests accept HTTP 200 even if body is empty.

### No Real IPP Client
- Tests use HTTP POST with crafted payloads
- Real IPP clients (CUPS) not tested
- May not work with actual printers

**Future**: Test with CUPS ipptool or Python IPP library.

## Running Tests

```bash
# Build release binary with all features
cargo build --release --all-features

# Run IPP E2E tests
cargo test --features e2e-tests,ipp --test server::ipp::test

# Run specific test
cargo test --features e2e-tests,ipp --test server::ipp::test test_ipp_get_printer_attributes
```

**IMPORTANT**: Always build release binary before running tests.

## Future Enhancements

### Scripting Mode
- Add scripting support for IPP operations
- Reduce LLM calls from 6 to 3 (startups only)
- Generate Python/JS handlers for Get-Printer-Attributes, Print-Job

### Real IPP Client Testing
- Use CUPS ipptool for protocol compliance
- Test with actual print clients (Windows, macOS)
- Validate full IPP attribute encoding

### Additional Operations
- Test Get-Jobs, Cancel-Job, Get-Job-Attributes
- Test multiple concurrent print jobs
- Test error handling (printer offline, job rejected)

### Performance Tests
- Measure LLM call latency per operation
- Test high-volume job submission
- Stress test with concurrent clients

## References

- [RFC 2910: Internet Printing Protocol/1.1](https://tools.ietf.org/html/rfc2910)
- [CUPS ipptool](https://www.cups.org/doc/man-ipptool.html)
- [reqwest HTTP client](https://docs.rs/reqwest)
