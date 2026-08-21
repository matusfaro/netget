# Redis Protocol Implementation

RESP2 server. `redis-protocol` v6.0 parses inbound frames; responses are encoded
by hand in `actions.rs` (`encode_*`, the single source of truth — each reply verb's
executor returns the encoded bytes as `ActionResult::Output`). The LLM owns every
reply — there is no key space, no storage, and no command dispatch table in Rust.

**State**: Experimental — LLM-authored, not human-reviewed. Verified against
`redis-cli` for the array/bulk/integer/nil encodings.
**Port**: 6379 by default. **Privilege**: `None` (6379 > 1024).
**Stack**: `ETH>IP>TCP>Redis`. **Spec**: RESP2.

## What the model sees and controls

**Event**: `redis_command`, one per decoded RESP frame.

| Field | Notes |
|---|---|
| `command` | the frame flattened to a single space-separated string, e.g. `SET mykey hello` |

That string is all the model gets — arguments are not split out, and there is no
argument-count or key field. A command whose argument contains a space is
indistinguishable from two arguments.

**Actions** (all sync; there are no async actions):

| Action | Parameters | Wire form |
|---|---|---|
| `redis_simple_string` | `value` (required) | `+value\r\n` |
| `redis_bulk_string` | `value` (optional; `null` ⇒ nil) | `$len\r\n…\r\n` |
| `redis_integer` | `value` (required, i64) | `:value\r\n` |
| `redis_array` | `values` (required, array) | `*n\r\n…` |
| `redis_error` | `message` (required) | `-message\r\n` |
| `redis_null` | — | `$-1\r\n` |
| `close_this_connection` | — | flushes pending output, then closes |

`redis_array` element mapping, exactly as implemented in `actions.rs::encode_array`:

| JSON element | RESP2 |
|---|---|
| string | bulk string |
| integer | RESP integer (`:42\r\n`), **not** a bulk string |
| float | bulk string of its text form |
| `true` / `false` | bulk string `"1"` / `"0"` |
| `null` | nil bulk string |
| array / object | its JSON text in a bulk string |

Verified with `redis-cli --no-raw`: `["k1", 42, true, null, {"a":1}]` returns
`"k1"`, `(integer) 42`, `"1"`, `(nil)`, `"{\"a\":1}"`.

### Failure behavior

- **No response action** → `-ERR no response produced for this command`. (Redis
  is strictly request/response; staying silent would hang the client until its
  own timeout.)
- **LLM call fails** → `-ERR LLM error: …`.
- **Action result that is `Custom` rather than `Output`** → logged at WARN and
  skipped (no Redis action returns `Custom` any more; the arm exists so a
  regression is loud); if nothing else was produced the no-response error above
  is sent.
- **Undecodable RESP** → the connection is closed.
- **Incomplete frame larger than 64 MB** (`MAX_PENDING_FRAME_BYTES`) → an error
  is sent and the connection closed. Without this cap a client announcing
  `$2000000000\r\n` and stalling would grow the per-connection buffer without
  bound.

## Architecture

- `spawn_with_llm_actions` binds with `?`, so a bind failure surfaces as
  `ServerStatus::Error` rather than a phantom `Running`, and registers the
  accept-loop `JoinHandle` via `AppState::register_server_task()` so
  `stop_server` releases the socket.
- One task per connection. `handle_connection` wraps `run` so the connection is
  always marked `Closed` in `AppState` on exit; `update_connection_stats` is
  called for bytes/packets in both directions.
- Read into a `Vec`, `decode()` frames off the front, drain what was consumed.
  Multiple pipelined frames in one read are processed in order, each with its own
  LLM call.
- Reply verbs are encoded in `execute_action` (`actions.rs`), which returns the
  RESP bytes as `ActionResult::Output`. The read loop only concatenates `Output`
  bytes (flattening `Multiple`) and writes them; it encodes nothing itself except
  the three errors it synthesises (frame cap, LLM failure, no-response), which
  call the same `actions::encode_error`.
- Responses for one command are accumulated into a single buffer and written
  once, so a `close_this_connection` issued alongside a reply still flushes.
- No per-connection state machine: commands on one connection are handled
  strictly sequentially by the read loop, so concurrent LLM calls cannot happen.

### Dashboard injection (`[ message this peer ]` / `[ disconnect this peer ]`)

Every connection registers a peer handle (`server::peer_support`) before its first read,
so a manual `*` rule parking the first command still leaves the operator able to reach
it. The stream is `tokio::io::split` and the write half is an `Arc<Mutex<..>>` shared
with the generic peer command task; the handle is removed on every exit path through the
single cleanup in `handle_connection`. Counters (`update_connection_stats`) move on every
read and every write, including the frame-cap error.

**The whole vocabulary is injectable.** All six reply verbs (`redis_simple_string`,
`redis_bulk_string`, `redis_array`, `redis_integer`, `redis_error`, `redis_null`) are
RESP-encoded inside `execute_action` and return `ActionResult::Output`, which the generic
peer task writes to the connection's write half — so an injected reply verb puts exactly
the same bytes on the wire as the read-loop path (they share the `encode_*` functions in
`actions.rs`). `close_connection` (what "disconnect this peer" sends; an explicit arm in
`execute_action`, not offered to the model — its verb is `close_this_connection`)
half-closes and the client reads EOF. This used to be the "Custom-result gap": the verbs
returned `Custom` and only the read loop could encode them, so injection reported
`Executed` without writing. `tests/server/redis/peer_inject_test.rs` pins the fixed
behavior (`Sent`, bytes at the client socket, counters moved).

Injected replies are unsolicited from the client's point of view: `redis-cli` sitting at
its prompt will parse the frame as the reply to its *next* command. That is inherent to
injecting into a strictly request/response protocol, not a NetGet bug.

## Not implemented

- **RESP3** — no `HELLO 3`, push messages, doubles, maps or sets.
- **Inline commands** (`PING\r\n` typed into `nc`) — only RESP arrays decode;
  anything else closes the connection. `redis-cli` and `redis-rs` always send
  RESP arrays, so this only affects hand-typed sessions.
- **AUTH / SELECT / MULTI / EXEC / WATCH** — reach the model as ordinary
  commands with no special handling. There is no auth gate.
- **Pub/sub**, **blocking commands** (`BLPOP`), **Lua** (`EVAL`), **cluster**,
  **replication**, **persistence**.
- **TLS**.
- Server-initiated pushes: nothing can be written except in reply to a command.

## Testing

`tests/server/redis/e2e_test.rs` (declared in `tests/server/mod.rs`). Mocked by
default:

```bash
./cargo-isolated.sh test --no-default-features --features redis \
    --test server::redis::e2e_test -- --test-threads=100
```

Real-client checks used during review, via `--mcp-http` with a static handler so
no model is involved:

```bash
netget --mcp-http 18899 &
# start_server protocol=redis port=16379 event_handlers=[{redis_command → static redis_array}]
redis-cli -p 16379 --no-raw KEYS '*'
```

## Example prompts

```
Start a Redis server on port 6379. Reply PONG to PING with redis_simple_string.
For GET on a key you have not seen, use redis_null. For SET, reply OK.
```

Prefer a script or static handler for anything deterministic — every command
otherwise costs one model round-trip.

## References

- [RESP2 specification](https://redis.io/docs/reference/protocol-spec/)
- [redis-protocol crate](https://docs.rs/redis-protocol/)
- [redis-rs](https://docs.rs/redis/) — used by the E2E tests
