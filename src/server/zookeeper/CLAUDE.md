# ZooKeeper Server Implementation

## Status: INCOMPLETE — hidden from the LLM

`metadata()` declares `DevelopmentState::Incomplete`, so `is_available_to_llm()` returns false
and the model is never offered this protocol. The port can still be opened explicitly (MCP
`start_server`, `--base-stack zookeeper`), and the server logs a WARN saying so on startup.

It was demoted from `Experimental` for the same reason Kafka was: **no real ZooKeeper client
can complete a session against it.**

### Why it is Incomplete

1. **No session handshake.** A ZooKeeper client's first message is a `ConnectRequest`:

   ```
   [4 len][4 protocolVersion][8 lastZxidSeen][4 timeOut][8 sessionId][4+16 passwd][1 readOnly]
   ```

   It carries neither an xid nor an opcode. `parse_request` reads bytes 0..4 as the xid and
   4..8 as the opcode, so a connect attempt is reported as `operation: "unknown"`, and the
   `ConnectResponse` the client blocks on — a different frame shape entirely
   (`protocolVersion, timeOut, sessionId, passwd`) — is never produced. `zkCli.sh`, `kazoo`
   and `zookeeper-async` all hang or abort at this point. Nothing downstream of the handshake
   has ever run against a real client.

2. **The reply body is hand-encoded Jute hex.** `zookeeper_response` takes `data_hex`, so
   answering `getData` means the model must emit a Jute-serialized `GetDataResponse` — a
   `byte[]` vector followed by a 68-byte `Stat` — as a hex string. This violates the project's
   no-bytes rule (`CLAUDE.md`, "Action & event design rules"), and models do not produce it
   correctly. It is documented rather than removed because removing it without a structured
   encoder leaves a server that cannot answer anything at all.

3. **Only the request header is parsed.** The xid, the opcode and the leading path string are
   extracted; every other request field (watch flag, znode data on `create`/`setData`, ACLs,
   version numbers) is discarded and never reaches the handler.

### Route back to Experimental

In order:

1. Detect and answer the `ConnectRequest` before entering the request loop, and emit a
   `zookeeper_session_request` event so a handler can set the negotiated timeout and session id.
2. Replace `data_hex` with structured per-operation actions — `zookeeper_data` (`data`, `stat`),
   `zookeeper_children` (`children[]`, `stat`), `zookeeper_stat`, `zookeeper_create_result`
   (`path`) — and do the Jute encoding in Rust.
3. Decode the full request body per opcode so `create`/`setData` payloads reach the handler.
4. Add an E2E test driven by a real ZooKeeper client rather than hand-built byte sequences.

Implement watches only after all four; watches need a second, server-initiated frame path.

## Wire format handled today

```
Request   [4 len][4 xid][4 opcode][4 pathLen][pathLen path]... (rest not decoded)
Reply     [4 len][4 xid][8 zxid][4 err][body]
```

Opcodes recognized by name: 1 create, 2 delete, 3 exists, 4 getData, 5 setData, 6 getACL,
7 setACL, 8 getChildren, 9 sync, 11 ping, 12 getChildren2, 13 check, 14 multi. Anything else
is reported as `unknown` with the numeric `op_code` alongside.

## Event

### `zookeeper_request`

| Field | Type | Meaning |
|---|---|---|
| `xid` | integer | Request transaction id. **Echo this back.** |
| `operation` | string | Opcode name, or `"unknown"` |
| `op_code` | integer | Numeric opcode |
| `path` | string | Leading path string, or `""` when absent/undecodable |

Declared on the event type via `.with_actions([zookeeper_response])`, so the model is offered
the action rather than falling back to the protocol's full sync set.

## Action

### `zookeeper_response`

| Field | Required | Meaning |
|---|---|---|
| `xid` | no | Reply transaction id. **Omit it and the request's own xid is used.** |
| `zxid` | yes | Server change counter |
| `error_code` | yes | 0 OK, -101 NONODE, -110 NODEEXISTS, -102 NOAUTH |
| `data_hex` | no | Jute-serialized reply body, hex. Invalid hex is a hard error. |

### Correlation

A client matches replies to requests by xid alone. Two things guarantee it here:

- `xid` is carried in the event data, so a script handler can read it and a static handler can
  interpolate it — `"xid": "{{event.xid}}"`.
- If the action omits `xid`, the connection loop substitutes the xid of the request being
  answered. It never defaults to 0; xid 0 is not a valid reply to a real request (negative
  xids are reserved for pings and watch notifications) and leaves the client waiting forever.

## No storage

The server keeps no znode tree, no session table and no data of any kind. Every reply comes
from the handler. This is the only rubric item ZooKeeper has always passed.

## Robustness

Fixed while demoting, because the port can still be opened:

- **Remote panic (fixed).** `parse_request` cast the path length to `usize` before checking it.
  A length of `-1` became `usize::MAX`, `12 + usize::MAX` wrapped to `11`, the guard
  `payload.len() >= 11` passed, and `&payload[12..11]` panicked with start > end. Four
  attacker-chosen bytes killed the connection task. The length is now validated as `i32`
  against the bytes actually remaining, before any widening.
- **Frame length.** The 4-byte prefix is validated as `i32` in `8..=1048576` (ZooKeeper's own
  `jute.maxbuffer` default) before being widened. Casting first meant a negative length
  sign-extended to ~1.8e19 and any later check tested a number the sender never sent.
- **Accept loop.** A persistent accept error (EMFILE) used to be logged and retried
  immediately, spinning a hot loop that flooded the unbounded status channel. The loop now
  breaks and the listener stops.
- **Connection tracking.** Connections are now registered with `add_connection_to_server` and
  marked closed on exit; previously they never appeared in the TUI or MCP connection list.
- **Invalid `data_hex`** used to be silently dropped, sending a header with no body and
  desynchronizing the client for the rest of the connection. It is now an error.

The accept-loop `JoinHandle` is registered via `AppState::register_server_task()`, so
`stop_server` releases the port. Per-connection tasks are still untracked — a project-wide gap,
noted in the root `CLAUDE.md`.

## Testing

`tests/server/zookeeper/e2e_test.rs` builds request bytes by hand over a raw `TcpStream` and
never sends a `ConnectRequest`, which is why the handshake gap went unnoticed. See
`tests/server/zookeeper/CLAUDE.md`.
