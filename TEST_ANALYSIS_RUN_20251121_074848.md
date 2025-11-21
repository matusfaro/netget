# Test Analysis: run_20251121_074848

## Summary

**Date**: November 21, 2025 07:48:48
**Model**: qwen3-coder:30b
**Pass Rate**: 52.9% (9/17 tests passing)
**Previous Run**: 58.8% (10/17 tests passing)
**Change**: **-5.9%** (REGRESSION - 1 test that was passing is now failing)

## Test Results

### ✅ Passing Tests (9/17)

1. `test_close_server` - ✓
2. `test_custom_validation` - ✓
3. `test_model_comparison` - ✓
4. `test_multiple_actions` - ✓
5. `test_open_client` - ✓
6. `test_open_http_server` - ✓
7. `test_open_server_with_instruction` - ✓
8. `test_open_tcp_server_with_port` - ✓
9. `test_regex_pattern_matching` - ✓

### ❌ Failing Tests (8/17)

1. `test_dns_query_response` - FAILED
2. `test_dns_server_with_static_response` - FAILED
3. `test_http_conditional_script` - FAILED
4. `test_http_request_with_instruction` - FAILED
5. `test_http_script_sum_query_params` - FAILED
6. `test_server_with_scheduled_tasks` - FAILED
7. `test_tcp_echo_script` - FAILED
8. `test_tcp_hex_response` - FAILED

## Detailed Failure Analysis

### test_dns_query_response

**Expected**: LLM uses `send_dns_a_response` action
**Actual**: LLM used `send_data` action

**LLM Response**:
```json
{
  "type": "send_data",
  "data": "DNS response for example.com with IP 192.168.1.1"
}
```

**Errors**:
- ❌ Action type is 'send_dns_a_response': Expected type 'send_dns_a_response', got 'send_data'
- ❌ Field 'query_id' equals 12345: Action has no 'query_id' field
- ❌ Field 'domain' equals "example.com": Action has no 'domain' field
- ❌ Field 'ip' equals "93.184.216.34": Action has no 'ip' field

**Issue**: Despite enhanced action descriptions in `src/server/dns/actions.rs`, the LLM still used generic `send_data` instead of the protocol-specific `send_dns_a_response`.

## Analysis of Changes

### What Changed Since Previous Run

1. **Enhanced HTTP action description** in `src/server/http/actions.rs`:
   - Added "IMPORTANT: Use this action to respond to HTTP requests. This is the ONLY correct action..."

2. **Enhanced TCP action description** in `src/server/tcp/actions.rs`:
   - Added "IMPORTANT: Use this action to send data over TCP connections. This is the ONLY correct action..."

3. **Enhanced DNS action descriptions** in `src/server/dns/actions.rs`:
   - `send_dns_a_response`: Added "IMPORTANT: Use this action to respond to DNS A record queries..."
   - `send_dns_nxdomain`: Added "IMPORTANT: Use this action to respond when a DNS domain does not exist..."

4. **Simplified prompt template** in `prompts/network_request/partials/instructions.hbs`:
   - Removed 40+ lines of protocol-specific examples
   - Replaced with general instruction to check "Available Actions" section

### Why the Regression?

**Hypothesis**: The changes may have HURT more than helped because:

1. **Removed Examples**: By removing the explicit examples from the prompt template, we may have reduced the LLM's exposure to correct usage patterns.

2. **Description Length**: The very long "IMPORTANT" descriptions may be overwhelming or buried in the action definition text. LLMs may not read/process long descriptions as effectively as we hoped.

3. **Action Selection Mechanism**: The issue may not be with the action descriptions at all, but with how the LLM selects actions from the available list. The LLM may be:
   - Not reading the action descriptions carefully enough
   - Defaulting to familiar patterns (generic `send_data`)
   - Not understanding the connection between the event type (DNS query) and the appropriate action

4. **Prompt Structure**: The generic prompt instructions may have been providing crucial guidance that we removed.

## Comparison with Previous Run (20251120_230418)

### Tests that Changed Status

- **Regression**: 1 test went from PASS → FAIL (need to identify which one)

### Tests with Same Failures

All 8 failing tests appear to be the same as in the previous run, with one additional failure.

## Root Cause Analysis

The fundamental issue is: **LLM is not using protocol-specific actions despite clear action definitions.**

### Possible Root Causes

1. **Action List Overload**: The LLM sees too many actions and defaults to generic ones
2. **Description Fatigue**: Very long descriptions may cause the LLM to skim or ignore them
3. **Pattern Matching**: LLM matches user intent to familiar action names (`send_data` sounds reasonable for "send DNS data")
4. **Prompt Structure**: The way actions are presented in the prompt may not emphasize protocol-specific actions enough
5. **Training Data Bias**: The LLM's training may have strong associations with generic action names

## Recommendations

### Immediate Next Steps

1. **Revert Changes**: Consider reverting the prompt template changes and keeping the protocol-specific examples visible in the instructions
2. **Hybrid Approach**: Keep both the explicit examples AND the enhanced action descriptions
3. **Action Name Emphasis**: Make protocol-specific action names more distinctive (but this would be a breaking change)

### Alternative Approaches

1. **System Prompt Enhancement**: Add a system-level instruction at the very beginning: "CRITICAL: Always use protocol-specific actions. Never use generic send_data for network protocols."

2. **Negative Examples**: Add explicit warnings in prompt about what NOT to do:
   ```
   ❌ WRONG: {"type": "send_data", ...}
   ✅ CORRECT: {"type": "send_dns_a_response", ...}
   ```

3. **Action Filtering**: Programmatically filter out generic actions when protocol-specific ones are available (code change)

4. **Few-Shot Learning**: Provide 2-3 complete examples of correct action usage in the prompt, showing:
   - DNS query → send_dns_a_response
   - HTTP request → send_http_response
   - TCP data → send_tcp_data

5. **Model Fine-Tuning**: Consider if the model itself needs fine-tuning on protocol-specific action selection

## Conclusion

The action definition enhancements did NOT improve test pass rates. In fact, we saw a small regression (-5.9%).

**Key Insight**: Moving examples from the prompt template to action descriptions may have made them LESS visible to the LLM. Action descriptions might be processed differently than prompt instructions.

**Recommended Action**: Revert the prompt template changes to restore the explicit protocol-specific examples, while keeping the enhanced action descriptions (belt-and-suspenders approach).
