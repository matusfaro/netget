# Dynamic Examples System - Comprehensive Analysis

**Date**: 2025-11-21
**Author**: Claude
**Purpose**: Identify all locations where static examples exist or could be added, and design a comprehensive dynamic example system

---

## Executive Summary

This analysis identifies **8 distinct prompt contexts** where examples appear, plus **6 additional opportunities** for examples. The core problem: **static examples teach the LLM to use wrong action types**. Solution: Make all examples dynamic and context-aware.

### Key Findings

1. **Static Examples Found**: 7 locations with hardcoded `send_data`, `show_message`, generic actions
2. **Missing Examples**: 6 locations that could benefit from examples
3. **Already Dynamic**: 2 locations (easy_request templates)
4. **Critical Issue**: User input examples after `read_base_stack_docs` are static and don't reflect the protocol

---

## Part 1: Existing Examples (Static → Make Dynamic)

### 1.1 Network Request Prompt (`prompts/network_request/`)

**File**: `prompts/network_request/main.hbs`
**Context**: LLM handling a network event (TCP data received, HTTP request, etc.)
**Current State**: No direct examples, but includes scripting.hbs which has static ones
**Problem**: N/A (inherits from shared partials)
**Solution**: Add event-specific example section

**Proposed Addition**:
```handlebars
{{#if event_examples}}
## Response Examples for This Event

{{#each event_examples}}
{{#if (eq this.0 "mandatory")}}
**MANDATORY Response Format:**
{{else if (eq this.0 "typical")}}
**Typical Response (Recommended):**
{{else}}
**Alternative Response:**
{{/if}}
```json
{"actions": [{{{this.1}}}]}
```
{{/each}}
{{/if}}
```

---

### 1.2 Scripting Mode Examples (`prompts/shared/partials/scripting.hbs`)

**Lines**: 93, 128, 152
**Context**: Teaching LLM about event handlers (script/static/llm modes)
**Current State**: STATIC examples using `send_data`

**Problem Examples**:
```json
// Line 93 - Static handler example
{"type": "send_data", "data": "Welcome to the server!\n"}

// Line 128 - SSH banner example
{"type": "send_data", "data": "SSH-2.0-MyServer\n"}

// Line 152 - Generic static response
{"type": "send_data", "data": "Response\n"}
```

**Impact**: HIGH - These examples directly contradict protocol-specific action descriptions
**Solution**: Replace with event-specific examples from EventType

**Proposed Fix**:
```handlebars
{{#if event_examples}}
**Example static handler for this event:**
```json
{
  "event_pattern": "{{current_event_id}}",
  "handler": {
    "type": "static",
    "actions": [{{#with (first event_examples)}}{{{this.1}}}{{/with}}]
  }
}
```
{{else}}
**Example static handler (generic):**
```json
{
  "event_pattern": "*",
  "handler": {
    "type": "static",
    "actions": [{"type": "PROTOCOL_SPECIFIC_ACTION", "param": "value"}]
  }
}
```
{{/if}}
```

---

### 1.3 Response Format Examples (`prompts/shared/partials/response_format.hbs`)

**Lines**: 45-61
**Context**: Teaching LLM the JSON response format
**Current State**: Mix of generic (`show_message`) and semi-generic (`open_server`) examples

**Problem Examples**:
```json
// Line 46 - Generic action
{"actions": [{"type": "show_message", "message": "Hello"}]}

// Line 52 - open_server is good but could be protocol-specific
{"actions": [{"type": "open_server", "port": 8080, "base_stack": "http"}]}
```

**Impact**: MEDIUM - Generic examples less harmful here since they're about format, not protocol
**Solution**: Keep generic for format examples, but add context-specific examples when available

---

### 1.4 User Input Instructions (`prompts/user_input/partials/instructions.hbs`)

**Lines**: 18-20, 24-37, 40-42
**Context**: Teaching LLM how to open servers/clients
**Current State**: Semi-static examples with hardcoded protocols

**Problem Examples**:
```json
// Line 19 - Always uses HTTP
{"type": "open_server", "port": 8080, "base_stack": "http", "instruction": "Echo server that returns Hello World"}

// Lines 24-37 - Scheduled tasks example always uses HTTP
{"type": "open_server", "port": 8080, "base_stack": "http", ...}

// Lines 40-42 - Client example always uses Redis
{"type": "open_client", "host": "localhost", "port": 6379, "base_stack": "redis"}
```

**Impact**: MEDIUM - Not harmful, but missed opportunity for context
**Solution**: After `read_base_stack_docs`, use protocol-specific examples

---

### 1.5 Tool Response - read_server_documentation (`src/llm/actions/tools.rs:1880-1890`)

**Lines**: 1880-1890
**Context**: After LLM reads protocol docs, shows open_server action
**Current State**: STATIC example with placeholder "Handle requests according to protocol specification"

**Problem Example**:
```json
{
  "type": "open_server",
  "port": 8080,
  "base_stack": "tcp",  // ← Uses requested protocol, good!
  "instruction": "Handle requests according to protocol specification"  // ← Generic!
}
```

**Impact**: HIGH - This is exactly when we should show protocol-specific examples!
**Solution**: Use protocol's example_prompt() or extract from EventType examples

**Proposed Fix**:
```rust
// Get typical example from first event type
let example_instruction = if let Some(event_type) = server_protocol.get_event_types().first() {
    if let Some(example) = &event_type.typical_response_example {
        format!("Respond to {} events with {}", event_type.id,
                serde_json::to_string_pretty(example).unwrap())
    } else {
        protocol.example_prompt().to_string()
    }
} else {
    protocol.example_prompt().to_string()
};

result.push_str(&format!("  \"instruction\": \"{}\"\n", example_instruction));
```

---

### 1.6 Tool Response - read_client_documentation (`src/llm/actions/tools.rs:1963-1975`)

**Lines**: 1963-1975
**Context**: After LLM reads client protocol docs
**Current State**: STATIC example with placeholder instruction

**Problem**: Same as 1.5 but for clients
**Impact**: HIGH
**Solution**: Similar to 1.5, use client protocol examples

---

### 1.7 Action Examples (`prompts/shared/partials/actions.hbs:18-23, 48-53`)

**Context**: Displaying available actions to LLM
**Current State**: DYNAMIC - Uses `action.example` field from ActionDefinition
**Impact**: N/A - Already dynamic!
**Note**: This is where we already display examples from ActionDefinition.example

**Current Template**:
```handlebars
{{#if this.example}}
Example:
```json
{{{this.example}}}
```
{{/if}}
```

---

## Part 2: Missing Examples (Add New)

### 2.1 Feedback Processing (`prompts/feedback/partials/instructions.hbs`)

**Context**: LLM analyzing accumulated feedback and adjusting server/client behavior
**Current State**: NO examples
**Opportunity**: Show examples of how to interpret feedback and what actions to take

**Proposed Addition**:
```handlebars
### Example Adjustments

**Pattern**: Multiple timeout errors in feedback
**Action**: Increase timeout or add error handling
```json
{"actions": [{"type": "update_instruction", "instruction": "... with 30s timeout"}]}
```

**Pattern**: Users requesting feature not in current instruction
**Action**: Extend instruction to include new behavior
```json
{"actions": [{"type": "update_instruction", "instruction": "... and also handle /api/v2 endpoints"}]}
```
```

---

### 2.2 Network Request Task (`prompts/network_request/task.hbs`)

**File**: `prompts/network_request/task.hbs`
**Current State**: Very minimal, just role definition
**Opportunity**: Could add context about what makes a good network response

**Current Content**:
```handlebars
# Your Role

You are handling a network event for an active {{protocol_name}} server/client.
```

**Proposed Addition**: Add protocol-specific response guidelines

---

### 2.3 User Input Task (`prompts/user_input/task.hbs`)

**Context**: General instructions for user input processing
**Current State**: NO examples (relies on instructions.hbs)
**Opportunity**: Could add examples of common user request patterns

---

### 2.4 Base Stack Documentation (`generate_base_stack_documentation`)

**Location**: `src/llm/actions/common.rs:1220`
**Context**: List of available protocols
**Current State**: Just names and keywords, no examples
**Opportunity**: Add one-line example per protocol

**Proposed Enhancement**:
```rust
// Current: "http (web, api, rest)"
// Enhanced: "http (web, api, rest) - Example: 'Start HTTP server on 8080 serving static files'"
```

---

### 2.5 Protocol Trait example_prompt()

**Location**: Each protocol's actions.rs
**Current State**: Static string per protocol
**Opportunity**: Make it return different examples based on context

**Current Pattern**:
```rust
fn example_prompt(&self) -> &'static str {
    "Start a TCP server on port 9000 that echoes back all received data"
}
```

**Proposed Enhancement**: Return multiple examples or context-aware examples

---

### 2.6 Conversation History Context

**Context**: When LLM is invoked again after tool use
**Current State**: Shows previous tool results but no examples of "what to do next"
**Opportunity**: After tool completion, suggest next action with example

---

## Part 3: Already Dynamic (Keep As Is)

### 3.1 Easy Request Templates

**Files**: `prompts/easy_request/main.hbs`, `prompts/easy_request/http.hbs`
**Context**: Simplified mode for HTTP servers
**Current State**: DYNAMIC - Has `{{#if examples}}` section that accepts examples
**Status**: ✅ Already implemented correctly

---

## Part 4: Implementation Strategy

### Phase 1: EventType Extensions (Foundation)

**Add to EventType struct**:
```rust
pub struct EventType {
    // ... existing fields ...
    pub typical_response_example: Option<serde_json::Value>,
    pub mandatory_response_example: Option<serde_json::Value>,
    pub optional_response_examples: Vec<serde_json::Value>,
}
```

**Priority Protocols** (have E2E tests):
1. TCP - `send_tcp_data`
2. HTTP - `send_http_response`
3. DNS - `send_dns_a_response`, `send_dns_nxdomain`
4. SSH - `ssh_send_data`
5. SMTP - `send_smtp_response`
6. All others with failing tests

---

### Phase 2: Template Updates (High Impact)

**Priority Order**:
1. ✅ **`scripting.hbs`** - Lines 93, 128, 152 (CRITICAL - directly contradicts)
2. ✅ **`tools.rs:1880, 1963`** - open_server/client examples after docs read
3. ✅ **`network_request/main.hbs`** - Add event examples section
4. **`user_input/partials/instructions.hbs`** - Context-aware open_server examples
5. **`response_format.hbs`** - Add context-specific examples when available

---

### Phase 3: Prompt Builder Changes

**Update `src/llm/prompt.rs`**:
1. Add event examples extraction from EventType
2. Pass examples to template data
3. Track "last_read_protocol" in conversation history for context

**Update `src/llm/action_helper.rs`**:
1. Pass Event to prompt builder (already available)
2. Extract examples at prompt build time

---

### Phase 4: Protocol-Specific Examples

**For each protocol** (example: TCP):
```rust
// src/server/tcp/actions.rs

pub static TCP_DATA_RECEIVED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("tcp_data_received", "TCP data received")
        .with_parameters(vec![...])
        .with_actions(get_tcp_sync_actions())
        .with_typical_example(json!({
            "type": "send_tcp_data",
            "data": "48656c6c6f"  // Echo in hex
        }))
        .with_optional_example(json!({
            "type": "disconnect"
        }))
});
```

---

## Part 5: Expected Improvements

### Test Pass Rate Projections

**Current**: 52.9% (9/17 tests passing)

**After EventType examples** (Phase 1-2):
- Fix: `test_tcp_hex_response` (wrong action type)
- Fix: `test_dns_query_response` (wrong action type)
- Fix: `test_http_request_with_instruction` (wrong action type)
- Fix: `test_tcp_echo_script` (wrong action in examples)
- **Projected**: 70-80% (12-14/17 tests)

**After protocol-specific tool examples** (Phase 3):
- Fix: `test_http_script_sum_query_params` (better open_server examples)
- Fix: `test_dns_server_with_static_response` (correct static examples)
- **Projected**: 80-90% (14-15/17 tests)

---

## Part 6: Priority Matrix

| Location | Impact | Effort | Priority | Phase |
|----------|--------|--------|----------|-------|
| scripting.hbs static examples | HIGH | LOW | 🔴 CRITICAL | 1 |
| EventType.typical_example | HIGH | MEDIUM | 🔴 CRITICAL | 1 |
| read_server_docs open_server example | HIGH | LOW | 🔴 CRITICAL | 2 |
| network_request event examples | HIGH | MEDIUM | 🟡 HIGH | 2 |
| user_input after docs read | MEDIUM | MEDIUM | 🟡 HIGH | 3 |
| response_format context examples | LOW | LOW | 🟢 MEDIUM | 4 |
| feedback examples | LOW | MEDIUM | 🟢 LOW | 4 |
| base_stack_docs enhancements | LOW | LOW | 🟢 LOW | 5 |

---

## Part 7: Breaking Down Implementation

### Minimum Viable Fix (1-2 hours)

**Scope**: Fix the most critical static examples

**Files**:
1. `src/protocol/event_type.rs` - Add example fields
2. `src/server/tcp/actions.rs` - Add TCP examples
3. `src/server/http/actions.rs` - Add HTTP examples
4. `src/server/dns/actions.rs` - Add DNS examples
5. `prompts/shared/partials/scripting.hbs` - Use dynamic examples

**Expected Improvement**: 60-70% pass rate

---

### Full Implementation (4-6 hours)

**Scope**: All high-priority items

**Additional Files**:
6. `src/llm/prompt.rs` - Extract and pass examples
7. `prompts/network_request/main.hbs` - Event examples section
8. `src/llm/actions/tools.rs` - Protocol-specific open_server/client examples
9. All protocols with E2E tests - Add examples

**Expected Improvement**: 80-90% pass rate

---

## Part 8: Conversation Flow Analysis

### Scenario: User Opens HTTP Server After Reading Docs

**Current Flow**:
1. User: "How do I start an HTTP server?"
2. LLM: `{"actions": [{"type": "read_base_stack_docs", "protocol": "http"}]}`
3. Tool Result: Returns HTTP docs + STATIC open_server example
4. User (next turn): "Start it on port 8080"
5. LLM: Uses the STATIC example from docs (missed opportunity!)

**Proposed Flow with Dynamic Examples**:
1. User: "How do I start an HTTP server?"
2. LLM: `{"actions": [{"type": "read_server_documentation", "protocol": "http"}]}`
3. Tool Result: Returns HTTP docs + **HTTP-SPECIFIC** open_server example:
   ```json
   {
     "type": "open_server",
     "port": 8080,
     "base_stack": "http",
     "instruction": "Respond to GET /api/users with JSON array of users",
     "event_handlers": [{
       "event_pattern": "http_request",
       "handler": {"type": "llm"}
     }]
   }
   ```
4. User: "Start it on port 8080"
5. LLM: Uses the HTTP-SPECIFIC pattern from docs ✅

---

## Part 9: Data Flow Diagram

```
EventType Definition (src/server/*/actions.rs)
    ↓
    ├─ typical_response_example: json!({...})
    ├─ mandatory_response_example: json!({...})
    └─ optional_response_examples: vec![...]
    ↓
Event Instance (event received)
    ↓
prompt_builder::build_network_event_action_prompt_for_server()
    ↓
    ├─ Extract examples from event.event_type
    ├─ Format for template
    └─ Pass to TemplateDataBuilder
    ↓
Template (network_request/main.hbs)
    ↓
    ├─ {{#if event_examples}}
    ├─ Display typical example
    ├─ Display optional examples
    └─ {{/if}}
    ↓
LLM sees context-specific examples
    ↓
Returns correct action type! ✅
```

---

## Part 10: Example Comparison

### Before (Static)

**Network event**: TCP data received
**Prompt includes**:
```json
// From scripting.hbs line 93
{"type": "send_data", "data": "Welcome to the server!\n"}
```

**LLM response**:
```json
{"actions": [{"type": "send_data", "data": "48656c6c6f"}]}  // ❌ WRONG!
```

---

### After (Dynamic)

**Network event**: TCP data received
**Event includes**: `tcp_data_received_event.typical_response_example`
**Prompt includes**:
```json
// From TCP_DATA_RECEIVED_EVENT
{"type": "send_tcp_data", "data": "48656c6c6f"}
```

**LLM response**:
```json
{"actions": [{"type": "send_tcp_data", "data": "48656c6c6f"}]}  // ✅ CORRECT!
```

---

## Conclusion

This analysis identified **13 locations** where examples are used or could be added. The most critical fixes are:

1. **EventType examples** - Foundation for all dynamic examples
2. **scripting.hbs** - Remove static `send_data` examples
3. **read_server_documentation** - Protocol-specific open_server examples
4. **network_request prompt** - Event-specific response examples

**Estimated Impact**: 25-35% improvement in test pass rate with full implementation.

**Recommended Approach**: Start with Minimum Viable Fix (critical items only), measure impact, then proceed with full implementation if results are positive.
