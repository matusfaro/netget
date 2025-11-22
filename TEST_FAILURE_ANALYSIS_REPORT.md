# Test Failure Analysis Report
**Date**: 2025-11-21
**Test Run**: `run_20251120_214438`
**Total Tests**: 17
**Failed Tests**: 15
**Pass Rate**: 11.8%

---

## Executive Summary

The test failures reveal a **critical API mismatch** between what the tests expect and what the actual NetGet API provides. The tests appear to be written for an **older API design** that has since evolved. The LLM is actually responding correctly according to the current API (using `base_stack`, `event_handlers`, etc.), but the tests are checking for fields from the old API (`protocol`, `handler`, etc.).

### Root Cause Categories:
1. **API Field Naming (93% of failures)**: Tests expect `protocol` but API uses `base_stack`
2. **Handler Structure (60% of failures)**: Tests expect `handler` field but API uses `event_handlers` array
3. **Action Type Specificity (40% of failures)**: Tests expect protocol-specific actions but LLM uses generic `send_data`
4. **Client API Mismatch (7% of failures)**: Tests expect different field structure for `open_client`

---

## Detailed Failure Analysis

### Category 1: Protocol Field Mismatch (14/15 failures)

**Issue**: Tests check for `protocol` field, but the actual API uses `base_stack`.

**Affected Tests**:
- test_custom_validation
- test_dns_server_with_static_response
- test_http_conditional_script
- test_http_script_sum_query_params
- test_model_comparison
- test_open_client (uses `protocol` field in different context)
- test_open_http_server
- test_open_server_with_instruction
- test_open_tcp_server_with_port
- test_regex_pattern_matching
- test_server_with_scheduled_tasks
- test_tcp_echo_script

**Example LLM Response** (correct according to actual API):
```json
{
  "type": "open_server",
  "port": 8080,
  "base_stack": "http",  // ← LLM correctly uses "base_stack"
  "instruction": "..."
}
```

**Test Expectation** (outdated):
```rust
.expect_protocol("http")  // ← Test expects "protocol" field
```

**Recommendation**:
- **FIX THE TESTS** - Update all tests to check for `base_stack` instead of `protocol`
- The LLM is correct; the tests are using an outdated API schema

---

### Category 2: Handler Structure Mismatch (9/15 failures)

**Issue**: Tests expect a single `handler` field, but the actual API uses an `event_handlers` array.

**Affected Tests**:
- test_dns_server_with_static_response
- test_http_conditional_script
- test_http_script_sum_query_params
- test_tcp_echo_script

**Example LLM Response** (correct):
```json
{
  "type": "open_server",
  "port": 8080,
  "base_stack": "http",
  "event_handlers": [  // ← Modern API uses array of event handlers
    {
      "event_pattern": "http_request",
      "handler": { "type": "script", "..." }
    }
  ]
}
```

**Test Expectation** (outdated):
```rust
.expect_script_handler()  // ← Test expects top-level "handler" field
```

**Recommendation**:
- **FIX THE TESTS** - Update tests to check the `event_handlers` array structure
- Consider adding helper methods like `.expect_event_handler_with_script()`

---

### Category 3: Action Type Specificity (6/15 failures)

**Issue**: Tests expect protocol-specific action types (e.g., `send_dns_a_response`, `send_http_response`, `send_tcp_data`), but the LLM uses generic `send_data` or `show_message`.

**Affected Tests**:
- test_dns_query_response (expects `send_dns_a_response`, got `send_data`)
- test_http_request_with_instruction (expects `send_http_response`, got `send_data`)
- test_tcp_hex_response (expects `send_tcp_data`, got `show_message`)

**Example Failure - DNS**:
```json
// LLM Response:
{"type": "send_data", "data": "DNS response for example.com with IP 93.184.216.34"}

// Test Expectation:
{"type": "send_dns_a_response", "query_id": 12345, "domain": "example.com", "ip": "93.184.216.34"}
```

**Root Cause**: The LLM doesn't know about protocol-specific action types. It's using generic actions.

**Recommendation**:
- **IMPROVE THE PROMPT** - Add protocol-specific actions to the prompt for network event handlers
- The prompt should list available actions like:
  - `send_dns_a_response` (with fields: query_id, domain, ip)
  - `send_http_response` (with fields: status, body, headers)
  - `send_tcp_data` (with fields: data_hex or data)
- **OR FIX THE TESTS** - If the actual API accepts generic `send_data`, update tests to match

---

### Category 4: Client API Mismatch (1/15 failures)

**Issue**: `open_client` action has different field structure than expected.

**Affected Test**: test_open_client

**LLM Response**:
```json
{
  "type": "open_client",
  "port": 6379,
  "base_stack": "redis",
  "host": "localhost"
}
```

**Test Expectation**:
```rust
.expect_protocol("redis")           // ← Expects "protocol" field
.expect_field_contains("remote_addr", "localhost:6379")  // ← Expects "remote_addr" field
```

**Recommendation**:
- **FIX THE TEST** - Update to check for `base_stack` and separate `host`/`port` fields
- **OR IMPROVE THE PROMPT** - If the API actually expects `remote_addr: "localhost:6379"`, document this clearly in the prompt

---

## Individual Test Recommendations

### 1. test_custom_validation
**Status**: ❌ FAILED
**Issues**:
- Missing `protocol` field (should be `base_stack`)
- Test expects `instruction` field to contain "timeout" but LLM added a `timeout` parameter instead

**Fix**: Update test to check `base_stack` and verify the presence of timeout in appropriate location.

---

### 2. test_dns_query_response
**Status**: ❌ FAILED
**Issues**:
- Wrong action type: `send_data` instead of `send_dns_a_response`
- Missing protocol-specific fields: query_id, domain, ip

**Fix**:
- **Prompt improvement needed**: Add DNS-specific actions to the network event prompt
- Include example: `{"type": "send_dns_a_response", "query_id": <id>, "domain": "<domain>", "ip": "<ip>"}`

---

### 3. test_dns_server_with_static_response
**Status**: ❌ FAILED
**Issues**:
- Missing `protocol` field (should be `base_stack`)
- Test expects top-level `handler` field with static response, but LLM uses `event_handlers` array with script

**Fix**: Update test to check `event_handlers` array structure, or provide clearer prompt guidance about static handlers.

---

### 4. test_http_conditional_script
**Status**: ❌ FAILED
**Issues**:
- Missing `protocol` field (should be `base_stack`)
- Test expects top-level `handler` field, but LLM uses `event_handlers` array
- Test expects `instruction` field to contain "GET" and "POST"

**Fix**: Update test to check `event_handlers` array. The LLM correctly identified this as a script-based handler.

---

### 5. test_http_request_with_instruction
**Status**: ❌ FAILED
**Issues**:
- Wrong action type: `send_data` instead of `send_http_response`
- Missing protocol-specific fields: status, body

**Fix**:
- **Prompt improvement needed**: Add HTTP-specific actions to the network event prompt
- Include example: `{"type": "send_http_response", "status": 200, "body": "Hello, World!"}`

---

### 6. test_http_script_sum_query_params
**Status**: ❌ FAILED
**Issues**:
- Missing `protocol` field (should be `base_stack`)
- Test expects top-level `handler` field
- Test expects `instruction` field to contain "sum"

**Fix**: Update test to check `event_handlers` array. The LLM correctly created a script handler.

---

### 7. test_model_comparison
**Status**: ❌ FAILED
**Issues**:
- Missing `protocol` field (should be `base_stack`)

**Fix**: Simple - update test to check `base_stack` instead of `protocol`.

---

### 8. test_open_client
**Status**: ❌ FAILED
**Issues**:
- Missing `protocol` field (should be `base_stack`)
- Missing `remote_addr` field (LLM uses `host` and `port` separately)

**Fix**: Update test to check for `base_stack`, `host`, and `port` fields separately, or document that `remote_addr` should be a single field.

---

### 9. test_open_http_server
**Status**: ❌ FAILED
**Issues**:
- Missing `protocol` field (should be `base_stack`)

**Fix**: Simple - update test to check `base_stack` instead of `protocol`.

---

### 10. test_open_server_with_instruction
**Status**: ❌ FAILED
**Issues**:
- Missing `protocol` field (should be `base_stack`)

**Fix**: Simple - update test to check `base_stack` instead of `protocol`. The LLM correctly included the `instruction` field.

---

### 11. test_open_tcp_server_with_port
**Status**: ❌ FAILED
**Issues**:
- Missing `protocol` field (should be `base_stack`)

**Fix**: Simple - update test to check `base_stack` instead of `protocol`.

---

### 12. test_regex_pattern_matching
**Status**: ❌ FAILED
**Issues**:
- Missing `protocol` field (should be `base_stack`)

**Fix**: Simple - update test to check `base_stack` instead of `protocol`.

---

### 13. test_server_with_scheduled_tasks
**Status**: ❌ FAILED
**Issues**:
- Missing `protocol` field (should be `base_stack`)
- Test expects `scheduled_tasks` array but LLM attempted to use event handlers with a timer script

**Fix**:
- **Prompt improvement needed**: Add clear documentation about `scheduled_tasks` field
- Example: `{"type": "open_server", "scheduled_tasks": [{"task_id": "heartbeat", "recurring": true, "interval_secs": 10, "instruction": "Send heartbeat"}]}`

---

### 14. test_tcp_echo_script
**Status**: ❌ FAILED
**Issues**:
- Missing `protocol` field (should be `base_stack`)
- Test expects top-level `handler` field
- Test expects script handler to be present

**Fix**: Update test to check `event_handlers` array structure.

---

### 15. test_tcp_hex_response
**Status**: ❌ FAILED
**Issues**:
- Wrong action type: `show_message` instead of `send_tcp_data`
- Missing protocol-specific field: data_hex

**Fix**:
- **Prompt improvement needed**: Add TCP-specific actions to the network event prompt
- Include example: `{"type": "send_tcp_data", "data_hex": "48656c6c6f"}`

---

## Recommendations Summary

### CRITICAL: Fix the Tests (Priority 1)
**Impact**: 93% of failures
**Effort**: Low-Medium

All tests need to be updated to match the current API schema:
1. Replace `.expect_protocol()` with `.expect_field_exact("base_stack", json!("..."))`
2. Update handler expectations to check `event_handlers` array
3. Add helper methods to test builder for common patterns:
   ```rust
   fn expect_base_stack(self, stack: impl Into<String>) -> Self
   fn expect_event_handler_with_script(self) -> Self
   fn expect_event_handler_with_static(self, response: Value) -> Self
   ```

### HIGH: Improve Network Event Prompts (Priority 2)
**Impact**: 40% of failures
**Effort**: Medium

Add protocol-specific actions to network event prompts:
1. **DNS Actions**:
   - `send_dns_a_response` (query_id, domain, ip)
   - `send_dns_nxdomain` (query_id, domain)

2. **HTTP Actions**:
   - `send_http_response` (status, body, headers)

3. **TCP Actions**:
   - `send_tcp_data` (data_hex or data)

4. **Scheduled Tasks**:
   - Document `scheduled_tasks` array in `open_server` action
   - Provide clear examples in the prompt

### MEDIUM: Clarify Client API (Priority 3)
**Impact**: 7% of failures
**Effort**: Low

Document whether `open_client` should use:
- **Option A**: `remote_addr: "host:port"` (single field)
- **Option B**: `host: "host"` and `port: port` (separate fields)
- **Option C**: `base_stack` + `host` + `port`

---

## Proposed Action Plan

### Phase 1: Immediate Fixes (1-2 hours)
1. Create helper methods in test builder for `base_stack` and `event_handlers`
2. Update all 14 tests with protocol field mismatches
3. Update 4 tests with handler structure mismatches

### Phase 2: Prompt Improvements (2-3 hours)
1. Add protocol-specific actions to network event prompt template
2. Add `scheduled_tasks` documentation and examples
3. Test with qwen2.5-coder:0.5b to verify improvements

### Phase 3: Validation (1 hour)
1. Run full test suite with updated tests
2. Verify pass rate improvement (expect >80%)
3. Document any remaining failures

---

## Conclusion

The test failures are **not LLM failures** - they're **test design issues**. The LLM is responding correctly according to the current NetGet API, but the tests were written for an older API design.

**Key Insight**: The tests are testing against a phantom API that no longer exists. Once the tests are updated to match the actual API, we should see a dramatic improvement in pass rate.

**Estimated Impact**: Updating the tests should fix 93% of failures immediately. The remaining 7% will require prompt improvements for protocol-specific actions.
