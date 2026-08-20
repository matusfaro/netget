# Telnet Server Implementation

## Overview

A line-based text server on the Telnet port. Every byte the client sees is produced by a
handler — script, static or LLM. It is deliberately **Telnet-lite**: no option negotiation, no
terminal emulation, no filesystem behind it.

**Status**: Experimental
**Port**: 23 (privileged — `privilege_requirement` is `PrivilegedPort(23)`)
**Feature**: `telnet`
**Files**: `mod.rs` (accept loop, read loop), `actions.rs` (actions, events, metadata)

## What this is not

**No IAC option negotiation.** RFC 854's `IAC WILL/WONT/DO/DONT` sequences are not parsed and
not answered — they arrive as ordinary bytes inside the first `telnet_message_received`
message. A real `telnet(1)` client opens by sending several negotiation sequences, so the first
message a handler sees will contain 0xFF-prefixed junk and the client will never get the echo
and line-mode settings it asked for. **Test with `nc`, not `telnet`.** The previous version of
this document claimed the `telnet` CLI "works"; that is only true in the sense that bytes flow.

Also absent: character-at-a-time mode, server-side line editing, terminal type / window size
(`TTYPE`, `NAWS`), ANSI handling (colour codes are just bytes you emit), TLS or any encryption,
and any form of authentication that the handler does not invent for itself.

### Not a dependency, despite `Cargo.toml`

`Cargo.toml` has `telnet = ["nectar"]`, but no code in this module references `nectar` — the
codec is `tokio::io::BufReader::read_line`. The dependency is dead weight; removing it is an
edit to `Cargo.toml`, which this module does not own.

## Architecture

### Connection flow

1. Accept TCP (`create_reusable_tcp_listener`).
2. Register the connection in `ServerInstance` (with `ProtocolConnectionInfo::empty()` — there
   is no Telnet-specific connection info variant). Byte/packet counters and `last_activity` are
   updated on every read and write via `update_connection_stats`; they used to stay at zero,
   so the dashboard drew every telnet peer as `↓0 ↑0`.
3. Register a peer command channel (`server::peer_support`), so the dashboard's
   `[ message this peer ]` / `[ disconnect this peer ]` — and `AppState::send_to_peer` generally
   — can inject `send_telnet_*` / `close_connection` into this connection through the same
   executor the handlers use. The handle is removed on the close path.
4. If started with `send_first: true`, raise `telnet_connection_opened` and write the result —
   this is the only way to greet before the client types.
5. `read_line` in a loop; per line, raise `telnet_message_received`, write the result.
6. `close_connection` sets a flag that breaks the read loop and shuts the socket down.
7. Mark the connection closed in `ServerInstance`.

An idle telnet connection is **not** reaped: `AppState::cleanup_old_connections` only touches
protocols whose metadata declares `connectionless`, and Telnet does not. Before that scoping, a
peer whose line was parked more than 10s for a `manual` answer was evicted from state and shown
as closed while its socket was fine.

The accept-loop `JoinHandle` is registered with `AppState::register_server_task()`, so
`stop_server` aborts it and releases port 23. `spawn_with_llm_actions` propagates bind failure
with `?`.

Processing is strictly sequential: one line is fully handled before the next is read. There is
no Idle/Processing/Accumulating state machine and no `queued_data` — TCP backpressure does the
queueing. (Earlier revisions of this file described a state machine that does not exist.)

### Fixed failure modes

- `send_telnet_line` used to turn a `line` ending in `"\n"` into one ending in a lone `"\r"`,
  losing the line feed. It now always ends the line with exactly one CRLF.
- Log previews sliced `&text[..100]`, which panics when byte 100 falls inside a multi-byte
  UTF-8 character — reachable from any client that sends non-ASCII. Previews now truncate on a
  character boundary (`preview()` in `mod.rs`).
- `close_connection` broke out of the action loop only, leaving the read loop running and the
  socket open; it now closes the connection.
- Responses were round-tripped through `String::from_utf8_lossy` before being written, which
  silently replaced any non-UTF-8 byte with U+FFFD. Bytes are now written verbatim.
- `read_line` errors (non-UTF-8 input) were swallowed by `while let Ok(..)` and looked like a
  clean disconnect; they are now logged.

## LLM Integration

`call_llm` is used for both events, so script and static handlers run in-process with **zero**
LLM calls (`call_llm` → `try_execute_event_handler`).

### Events

| Event                      | When                                              | Parameters |
|----------------------------|---------------------------------------------------|------------|
| `telnet_connection_opened` | on connect, **only if `send_first: true`**        | –          |
| `telnet_message_received`  | one complete line arrived                         | `message`  |

`message` is the line with its trailing CR/LF and surrounding whitespace trimmed.

### Actions

| Action                 | Bytes written                                  | Parameters          |
|------------------------|------------------------------------------------|---------------------|
| `send_telnet_message`  | `message` verbatim, nothing added               | `message` (required)|
| `send_telnet_line`     | `line` + exactly one CRLF                       | `line` (required)   |
| `send_telnet_prompt`   | `prompt` verbatim, no newline (default `"> "`)  | `prompt` (optional) |
| `wait_for_more`        | nothing — read the next line first              | –                   |
| `close_connection`     | nothing — closes the connection                 | –                   |

`send_telnet_message` is the escape hatch for exact control (e.g. a banner and prompt in one
write, or a byte sequence with no line ending). There are no async (user-triggered) actions.

### Startup parameters

| Parameter    | Type    | Effect                                              |
|--------------|---------|-----------------------------------------------------|
| `send_first` | boolean | `true` raises `telnet_connection_opened` on connect |

## Storage

None. The protocol holds no session state, no filesystem and no user database. Anything that
looks like state — the current directory, a login prompt sequence, a command history — lives in
the handler's own memory.

## Testing

**There is no E2E test under `tests/server/telnet/`** (the directory does not exist).
`tests/client_handle_test.rs` covers the dashboard flow end to end without a model: a telnet
server and a telnet client both on the `*` → manual rule, the human's answer to the client's
parked `telnet_connected` arriving as `telnet_message_received` on the server, the connection
surviving the idle sweep while that question waits, and the server's answer reaching the client.
Verify anything else by hand with a raw client, which avoids the IAC problem:

```
nc localhost 2323
help
exit
```

## Example prompts

### Interactive shell

```
listen on port 2323 via telnet with send_first
On connection open send "NetGet 1.0\r\n$ "
help  -> list the commands: help, date, echo <text>, exit
date  -> the current date and time
echo  -> the text after the command
exit  -> "bye" then close_connection
Send "$ " after every response
```

### Line collector

```
listen on port 2323 via telnet
Buffer lines with wait_for_more until the client sends END
Then reply with the number of lines collected
```
