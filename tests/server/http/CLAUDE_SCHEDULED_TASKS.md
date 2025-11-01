# HTTP Scheduled Tasks E2E Tests

## Test Overview
Tests the scheduled task execution feature using an HTTP server as the protocol context. Validates that the LLM can create, manage, and execute both one-shot and recurring scheduled tasks, with tasks modifying server state and responding to HTTP requests based on task execution.

## Test Strategy
- **HTTP server as test vehicle**: Uses HTTP protocol because it provides easy validation via GET requests
- **State modification via tasks**: Tasks modify internal server state (counters, flags)
- **HTTP endpoints for validation**: Each test exposes endpoints that reflect task execution state
- **Time-based validation**: Tests wait for tasks to execute and verify state changes
- **Both task creation methods**: Tests both standalone `schedule_task` action and `scheduled_tasks` parameter in `open_server`

## LLM Call Budget
- `test_http_with_recurring_task()`: 1 startup + ~3 task executions = ~4 LLM calls
- `test_http_with_oneshot_task()`: 1 startup + 2 GET requests + 1 task execution = 4 LLM calls
- `test_http_with_server_attached_tasks()`: 1 startup + 3 GET requests + ~3 recurring task executions + 1 one-shot task = ~8 LLM calls
- **Total: ~16 LLM calls** (higher due to task execution overhead)

**Note**: This test suite exceeds the typical 10-call guideline because:
1. Each scheduled task execution requires an LLM call (or script execution)
2. Testing both one-shot and recurring tasks requires waiting for multiple executions
3. Validating task state changes requires additional HTTP GET requests
4. This is acceptable for testing a new feature (scheduled tasks)

**Optimization Opportunity**: Enable scripting mode for tasks to reduce LLM calls. With scripting, task executions would not call LLM, reducing calls to ~7 total (3 startups + 4 GET requests for validation).

## Scripting Usage
❌ **Scripting Disabled** - Action-based task execution

**Rationale**: Tests validate that LLM can correctly execute scheduled tasks using action-based responses. This ensures the full task execution pipeline works (prompt generation, LLM invocation, action execution).

**Future Consideration**: Add separate test with scripting enabled to validate script-based task execution (should be much faster).

## Client Library
- **reqwest v0.11** - Async HTTP client
- **Usage**: GET requests to validate server state after task execution
- **Why HTTP?**: Provides simple, stateless way to query server state without complex protocol handling

## Expected Runtime
- Model: qwen3-coder:30b
- Runtime: ~60-90 seconds for full test suite (3 tests with multiple task executions and waits)
- Each test includes:
  - Server startup: 2-3 seconds
  - Task creation LLM call: 5-8 seconds
  - Waiting for task execution: 3-7 seconds per test
  - Task execution LLM calls: 5-8 seconds each
  - HTTP validation requests: 5-8 seconds each
- Longest test: `test_http_with_server_attached_tasks()` (~30-40s due to multiple tasks and validations)

**Note**: Tests are intentionally slow due to:
1. Time-based task scheduling (must wait for delays/intervals)
2. Multiple LLM calls per test (task executions)
3. Multiple validation GET requests

## Failure Rate
- **Medium** (~10-15%) - Task timing and state management can be complex
- Common failure modes:
  - LLM doesn't create scheduled task correctly (wrong parameters)
  - Task doesn't execute due to timing issues
  - LLM doesn't maintain internal state correctly (counter, flag)
  - HTTP response doesn't reflect task execution state
  - LLM confuses one-shot vs. recurring task semantics
- Timeout failures: Rare (<5%) - tasks should execute within expected time windows

**Known Issues**:
1. Task timing is imprecise (1-second execution loop granularity)
2. Tests use lenient assertions to accommodate LLM response variability
3. State management across multiple LLM calls can be fragile

## Test Cases

### 1. Recurring Scheduled Task (`test_http_with_recurring_task`)
- **Prompt**: HTTP server with recurring task that increments counter every 2 seconds
- **Task Definition**:
  - `task_id`: "heartbeat_counter"
  - `recurring`: true
  - `interval_secs`: 2
  - `instruction`: "Increment the internal heartbeat counter by 1"
- **Validation Flow**:
  1. Server starts (counter = 0)
  2. Recurring task created via `schedule_task` action
  3. Wait 7 seconds (~3 task executions at 0s, 2s, 4s, 6s)
  4. GET /heartbeat → Verify counter > 0
- **Expected**: Counter incremented at least once (lenient to handle timing variance)
- **Purpose**: Tests recurring task creation, execution, and state modification

### 2. One-Shot Scheduled Task (`test_http_with_oneshot_task`)
- **Prompt**: HTTP server with one-shot task that sets flag after 3-second delay
- **Task Definition**:
  - `task_id`: "set_ready_flag"
  - `recurring`: false
  - `delay_secs`: 3
  - `instruction`: "Set the internal ready flag to true"
- **Validation Flow**:
  1. Server starts (ready flag = false)
  2. One-shot task created via `schedule_task` action
  3. GET /status → Verify status is "initializing" (before task)
  4. Wait 5 seconds (task executes at ~3s)
  5. GET /status → Verify status is "ready" (after task)
- **Expected**: Status changes from "initializing" to "ready" after task executes
- **Purpose**: Tests one-shot task creation, delayed execution, and flag setting

### 3. Server-Attached Scheduled Tasks (`test_http_with_server_attached_tasks`)
- **Prompt**: HTTP server with tasks defined in `scheduled_tasks` parameter of `open_server` action
- **Task Definitions**:
  1. Recurring task "update_metrics" (interval: 2s, instruction: increment metrics counter)
  2. One-shot task "delayed_init" (delay: 3s, instruction: set initialized flag to true)
- **Validation Flow**:
  1. Server starts with `scheduled_tasks` parameter defining both tasks
  2. GET /initialized → Verify "no" (before one-shot task)
  3. Wait 5 seconds (tasks execute)
  4. GET /metrics → Verify counter > 0 (recurring task executed)
  5. GET /initialized → Verify "yes" (one-shot task executed)
- **Expected**: Both tasks execute correctly, metrics incremented, flag set
- **Purpose**: Tests server-attached task creation (via `open_server` parameter) and concurrent execution of multiple tasks

## Known Issues

### 1. Task Timing Imprecision
The task execution loop runs every 1 second, so tasks may execute up to 1 second late:
- Task with `delay_secs: 3` might execute at 3.0-4.0 seconds
- Task with `interval_secs: 2` might execute at 2.0-3.0 second intervals

**Mitigation**: Tests use generous wait times and lenient assertions.

### 2. LLM State Management
The LLM must maintain internal state (counters, flags) across multiple invocations. This can be fragile:
- Counter might not increment correctly
- Flag might not persist between requests
- LLM might reset state unexpectedly

**Mitigation**: Tests check for any increment (not exact values). Instructions explicitly mention "internal" state.

### 3. Response Variability
The LLM may format responses differently than expected:
- "ready" might be "Ready", "READY", "is ready", etc.
- Counter might be "3", "three", "count: 3", etc.

**Mitigation**: Tests use `contains()` checks and case-insensitive matching.

### 4. Task Creation Confirmation
Tests don't verify that tasks were actually created (no `/list_tasks` endpoint). They only verify execution effects.

**Future Improvement**: Add validation that tasks appear in server state before waiting for execution.

### 5. No Task Cancellation Tests
Tests don't validate `cancel_task` action or max_executions parameter.

**Future Enhancement**: Add tests for:
- Canceling recurring tasks mid-execution
- Verifying task stops after `max_executions` reached
- Listing active tasks

## Performance Notes

### Why HTTP for Task Testing?
HTTP provides several advantages for testing scheduled tasks:
1. **Stateless validation**: GET requests don't require protocol state management
2. **Easy state inspection**: Server exposes internal state via endpoints
3. **Simple client**: reqwest makes validation straightforward
4. **Clear semantics**: Request-response pattern matches validation flow
5. **Familiar protocol**: Easier to debug than custom protocols

**Alternatives Considered**:
- **TCP**: Too low-level, would need custom protocol for state queries
- **DNS**: Stateless, hard to maintain counters/flags
- **SSH**: Too complex, requires authentication and shell semantics
- **SMTP**: Not designed for state queries

### Task Execution Overhead
Each task execution in action-based mode requires:
1. Build task execution prompt (includes instruction, context, available actions)
2. Call LLM (5-8 seconds)
3. Parse LLM response (extract actions)
4. Execute actions (state modifications)

With scripting enabled, steps 2-3 would be replaced by script execution (~1-10ms).

**Performance Impact**: Scripting would make tests 500-1000x faster:
- Action-based: ~60-90 seconds for 3 tests
- Script-based: ~5-10 seconds for 3 tests (estimated)

### Test Consolidation
All three tests could be consolidated into one comprehensive test:

```rust
async fn test_http_scheduled_tasks_comprehensive() {
    // Server with:
    // - Recurring task (heartbeat counter)
    // - One-shot task (ready flag)
    // - Server-attached tasks (metrics + initialized)

    // Validate all task types in single test
    // Reduces startup overhead from 3 → 1 server
    // Total LLM calls: ~12-15 (vs. current 16)
}
```

**Trade-off**: Consolidation saves ~4 LLM calls but makes failure diagnosis harder. Current isolation is better for initial feature testing.

## Future Enhancements

### Test Coverage Gaps
1. **Task Cancellation**: No tests for `cancel_task` action
2. **Max Executions**: No tests for `max_executions` parameter
3. **Task Listing**: No tests for viewing active tasks
4. **Error Handling**: No tests for task execution failures
5. **Retry Logic**: No tests for exponential backoff retry
6. **Task Context**: No tests for passing `context` parameter to tasks
7. **Server-Scoped vs Global**: No tests distinguishing task scopes
8. **Concurrent Tasks**: No tests for multiple tasks executing simultaneously
9. **Task Cleanup**: No tests verifying tasks are removed when server closes
10. **Script-Based Tasks**: No tests for `script_inline`/`script_path` parameters

### Potential New Tests
1. **Task cancellation**: Create recurring task, let it execute twice, cancel it, verify it stops
2. **Max executions**: Create recurring task with `max_executions: 3`, verify it stops after 3 runs
3. **Error retry**: Create task that fails initially, verify exponential backoff and error passing
4. **Global vs Server tasks**: Create global task, verify it has access to all actions
5. **Scripted tasks**: Enable scripting, create tasks with `script_inline`, verify fast execution
6. **Task context**: Pass `context` with thresholds, verify task uses context data
7. **Multiple servers**: Create tasks on multiple servers, verify isolation

### Scripting Mode Test
Add test variant with scripting enabled:

```rust
#[tokio::test]
async fn test_http_with_scripted_recurring_task() {
    // Same as test_http_with_recurring_task but with:
    // - script_inline parameter
    // - Verify task execution is fast (<100ms per execution)
    // - Verify LLM is not called for task executions
}
```

**Expected Runtime**: ~10-15 seconds (vs. 20-30 seconds for action-based)

### Integration with Other Features
Scheduled tasks could be tested with:
1. **Memory**: Tasks that create/update memory nodes
2. **File I/O**: Tasks that write logs to files
3. **Web Search**: Tasks that fetch data from web periodically
4. **Multi-Protocol**: Tasks on HTTP server that send UDP packets

## Comparison with Other Test Suites

| Test Suite | Tests | LLM Calls | Runtime | Complexity |
|------------|-------|-----------|---------|------------|
| HTTP Basic | 7 | 14 | 2-3m | Low (request-response) |
| DNS | 4 | 5 | 40-50s | Low (scripting enabled) |
| SSH | 3 | 8-10 | 2m | Medium (authentication) |
| **HTTP Scheduled Tasks** | **3** | **16** | **60-90s** | **High (timing + state)** |

Scheduled tasks tests have:
- **Higher LLM call count**: Task executions require LLM invocations
- **Longer runtime**: Must wait for time-based delays
- **Higher complexity**: State management across multiple LLM calls
- **Higher failure rate**: Timing and state management are fragile

This is acceptable for testing a new, complex feature like scheduled tasks.

## Debugging Tips

### If Tasks Don't Execute
1. Check server logs for task creation confirmation
2. Verify task parameters (recurring, interval_secs, delay_secs)
3. Ensure task execution loop is running (1-second ticker)
4. Check for task execution errors in logs

### If State Doesn't Update
1. Verify LLM receives task execution prompt
2. Check LLM response contains correct actions
3. Ensure actions are executed (check action executor logs)
4. Verify state is maintained between LLM calls (might need explicit "remember" instructions)

### If Tests Are Flaky
1. Increase wait times (current: 5-7 seconds, try 10 seconds)
2. Use more lenient assertions (check for any increment, not specific values)
3. Add explicit state persistence instructions in prompt
4. Consider using scripting mode for deterministic execution

## References
- Scheduled tasks implementation: `src/state/task.rs`
- Task execution loop: `src/cli/rolling_tui.rs` (execute_due_tasks)
- Task action definitions: `src/llm/actions/common.rs` (ScheduleTask, ServerTaskDefinition)
- Task prompt generation: `src/llm/prompt.rs` (build_task_execution_prompt)
- HTTP server tests: `tests/server/http/test.rs` (baseline HTTP testing)

