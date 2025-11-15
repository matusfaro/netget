# Instance 5: Git Test Infrastructure Fix

## Session Goal
Continue fixing protocol test failures from Instance 4, focusing on Git and gRPC tests.

## Key Discovery: Mock Extraction Bug

### The Problem
Git and gRPC tests were failing with "No servers or clients started in netget" or Ollama timeout errors. The root cause was **mock extraction logic in `tests/helpers/mock_ollama.rs` extracting incorrect instructions from prompts**.

### Example of the Bug
```
Prompt contained:
- System instructions: "Start a new server..."
- User command (at end): "listen on port 61461 via git."

Mock extraction was finding: "Start a new server" (from system prompt)
Should have found: "listen on port 61461 via git." (user command)
```

### The Fix
Updated `extract_context_from_prompt()` in `tests/helpers/mock_ollama.rs` to:
1. Look for user input at the END of prompts
2. Search AFTER "## System Capabilities" section
3. Search AFTER "DataLink protocol unavailable" marker
4. Support "Trigger: User input:" and "User input:" markers

## Files Modified

### `tests/helpers/mock_ollama.rs` (Lines 536-624)
**Impact**: CRITICAL - Core mock extraction logic
**Changes**: Completely rewrote instruction extraction to find user input at end of prompts

Key code:
```rust
// Look for user input AFTER system capabilities section
let has_user_message = if let Some(cap_idx) = prompt.find("## System Capabilities").or_else(|| prompt.find("# Current State")) {
    let after_cap = &prompt[cap_idx..];
    if let Some(end_idx) = after_cap.find("DataLink protocol unavailable") {
        let after_system = &after_cap[end_idx + "DataLink protocol unavailable".len()..];
        // Look for the first substantial non-empty line after the system section
        let lines: Vec<&str> = after_system.lines().collect();
        let mut found_instruction = false;
        for line in lines.iter() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && trimmed.len() > 5 {
                // Check if this is a "Trigger: User input:" or "User input:" line
                if let Some(after_marker) = trimmed.strip_prefix("Trigger: User input:") {
                    let instruction = after_marker.trim().trim_matches('"').trim_matches('\'');
                    context.instruction = instruction.to_string();
                    found_instruction = true;
                    break;
                } else {
                    context.instruction = trimmed.to_string();
                    found_instruction = true;
                    break;
                }
            }
        }
        found_instruction
    } else {
        false
    }
} else {
    // Fallback to [user] message pattern
    // ...
};
```

### `tests/server/git/e2e_test.rs` (Multiple Functions)
**Impact**: HIGH - Protocol event mocks for all 5 Git tests
**Changes**: Added second/third mock rules for Git protocol events (info/refs, upload-pack)

Pattern used:
```rust
let config = NetGetConfig::new(prompt)
    .with_mock(|mock| {
        mock
            // Mock 1: Server startup
            .on_instruction_containing("listen on port")
            .and_instruction_containing("git")
            .respond_with_actions(...)
            .expect_calls(1)
            .and()
            // Mock 2: Git protocol event (NEW)
            .on_instruction_containing("Git client is requesting references")
            .and_instruction_containing("test-repo")
            .respond_with_actions(...)
            .expect_calls(1)
            .and()
    });
```

### `src/llm/ollama_client.rs` (Line 554)
**Impact**: MEDIUM - Mock response handling
**Changes**: Fixed missing argument in `to_response_string()` call

```rust
// Before (compilation error):
let response_str = response.to_response_string();

// After (fixed):
let response_str = response.to_response_string(Some(&context.event_data));
```

### `tests/helpers/mock_ollama.rs` (Lines 337-372)
**Impact**: LOW - Debugging
**Changes**: Added debug logging to track mock matching

```rust
// DEBUG: Log extracted context
eprintln!("🔍🔍🔍 Mock generate context extracted:");
eprintln!("  event_type: {:?}", context.event_type);
eprintln!("  instruction: {}", &context.instruction[..context.instruction.len().min(200)]);
```

## Test Results

### Before Fix
- Git: 0/5 passing (all failing with "No servers started" or Ollama timeout)
- gRPC: 0/5 passing (same issues)

### After Fix
- Git: **4/5 passing** ✅
  - ✅ `test_git_info_refs_endpoint`
  - ✅ `test_git_repository_not_found`
  - ✅ `test_git_multiple_repositories`
  - ✅ `test_git_with_scripting`
  - ❌ `test_git_clone_with_system_git` (still timing out - different issue)

## Architecture Clarification

### Deprecated Approach (Attempted Incorrectly)
- ❌ Writing mock config to file
- ❌ Passing `--mock-config-file` CLI argument

### Correct Approach (Already in Place)
- ✅ Mock Ollama HTTP server (`tests/helpers/mock_ollama.rs`)
- ✅ Pass `--ollama-url` to spawned NetGet process
- ✅ Mock extraction logic extracts context from prompts
- ✅ Mock rules match against extracted context

## Remaining Issues

### `test_git_clone_with_system_git` Timeout
**Status**: Times out after 120 seconds
**Evidence**: Mock verification shows Rules #1 and #2 (protocol events) were never called - only Rule #0 (server startup) was called
**Hypothesis**: The `git clone` command makes HTTP requests that don't match the mock extraction logic
**Next Steps**:
1. Investigate what instruction the Git server is sending to mock Ollama
2. Check if Git protocol event prompts are formatted differently
3. May need to adjust mock extraction for Git-specific event formats

## Impact on Other Protocols

This fix should benefit ALL protocols that use mocks, as the core issue was in the shared mock extraction logic used by all tests.

Protocols likely to benefit:
- gRPC (0/5 → should improve)
- HTTP (if using mocks)
- DNS (if using mocks)
- Any protocol with complex multi-step interactions

## Key Learnings

1. **Mock extraction must look at END of prompts** - System prompts contain example text that can confuse extraction
2. **Two-stage LLM calls are common** - Server startup + protocol events each trigger separate LLM calls
3. **Mock Ollama HTTP server is the correct approach** - Not file-based config
4. **Debug logging is essential** - Without the debug output, would have been much harder to identify the extraction bug

## Technical Debt

None introduced - this session was purely bug fixes to existing infrastructure.
