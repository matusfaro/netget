# Named Pipe (POSIX FIFO) Protocol Implementation

Impersonates an OS-level IPC endpoint: a `mkfifo` FIFO the model reacts to. A writer process
writes bytes to `pipe_path`; each chunk becomes a `named_pipe_data_received` event; the model
answers with `write_named_pipe_data`, whose bytes are written to an optional second FIFO
(`response_pipe_path`) that a reader process drains. NetGet owns the plumbing; the model owns the
payload — the same contract as `socket_file`, addressed by a FIFO path.

**State**: Experimental. **Platform**: Unix only; the whole module is `#![cfg(unix)]`.
**Privilege**: none — access is filesystem permissions on the FIFO (created `0o600`), not ports.
**Windows**: out of scope. A Windows named pipe (`\\.\pipe\...`) is a different primitive and
would be a separate `#[cfg(windows)]` implementation.

## Startup

| Parameter | Required | Meaning |
|---|---|---|
| `pipe_path` | yes | FIFO to create and READ; a writer writes here |
| `response_pipe_path` | no | second FIFO to create and WRITE model output to; a reader reads here |

No port is involved; `default_binding()` returns empty binding defaults so `server_startup.rs`
does not demand one (the same reason `socket_file` needs it).

**An existing node is reused only if it really is a FIFO.** `pipe_path` comes from the model or an
MCP caller, so `ensure_fifo` stats it with `symlink_metadata` (no symlink following) and refuses to
start if it exists and is anything other than a FIFO, naming what it found — an arbitrary-file
clobber guard copied from `socket_file`.

## I/O model (why O_RDWR)

The read FIFO is opened `O_RDWR | O_NONBLOCK` and driven by tokio `AsyncFd`:

- `O_RDWR` returns immediately (no blocking wait for a writer to appear) and keeps both a reader
  and a writer reference open, so `read()` returns `EAGAIN` — not a spurious `EOF` — every time an
  external writer closes. Opening `O_RDONLY` instead would deliver a `read()==0` EOF on every
  writer close and busy-loop the readable() poll.
- `O_NONBLOCK` is required for `AsyncFd`.

The response FIFO is opened `O_RDWR` (blocking form, which also returns immediately because we are
both ends): writes buffer in the kernel FIFO buffer even before a reader attaches, and the reader
drains them when it opens the path. The server never reads the response FIFO, so an external
reader gets all the bytes.

## What the model sees and controls

**Event**: `named_pipe_data_received` — `data` + `encoding`. Emitted for every chunk read from the
FIFO. It is the only event and it is always emitted (no declared-but-never-fired traps).

**Action**: `write_named_pipe_data` — `data` + optional `encoding`. Returns `ActionResult::Output`,
written to the response FIFO. If no `response_pipe_path` was configured the output has nowhere to
go and is dropped with a WARN on both log channels (never silently).

### Encoding is explicit in both directions

Inbound: `data` is text when every byte is printable ASCII, hex otherwise, and `encoding`
(`"utf8"`/`"hex"`) says which. Outbound: `write_named_pipe_data` reads the same `encoding` field,
defaulting to `"utf8"`, and decodes hex when asked. No auto-detection — `"48656c6c6f"` is both
valid text and valid hex — so echoing a payload back unchanged means passing back the same `data`
*and* `encoding`. Invalid hex is an action error, not a panic. This mirrors the `send_tcp_data`
fix.

## Failure and lifecycle

- **`spawn()` awaits readiness** (both FIFOs created and opened, read fd registered with the
  runtime) and returns `Err` on any failure, so `server_startup` sets `ServerStatus::Error`.
- **Fail closed** on LLM error: nothing is written, and the error is logged at ERROR on both the
  tracing log and the status stream. A reader that gets no bytes is how the other fixed protocols
  behave on an LLM failure — better than a permissive default.
- The read/dispatch loop is registered via `register_server_task`, so `stop_server` aborts it and
  releases the fds. A `FifoCleanup` guard is moved *into* the task, so aborting the task drops the
  guard and unlinks every FIFO node this server created — cleanup on stop, even on abort.

## Not implemented

Back-pressure, accumulation (`wait_for_more`), multiple concurrent named-pipe servers sharing a
path, and any framing — each `read()` chunk is one event. Responses to `CloseConnection` /
`WaitForMore` are meaningless for a connectionless FIFO sink and are ignored.

## Verified

`tests/server/named_pipe/e2e_test.rs`: a real, independent `std::fs` writer writes `PING\n` to the
input FIFO; the mocked LLM answers `write_named_pipe_data` `PONG\n`; a real `std::fs` reader reads
`PONG\n` off the response FIFO. Asserts actual bytes on the pipe, not "it opened".
