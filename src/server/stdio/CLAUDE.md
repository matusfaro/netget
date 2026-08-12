# Standard I/O (stdin/stdout/stderr) Protocol Implementation

Impersonates a program in a pipe. NetGet itself becomes the child process behind a pipe
(`someprogram | netget ... | otherprogram`): it reads chunks from its own stdin
(`stdio_input_received`), and the model decides what to emit on stdout (`write_stdout`) and stderr
(`write_stderr`). NetGet owns the plumbing; the model owns the payload — over the process's own
standard streams.

**State**: Experimental. **Platform**: Unix only; the whole module is `#![cfg(unix)]`.
**Privilege**: none.

## Coexistence — the subtlety, and how it is resolved

This protocol takes over the process's real stdin/stdout, which collides with two things. Both are
handled:

1. **Interactive TUI / a terminal** — ratatui owns the terminal. `spawn()` returns `Err` (→
   `ServerStatus::Error`) when `stdin` is a TTY. Refuse, don't corrupt the UI.
2. **`--mcp` stdio** — MCP JSON-RPC owns stdin/stdout. `spawn()` returns `Err` when `--mcp` /
   `--mcp-stdio` is in the process argv (`--mcp-http` is a different flag and is allowed).

Only one stdio server may run per process (an `AtomicBool` claim, released on task end/abort).

### The startup constraint (important, non-obvious)

**NetGet must be launched via actions-JSON / `--load`, not a natural-language prompt.** NetGet's
prompt resolution (`Args::get_actions_json` → `piped_stdin`, `src/cli/args.rs`) does a **blocking
`read_to_string` on stdin** whenever the invocation is *not* actions-JSON — it exists to support
`cat prompt.txt | netget`. With an open, never-EOF stdin pipe that read blocks forever, *before any
server starts*, so a natural-language prompt (whether an arg or piped in) can never hand a live
stdin to the stdio server. Passing a `{"actions": [ {"type":"open_server","base_stack":"stdio"} ]}`
argument, or `--load file.netget`, returns before `piped_stdin()` is ever called and leaves stdin
intact. **This is the sanctioned launch path.** It is a NetGet-bootstrap fact, not something this
protocol can change without editing core arg handling.

### The stdout-sharing caveat (known limitation)

In the non-interactive runner the status stream is also `println!`'d to **stdout**
(`src/cli/non_interactive.rs`), so the stdout byte stream is *not* pristine — the model's
`write_stdout` bytes are interleaved with NetGet status lines. For a genuinely clean downstream
pipe this would need the status stream routed off stdout (e.g. to stderr); that is a core-runner
change and a documented follow-up, not done here. The model's output is present and correct on
stdout; it just shares the channel.

## What the model sees and controls

**Events** — all attach `write_stdout` / `write_stderr` (and `close_stdio` where sensible), and all
are actually emitted:

- `stdio_started` — no fields. Emitted once at startup only under `send_first` (same conditional
  pattern as `socket_file_connection_opened`).
- `stdio_input_received` — `data` + `encoding`. Emitted for each chunk read from stdin.
- `stdio_input_closed` — no fields. Emitted when stdin reaches EOF (upstream closed the pipe).

**Actions**: `write_stdout` (`data`, `encoding`) → written to fd 1; `write_stderr` (`data`,
`encoding`) → written to fd 2, carried internally as a `Custom{"stdio_stderr"}` result;
`close_stdio` → ends the session. Explicit `utf8`/`hex` encoding, no auto-detection, invalid hex is
an action error not a panic — the `send_tcp_data` lesson.

## Failure and lifecycle

- **`spawn()` refuses cleanly** (returns `Err`, no half-started state) under a TTY, `--mcp`, or a
  second stdio server.
- **Fail closed** on LLM error: emits nothing, logs ERROR on both channels.
- Processing is serial (a pipe is a serial stream). The read loop is registered via
  `register_server_task`; a `StdioClaimGuard` releases the process claim on stop/abort.

## Verified

`tests/server/stdio/e2e_test.rs`: the NetGet binary is spawned as a real child with piped
stdin/stdout, started via actions-JSON; a line typed on its stdin is answered by the mocked model
with an uppercased line on stdout, asserted on the child's real stdout. `verify_calls()` confirms
the one event round-trip.
