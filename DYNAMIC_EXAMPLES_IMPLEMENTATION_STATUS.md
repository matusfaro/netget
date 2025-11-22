# Dynamic Examples Implementation - Status Report

**Date**: 2025-11-21
**Status**: IN PROGRESS (Phase 3 of 4 underway)

---

## Implementation Progress

### ✅ Phase 1: EventType Extensions (COMPLETE)

**Files Modified**:
- `src/protocol/event_type.rs`

**Changes**:
1. Added three new fields to `EventType` struct:
   - `typical_response_example: Option<JsonValue>`
   - `mandatory_response_example: Option<JsonValue>`
   - `optional_response_examples: Vec<JsonValue>`

2. Updated `EventType::new()` constructor to initialize new fields

3. Added three builder methods:
   - `with_typical_example(example: JsonValue) -> Self`
   - `with_mandatory_example(example: JsonValue) -> Self`
   - `with_optional_example(example: JsonValue) -> Self`

**Status**: ✅ COMPLETE - Foundation in place

---

### 🔄 Phase 3: Protocol-Specific Examples (IN PROGRESS)

#### ✅ TCP Protocol (COMPLETE)
**File**: `src/server/tcp/actions.rs`

**Changes**:
1. `TCP_CONNECTION_OPENED_EVENT`:
   - Added typical_example: `send_tcp_data` with banner

2. `TCP_DATA_RECEIVED_EVENT`:
   - Added typical_example: `send_tcp_data` with hex echo
   - Added optional_example: `wait_for_more`
   - Added optional_example: `close_connection`

**Impact**: Should fix `test_tcp_hex_response` failure

---

#### ✅ HTTP Protocol (COMPLETE)
**File**: `src/server/http/actions.rs`

**Changes**:
1. `HTTP_REQUEST_EVENT`:
   - Added typical_example: 200 OK with HTML
   - Added optional_example: 404 Not Found
   - Added optional_example: 201 Created with JSON

**Impact**: Should fix `test_http_*` failures

---

#### ⏳ DNS Protocol (NEXT)
**File**: `src/server/dns/actions.rs`

**TODO**:
1. Add examples to `DNS_QUERY_EVENT`:
   - typical_example: `send_dns_a_response`
   - optional_example: `send_dns_nxdomain`
   - optional_example: `send_dns_aaaa_response`

**Impact**: Should fix `test_dns_query_response` failure

---

### ⏳ Phase 2: Template Updates (PENDING)

#### Critical Files to Update:

1. **`prompts/shared/partials/scripting.hbs` (HIGH PRIORITY)**
   - Lines 93, 128, 152 have static `send_data` examples
   - **THIS IS THE SMOKING GUN** - directly contradicts protocol actions
   - TODO: Replace with dynamic examples from EventType

2. **`src/llm/actions/tools.rs` (HIGH PRIORITY)**
   - Lines 1880-1890: `execute_read_server_documentation`
   - Lines 1963-1975: `execute_read_client_documentation`
   - TODO: Use protocol-specific examples instead of generic

3. **`prompts/network_request/main.hbs` (MEDIUM PRIORITY)**
   - TODO: Add event examples section

---

### ⏳ Phase 4: Prompt Builder Changes (PENDING)

**Files to Update**:

1. **`src/llm/prompt.rs`**
   - TODO: Extract examples from EventType
   - TODO: Pass examples to template data
   - TODO: Update `build_network_event_action_prompt_for_server()` signature

2. **`src/llm/action_helper.rs`**
   - TODO: Pass Event to prompt builder (if needed)
   - NOTE: Event may already be available

---

## Next Steps (Priority Order)

1. **Add DNS examples** (5 min)
2. **Update scripting.hbs** (10 min) - HIGH IMPACT
3. **Update prompt builder** (15 min) - Connect EventType → Templates
4. **Update read_server_documentation** (10 min) - Protocol-specific examples
5. **Add event examples to network_request template** (10 min)
6. **Compile and test** (10 min)

**Total Remaining**: ~60 minutes

---

## Files Modified So Far

1. `src/protocol/event_type.rs` ✅
2. `src/server/tcp/actions.rs` ✅
3. `src/server/http/actions.rs` ✅
4. `src/server/dns/actions.rs` ⏳ (in progress)
5. `prompts/shared/partials/scripting.hbs` ⏳ (pending)
6. `src/llm/actions/tools.rs` ⏳ (pending)
7. `prompts/network_request/main.hbs` ⏳ (pending)
8. `src/llm/prompt.rs` ⏳ (pending)

---

## Expected Test Improvements

### Current State
- **Pass Rate**: 52.9% (9/17 tests)
- **Failing Tests**: 8

### After Protocol Examples (Phase 1 + 3)
- **Expected**: 60-70% (10-12/17 tests)
- **Fixes**:
  - `test_tcp_hex_response` - TCP examples
  - `test_http_request_with_instruction` - HTTP examples
  - Maybe `test_dns_query_response` - DNS examples

### After Template Updates (Phase 2)
- **Expected**: 70-80% (12-14/17 tests)
- **Fixes**:
  - Tests failing due to static `send_data` in scripting.hbs
  - Tests with static/script handlers showing wrong examples

### After Full Implementation (Phase 4)
- **Expected**: 80-90% (14-15/17 tests)
- **Fixes**:
  - All action-type-related failures
  - Context-aware examples throughout prompts

---

## Compilation Status

**Last Check**: Not yet tested
**TODO**: Run `cargo check` after completing Phase 3

---

## Rollback Plan

If implementation causes issues:
1. Revert `src/protocol/event_type.rs` changes
2. Revert protocol action.rs changes
3. Revert template changes

All changes are backward compatible - new fields are `Option<>` or `Vec<>` so existing code continues to work.

---

## Testing Strategy

1. **Unit Compilation**: `cargo check --no-default-features --features tcp,http,dns`
2. **Quick Test**: Run `test_tcp_hex_response` to see if it passes
3. **Full Test**: Run all 17 model tests
4. **Compare**: Check pass rate improvement

---

## Documentation Updates Needed

After completion:
1. Update `DYNAMIC_EXAMPLES_ANALYSIS.md` with results
2. Create migration guide for other protocols
3. Document EventType example patterns in CLAUDE.md

---

## Notes

- EventType examples use `serde_json::json!()` macro
- Examples are protocol-specific action types, NOT generic actions
- Templates will conditionally show examples when available
- Backward compatible - old EventTypes without examples still work
