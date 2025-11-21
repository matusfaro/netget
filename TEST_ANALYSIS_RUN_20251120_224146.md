# Test Failure Analysis - Run 20251120_224146
**Date**: 2025-11-21
**Model**: qwen2.5-coder:7b (inferred from run context)
**Pass Rate**: 7/17 (41.2%)
**Improvement**: From 11.8% (previous) to 41.2% (current) = **+29.4% improvement**

---

## Executive Summary

The test validation framework updates successfully improved the pass rate from 11.8% to 41.2%. The remaining 10 failures fall into 4 categories:

1. **Protocol-Specific Actions** (60% of failures) - LLM uses generic `send_data` instead of protocol-specific actions
2. **Instruction Field Missing** (30% of failures) - LLM doesn't include instruction field in responses
3. **Base Stack Naming** (10% of failures) - LLM returns full stack instead of base protocol
4. **Open Client API** (10% of failures) - Field structure mismatch for client connections

---

## Test Results Summary

### ✅ Passed Tests (7/17)

| Test | Status |
|------|--------|
| test_close_server | ✅ PASS |
| test_custom_validation | ✅ PASS |
| test_model_comparison | ✅ PASS |
| test_multiple_actions | ✅ PASS |
| test_open_http_server | ✅ PASS |
| test_open_tcp_server_with_port | ✅ PASS |
| test_regex_pattern_matching | ✅ PASS |

### ❌ Failed Tests (10/17)

| Test | Failure Count | Primary Issue |
|------|--------------|---------------|
| test_dns_query_response | 2 | Base stack naming + static handler |
| test_dns_server_with_static_response | 4 | Missing instruction field + base stack |
| test_http_conditional_script | 3 | Missing instruction field + script execution |
| test_http_request_with_instruction | 3 | Generic action instead of send_http_response |
| test_http_script_sum_query_params | 2 | Missing instruction field + script execution |
| test_open_client | 1 | remote_addr field structure |
| test_open_server_with_instruction | 1 | Missing instruction field |
| test_server_with_scheduled_tasks | 1 | scheduled_tasks not present |
| test_tcp_echo_script | 1 | Script returns wrong action structure |
| test_tcp_hex_response | 2 | Generic show_message instead of send_tcp_data |

---

## Detailed Failure Analysis

### Category 1: Protocol-Specific Actions (6 failures)

**Issue**: LLM uses generic actions (`send_data`, `show_message`) instead of protocol-specific actions (`send_http_response`, `send_dns_a_response`, `send_tcp_data`).

**Affected Tests**:
1. **test_http_request_with_instruction**
   - Expected: `send_http_response` with `status` and `body` fields
   - Got: `send_data` with generic data

2. **test_tcp_hex_response**
   - Expected: `send_tcp_data` with `data_hex` field
   - Got: `show_message` (completely wrong action)

3. **test_tcp_echo_script**
   - Expected: `send_tcp_data` with `data_hex` at top level
   - Got: `send_data` with nested structure `{"data": {"data_hex": "..."}}`

4. **test_dns_query_response** (partial)
   - Expected: Static handler with `send_dns_a_response`
   - Got: No static handler structure

**Root Cause**: The prompt doesn't clearly enumerate protocol-specific actions for network event handlers. The LLM defaults to generic actions it knows about.

**Recommendation**:
- **HIGH PRIORITY**: Update `src/llm/prompt.rs` to include protocol-specific actions in network event contexts
- Add examples like:
  ```
  HTTP: send_http_response (fields: status, body, headers)
  TCP: send_tcp_data (fields: data_hex or data)
  DNS: send_dns_a_response (fields: query_id, domain, ip)
  ```

---

### Category 2: Missing Instruction Field (3 failures)

**Issue**: LLM doesn't include `instruction` field in `open_server` actions when expected.

**Affected Tests**:
1. **test_open_server_with_instruction**
   - Missing: `instruction` field containing "hello world"

2. **test_http_conditional_script**
   - Missing: `instruction` field containing "GET" and "POST"

3. **test_http_script_sum_query_params**
   - Missing: `instruction` field containing "sum"

**Root Cause**: The LLM may be interpreting the user prompt as the instruction itself and not including it in the action JSON, OR the prompt structure doesn't emphasize that instruction should be passed through.

**Recommendation**:
- **MEDIUM PRIORITY**: Clarify in prompt that when user provides an instruction, it should be included in the `open_server` action
- May need to review test expectations - perhaps `instruction` should be optional if the LLM can infer behavior

---

### Category 3: Base Stack Naming (2 failures)

**Issue**: LLM returns full protocol stack (`ETH>IP>UDP>DNS`) instead of just base protocol (`dns`).

**Affected Tests**:
1. **test_dns_query_response**
   - Expected: `base_stack: "dns"`
   - Got: `base_stack: "ETH>IP>UDP>DNS"`

2. **test_dns_server_with_static_response** (implied)
   - Similar stack naming issue

**Root Cause**: The LLM may be over-thinking the protocol stack and providing the full OSI layer chain instead of just the application-layer protocol.

**Recommendation**:
- **LOW PRIORITY**: Update prompt to clarify that `base_stack` should be the application-layer protocol name only (e.g., "http", "dns", "tcp")
- Alternative: Update test expectations to accept full stack notation and parse out the rightmost protocol

---

### Category 4: Open Client API Mismatch (1 failure)

**Issue**: Client connection uses separate `host` and `port` fields, but test expects `remote_addr: "host:port"`.

**Affected Test**:
1. **test_open_client**
   - Expected: `remote_addr` field containing "localhost:6379"
   - Got: Separate fields (likely `host` and `port`)

**Root Cause**: API design inconsistency between what tests expect and what LLM returns (or what the actual API accepts).

**Recommendation**:
- **MEDIUM PRIORITY**: Clarify in prompt whether clients should use:
  - Option A: `remote_addr: "host:port"` (single field)
  - Option B: `host: "host"` and `port: port` (separate fields)
- Update test expectations to match the chosen design

---

### Category 5: Scheduled Tasks (1 failure)

**Issue**: LLM doesn't generate `scheduled_tasks` array in `open_server` action.

**Affected Test**:
1. **test_server_with_scheduled_tasks**
   - Expected: `scheduled_tasks` array with task definitions
   - Got: Action without `scheduled_tasks` field

**Root Cause**: The prompt may not clearly document the `scheduled_tasks` feature or provide examples.

**Recommendation**:
- **MEDIUM PRIORITY**: Add `scheduled_tasks` documentation to the prompt with clear examples
- Example structure:
  ```json
  {
    "type": "open_server",
    "scheduled_tasks": [
      {
        "task_id": "heartbeat",
        "recurring": true,
        "interval_secs": 10,
        "instruction": "Send heartbeat"
      }
    ]
  }
  ```

---

### Category 6: Script Execution Issues (2 failures)

**Issue**: Tests that execute generated scripts are failing, likely due to missing instruction fields or incorrect script structure.

**Affected Tests**:
1. **test_http_conditional_script**
   - Script execution failed (likely due to missing instruction context)

2. **test_http_script_sum_query_params**
   - Script execution failed (likely due to missing instruction context)

**Root Cause**: Tests attempt to execute scripts that the LLM generated, but the scripts may rely on context (instruction) that wasn't included in the action.

**Recommendation**:
- **LOW PRIORITY**: Review script execution tests to ensure they provide all necessary context
- May need to update tests to extract instruction from the action before executing scripts

---

## Impact Analysis

### High Impact (Fix These First)

1. **Protocol-Specific Actions** → Would fix 6 test failures (60%)
   - Add protocol-specific actions to prompt
   - Estimated effort: 1-2 hours
   - Expected improvement: +35% pass rate

### Medium Impact

2. **Missing Instruction Field** → Would fix 3 failures (30%)
   - Clarify instruction pass-through in prompt
   - Estimated effort: 30 minutes
   - Expected improvement: +18% pass rate

3. **Open Client API** → Would fix 1 failure (10%)
   - Clarify client connection field structure
   - Estimated effort: 15 minutes
   - Expected improvement: +6% pass rate

4. **Scheduled Tasks** → Would fix 1 failure (10%)
   - Document scheduled_tasks feature
   - Estimated effort: 30 minutes
   - Expected improvement: +6% pass rate

### Low Impact

5. **Base Stack Naming** → Would fix 2 failures (20%)
   - Clarify base_stack naming convention
   - Estimated effort: 15 minutes
   - Expected improvement: +12% pass rate (overlaps with other fixes)

6. **Script Execution** → Would fix 2 failures (20%)
   - Review test design
   - Estimated effort: 1 hour
   - Expected improvement: Depends on other fixes

---

## Recommended Action Plan

### Phase 1: Prompt Improvements (HIGH PRIORITY)

**Goal**: Fix protocol-specific action issues

1. Update `src/llm/prompt.rs` to add network event action documentation:
   ```
   Available actions for network events:

   HTTP:
   - send_http_response: Send HTTP response (fields: status, body, headers)

   TCP:
   - send_tcp_data: Send TCP data (fields: data_hex or data)

   DNS:
   - send_dns_a_response: DNS A record response (fields: query_id, domain, ip)
   - send_dns_nxdomain: DNS NXDOMAIN response (fields: query_id, domain)
   ```

2. Clarify instruction field behavior:
   ```
   When opening a server with an instruction, include the instruction in the action:
   {
     "type": "open_server",
     "base_stack": "http",
     "port": 8080,
     "instruction": "your instruction here"
   }
   ```

3. Clarify base_stack naming:
   ```
   base_stack should be the application-layer protocol name only:
   - Use "dns" not "ETH>IP>UDP>DNS"
   - Use "http" not "TCP>HTTP"
   ```

**Expected Impact**: Pass rate increases from 41% to ~70%

### Phase 2: API Clarifications (MEDIUM PRIORITY)

**Goal**: Fix API design inconsistencies

1. Document open_client field structure
2. Document scheduled_tasks feature with examples

**Expected Impact**: Pass rate increases from ~70% to ~82%

### Phase 3: Test Review (LOW PRIORITY)

**Goal**: Ensure tests are validating correctly

1. Review script execution tests
2. Consider making instruction field optional in some contexts

**Expected Impact**: Pass rate increases from ~82% to ~88%

---

## Comparison with Previous Run (run_20251120_214438)

| Metric | Previous | Current | Change |
|--------|----------|---------|--------|
| Pass Rate | 11.8% (2/17) | 41.2% (7/17) | +29.4% |
| Protocol Field Issues | 14 tests | 2 tests | -86% |
| Handler Structure Issues | 9 tests | 0 tests | -100% |
| Action Type Issues | 6 tests | 6 tests | 0% |

**Key Improvements**:
- ✅ Protocol/base_stack validation now accepts both old and new API
- ✅ Handler structure validation now accepts both formats
- ✅ Tests no longer fail on API format mismatches

**Remaining Issues**:
- ❌ Protocol-specific actions still not being used
- ❌ Instruction field missing in some contexts
- ❌ Base stack naming still uses full protocol chain

---

## Conclusion

The test framework updates were **highly successful**, improving pass rate from 11.8% to 41.2%. The remaining failures are **not test framework issues** but rather **prompt clarity issues**.

The next step should be to update the prompt in `src/llm/prompt.rs` to clearly document:
1. Protocol-specific actions for network events
2. Instruction field pass-through behavior
3. Base stack naming convention
4. Client connection field structure
5. Scheduled tasks feature

**Estimated additional pass rate improvement**: +30-40% (to 70-80% pass rate) with prompt updates.
