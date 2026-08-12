# stdio (pipe-filter) E2E Tests

## Strategy

stdio owns the process's own stdin/stdout, so the shared harness (prompt-as-arg, stdout read as
logs, stdin never piped) cannot drive it. This test therefore **spawns the NetGet binary directly
as a real child** (`env!("CARGO_BIN_EXE_netget")`) with piped stdin/stdout — the actual
`prog | netget | prog` use case — points it at an in-process `MockOllamaServer`, feeds a line on
stdin, and asserts the model's bytes on the child's real stdout. LLM interaction is still asserted
via `mock_server.verify_calls()`.

## What the test asserts

`test_stdio_pipe_filter`:

1. Launches NetGet via **actions-JSON** (`{"actions":[{"type":"open_server","base_stack":"stdio"}]}`
   as the trailing arg) so NetGet does not drain/block on stdin during prompt resolution — see the
   startup constraint below.
2. Writes `hello\n` to the child's stdin.
3. The mock matches the `stdio_input_received` event (`data` contains `hello`) and answers
   `write_stdout` `HELLO\n`.
4. Reads the child's stdout and asserts it contains `HELLO`.
5. `verify_calls()` confirms the one event call.

## The startup constraint (why actions-JSON)

`Args::get_actions_json` → `piped_stdin` (`src/cli/args.rs`) does a **blocking** `read_to_string`
on stdin for any non-actions-JSON invocation (to support `cat prompt.txt | netget`). With an open,
never-EOF stdin pipe that blocks forever before the server starts. Launching via actions-JSON (or
`--load`) returns before `piped_stdin()` is called, leaving stdin for the stdio server. A
natural-language prompt therefore cannot be used to start a stdio server that reads a live stdin.

## LLM call budget

1 test x 1 mocked LLM call (the one stdin line; startup is deterministic actions-JSON) = **1 call**.

## Running

```bash
./cargo-isolated.sh test --no-default-features --features stdio --test server stdio -- --test-threads=100
```

## Notes / known limitations

- stderr is sent to `Stdio::null()` in the test to keep NetGet's own diagnostics out of the way;
  stdout carries both NetGet status lines and the model's `write_stdout` bytes, so the assertion
  uses `contains`.
- Not covered: `stdio_started` (send_first), `stdio_input_closed` (EOF), `write_stderr`,
  `close_stdio`, and the refusal paths (TTY / `--mcp`), which are enforced in `spawn()`.
