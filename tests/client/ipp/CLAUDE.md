# IPP Client E2E Test Strategy

## Overview

End-to-end tests for the IPP client verify LLM-controlled printing operations against a real or mock IPP print server.

## Test Philosophy

**Black-box testing**: Tests validate that the LLM can successfully:
1. Connect to an IPP printer
2. Query printer capabilities
3. Submit print jobs
4. Check job status

Tests use **real IPP operations** (not mocked responses) to ensure correct protocol behavior.

## Test Suite

### Test 1: `test_ipp_get_printer_attributes`
**Purpose**: Verify the client can query printer capabilities

**Setup**:
- IPP client connects to `http://localhost:631/printers/test-printer`
- LLM instruction: "Query the printer and tell me its capabilities"

**Actions**:
1. Client connects
2. Manually trigger `get_printer_attributes` operation
3. Verify response received
4. Check status messages for success

**Expected LLM Calls**: 1-2
- Initial connection (optional)
- Response processing

**Runtime**: ~5-8 seconds

**Validation**:
- Client status is `Connected`
- Status messages contain "IPP client" and "received response"

---

### Test 2: `test_ipp_print_job`
**Purpose**: Verify the client can submit a print job

**Setup**:
- IPP client connects to test printer
- LLM instruction: "Print a test page with the text 'NetGet IPP Test'"

**Actions**:
1. Client connects
2. Manually trigger `print_job` with test document
3. Verify print job submitted successfully
4. Check for job response

**Expected LLM Calls**: 1-2
- Initial connection (optional)
- Print job response processing

**Runtime**: ~5-8 seconds

**Validation**:
- Client status is `Connected`
- Status messages contain "Print-Job" or "print_job"
- No error messages

---

### Test 3: `test_ipp_full_workflow`
**Purpose**: Comprehensive workflow test (query → print → check status)

**Setup**:
- IPP client connects to test printer
- LLM instruction: "First query the printer capabilities, then print a test page, and finally check the job status"

**Actions**:
1. Client connects
2. Query printer attributes
3. Submit print job
4. Query job status (with placeholder job_id)

**Expected LLM Calls**: 3-4
- Connection
- Each operation response

**Runtime**: ~10-15 seconds

**Validation**:
- All operations complete without errors
- Client remains connected throughout

---

## LLM Call Budget

**Total for suite**: < 10 LLM calls

Breakdown:
- Test 1: 1-2 calls
- Test 2: 1-2 calls
- Test 3: 3-4 calls
- **Total**: 5-8 calls (well under budget)

## Prerequisites

### Required Services

1. **IPP/CUPS Server**:
   - Install CUPS: `sudo apt-get install cups` (Debian/Ubuntu) or `brew install cups` (macOS)
   - Start CUPS: `sudo systemctl start cups`
   - Add test printer:
     ```bash
     lpadmin -p test-printer -E -v file:/dev/null -m drv:///sample.drv/generic.ppd
     ```
   - Verify: `lpstat -p test-printer`

2. **Ollama**:
   - Running on default endpoint
   - Model available (e.g., qwen3-coder:30b)

### Alternative: Mock Server

For CI/CD environments without CUPS, consider:
- Mock IPP server that responds to Get-Printer-Attributes and Print-Job
- Minimal HTTP server returning valid IPP response bytes
- Or use Docker: `docker run -d -p 631:631 cups/cups`

## Running Tests

```bash
# All IPP client tests (requires CUPS and Ollama)
./cargo-isolated.sh test --no-default-features --features ipp --test client::ipp::e2e_test -- --ignored

# Specific test
./cargo-isolated.sh test --no-default-features --features ipp test_ipp_get_printer_attributes -- --ignored

# Without Ollama lock (faster, but may conflict)
./cargo-isolated.sh test --no-default-features --features ipp --test client::ipp::e2e_test
```

**Note**: Tests are marked `#[ignore]` because they require external services. Use `-- --ignored` to run them.

## Known Issues

1. **Job ID Extraction**: Current implementation doesn't parse job_id from Print-Job response
   - Test 3 uses placeholder job_id
   - Fix: Parse job-id attribute from response

2. **CUPS Permissions**: May require user to be in `lpadmin` group
   - Fix: `sudo usermod -a -G lpadmin $USER` and re-login

3. **Printer Not Found**: If test printer doesn't exist, operations fail
   - Fix: Create test printer as shown in Prerequisites

4. **Network Timeouts**: IPP operations may time out on slow systems
   - Fix: Increase sleep durations in tests

## Future Improvements

1. **Mock IPP Server**: Bundle a minimal IPP server for CI/CD
2. **Job ID Parsing**: Extract and use real job IDs from Print-Job responses
3. **Attribute Validation**: Assert specific printer attributes in responses
4. **Error Cases**: Test invalid URIs, missing printers, malformed requests
5. **LLM-Driven Tests**: Let LLM generate actions instead of manual triggers
6. **TLS Support**: Test IPPS (IPP over TLS) when implemented

## Maintenance Notes

- **CUPS Version**: Tests assume CUPS 2.x compatibility
- **Port Conflicts**: Default port 631 may conflict with system CUPS
  - Use test instance on alternate port if needed
- **Cleanup**: Tests don't delete print jobs, may accumulate in queue
  - Periodic cleanup: `cancel -a -x` (cancel all jobs)

## Test Efficiency Checklist

✅ **LLM Calls**: < 10 (5-8 actual)
✅ **Runtime**: < 30 seconds total
✅ **No External Network**: Localhost only
✅ **Feature Gated**: `#[cfg(all(test, feature = "ipp"))]`
✅ **Ignored by Default**: Requires `-- --ignored` flag
✅ **Documented Prerequisites**: CUPS and Ollama
✅ **Black-box Approach**: Uses public APIs only
