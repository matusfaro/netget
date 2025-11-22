# Test Failure Analysis - Run 20251120_230418
**Date**: 2025-11-21
**Model**: qwen3-coder:30b
**Pass Rate**: 10/17 (58.8%)
**Previous Pass Rate**: 7/17 (41.2%)
**Improvement**: +17.6% (+3 tests)

---

## Executive Summary

The prompt fixes were **partially successful**, improving the pass rate from 41.2% to 58.8%. However, **protocol-specific action guidance is still not being followed** by the LLM.

### What Worked ✅

1. **Base stack naming** - Fixed 1 test
2. **Instruction field documentation** - Fixed 1 test
3. **Client API field structure** - Fixed 1 test

### What Didn't Work ❌

1. **Protocol-specific actions** - LLM still uses `send_data` and `show_message`
2. **Scheduled tasks** - LLM still doesn't generate `scheduled_tasks` array
3. **Script instruction context** - Scripts fail due to missing instruction field

---

## Test Results Comparison

### ✅ Newly Passing Tests (3)

| Test | Previous | Current | Fix That Worked |
|------|----------|---------|-----------------|
| test_dns_server_with_static_response | ❌ | ✅ | Base stack naming + instruction field |
| test_open_client | ❌ | ✅ | host/port field structure |
| test_open_server_with_instruction | ❌ | ✅ | Instruction field documentation |

### ✅ Still Passing Tests (7)

- test_close_server
- test_custom_validation
- test_model_comparison
- test_multiple_actions
- test_open_http_server
- test_open_tcp_server_with_port
- test_regex_pattern_matching

### ❌ Still Failing Tests (7)

| Test | Failure Reason | Category |
|------|----------------|----------|
| test_dns_query_response | Uses `send_data` instead of `send_dns_a_response` | Protocol actions |
| test_http_request_with_instruction | Uses `send_data` instead of `send_http_response` | Protocol actions |
| test_tcp_hex_response | Uses `show_message` instead of `send_tcp_data` | Protocol actions |
| test_tcp_echo_script | Script returns `send_data` with wrong structure | Protocol actions |
| test_http_conditional_script | Missing instruction field + script execution | Script context |
| test_http_script_sum_query_params | Missing instruction field + script execution | Script context |
| test_server_with_scheduled_tasks | No `scheduled_tasks` array generated | Feature awareness |

---

## Detailed Failure Analysis

### Category 1: Protocol-Specific Actions NOT Being Used (4 failures)

**The Problem**: Despite adding protocol-specific action documentation to `prompts/network_request/partials/instructions.hbs`, the LLM is **still using generic actions**.

#### test_dns_query_response
```
Expected: {"type": "send_dns_a_response", "query_id": 12345, "domain": "example.com", "ip": "93.184.216.34"}
Got:      {"type": "send_data", ...}
```

#### test_http_request_with_instruction
```
Expected: {"type": "send_http_response", "status": 200, "body": "Hello, World!"}
Got:      {"type": "send_data", ...}
```

#### test_tcp_hex_response
```
Expected: {"type": "send_tcp_data", "data_hex": "48656c6c6f"}
Got:      {"type": "show_message", ...}
```

#### test_tcp_echo_script
```
Expected: {"type": "send_tcp_data", "data_hex": "48656c6c6f"}
Got:      {"type": "send_data", "data": {"data_hex": "48656c6c6f"}}
```

**Root Cause**: The protocol-specific action documentation is in the network_request instructions template, but it may not be prominent enough OR the LLM needs to see these actions in the **Available Actions** list, not just in the instructions.

**Recommended Fix**:
1. **Check if network_request prompt is actually being used** for these tests
2. **Move protocol-specific examples to the action definitions** themselves (in `src/llm/actions/`)
3. **Make the examples more prominent** - add them to action descriptions, not just instructions

---

### Category 2: Script Execution Issues (2 failures)

**The Problem**: Scripts fail because the `open_server` action doesn't include the `instruction` field, so the script doesn't have context.

#### test_http_conditional_script
```
Error: Field 'instruction' contains 'GET': Action has no 'instruction' field
Error: Field 'instruction' contains 'POST': Action has no 'instruction' field
Error: Script execution test: Failed to execute script
```

#### test_http_script_sum_query_params
```
Error: Field 'instruction' contains 'sum': Action has no 'instruction' field
Error: Script execution test: Failed to execute script
```

**Root Cause**: While we documented that instructions should be included, the LLM is **still not including them consistently** when generating scripted server actions.

**Recommended Fix**:
1. **Make instruction field REQUIRED** in the `open_server` action definition
2. **Add validation** that rejects `open_server` without instruction if event_handlers are present
3. **Update examples** to always show instruction field

---

### Category 3: Scheduled Tasks Not Generated (1 failure)

**The Problem**: Despite documenting the `scheduled_tasks` feature, the LLM doesn't generate it.

#### test_server_with_scheduled_tasks
```
Error: Custom validation 'has scheduled tasks' failed
```

**Root Cause**: The scheduled_tasks documentation may not be prominent enough, OR the user prompt doesn't explicitly ask for scheduled tasks.

**Investigation Needed**: Check the test's user input to see if it explicitly mentions "scheduled" or "periodic" tasks.

---

## Key Insights

### 1. Prompt Instructions vs Action Definitions

The prompt template instructions (`prompts/network_request/partials/instructions.hbs`) are **not as effective** as action definitions (`src/llm/actions/`).

**Hypothesis**: LLMs pay more attention to:
- The **Available Actions** list with examples
- Action parameter descriptions
- Action examples in the definitions

Rather than:
- General instructions in the prompt body
- Documentation paragraphs

### 2. User Input Prompt vs Network Event Prompt

We updated `prompts/user_input/partials/instructions.hbs` successfully (3 tests fixed), but `prompts/network_request/partials/instructions.hbs` changes had **no effect** (0 tests fixed from network event category).

**This suggests**: The network event tests may not be using the network_request prompt, OR the action definitions override the instructions.

---

## Recommended Next Steps

### HIGH PRIORITY: Update Action Definitions

Instead of documenting protocol-specific actions in the prompt instructions, **add them to the action definitions themselves**:

1. **Check `src/llm/actions/network_event.rs`** (or similar) for network event actions
2. **Add protocol-specific action definitions** with clear examples
3. **Make `send_data` and `show_message` deprecated or conditional**

Example action definition improvement:
```rust
ActionDefinition::new("send_http_response")
    .with_description("Send HTTP response to client. Use this for HTTP servers.")
    .with_parameter("status", ParameterType::Number, "HTTP status code (200, 404, etc.)", true)
    .with_parameter("body", ParameterType::String, "Response body content", true)
    .with_parameter("headers", ParameterType::Object, "HTTP headers", false)
    .with_example(json!({
        "type": "send_http_response",
        "status": 200,
        "body": "Hello, World!",
        "headers": {"Content-Type": "text/plain"}
    }))
```

### MEDIUM PRIORITY: Investigate Test Context

1. **Check which prompt is used** for network event tests
2. **Verify the action list** provided to the LLM includes protocol-specific actions
3. **Check if the tests are using the right prompt builder**

### LOW PRIORITY: Improve Instruction Field Handling

1. Make `instruction` field required for `open_server` when using event_handlers
2. Add validation that warns if instruction is missing
3. Update examples to always include instruction

---

## Progress Tracking

### Fixes That Worked (3/5) ✅

1. ✅ **Base stack naming** - LLM now uses simple protocol names
2. ✅ **Instruction field (user input)** - LLM includes instruction in open_server
3. ✅ **Client API** - LLM uses host/port separately

### Fixes That Didn't Work (2/5) ❌

4. ❌ **Protocol-specific actions** - LLM ignores instructions, still uses generic actions
5. ❌ **Scheduled tasks** - LLM doesn't generate scheduled_tasks array

---

## Statistical Comparison

| Metric | Run 1 (Before Fixes) | Run 2 (After Fixes) | Change |
|--------|----------------------|---------------------|--------|
| **Pass Rate** | 41.2% (7/17) | 58.8% (10/17) | **+17.6%** |
| **Tests Fixed** | - | 3 tests | +3 |
| **Protocol Action Issues** | 6 tests | **4 tests** | **-2** (partial) |
| **Instruction Field Issues** | 3 tests | **2 tests** | **-1** (partial) |
| **Base Stack Issues** | 2 tests | **0 tests** | **-2** ✅ |
| **Client API Issues** | 1 test | **0 tests** | **-1** ✅ |
| **Scheduled Tasks Issues** | 1 test | **1 test** | 0 |

**Overall**: Partial success. Some categories fully fixed, others still problematic.

---

## Conclusion

The prompt fixes were **partially effective**. We successfully fixed:
- Base stack naming issues (100% success)
- Client API field structure (100% success)
- Instruction field for user input (33% success)

However, **protocol-specific actions are still not being used** despite clear documentation. This suggests:
1. **Action definitions matter more than prompt instructions**
2. **We need to update the action registration code**, not just the prompts
3. **The LLM may not be seeing the protocol-specific actions in the Available Actions list**

**Next action**: Investigate `src/llm/actions/` to add protocol-specific action definitions or modify how actions are provided to the LLM.

**Expected additional improvement**: +2-4 tests (to 70-82% pass rate) if protocol-specific actions can be properly registered and surfaced to the LLM.
