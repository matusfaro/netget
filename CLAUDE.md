# NetGet — LLM-Controlled Network Protocol Server & Client

Rust CLI where an LLM (Ollama or any OpenAI-compatible endpoint) drives ~116 network
protocols as servers and ~90 as clients. NetGet owns the network stack; the LLM decides
what to say on the wire, either by reasoning per-request or via deterministic handlers.

Three ways to run it: interactive TUI (default), headless (`--mcp` / `--mcp-http`, see
`src/mcp_stdio/CLAUDE.md`), and non-interactive one-shot (`src/cli/non_interactive.rs`).

## Protocol inventory — always query, never trust a list

Protocol lists in docs go stale within weeks. Get ground truth from the registry:

```bash
# Every registered server protocol + maturity + implementation note
grep -n "register(Arc::new" src/protocol/server_registry.rs
grep -rn "DevelopmentState::" src/server/<protocol>/actions.rs   # that protocol's own claim

# At runtime (authoritative — reflects compiled-in features)
netget --mcp   # then call list_protocols / get_protocol_docs
```

Maturity lives in each protocol's `metadata()` (`ProtocolMetadataV2`, `src/protocol/metadata.rs`):

- **Stable** — real spec compliance, good LLM prompting, scripting support. Currently: `tor_relay`, `wireguard`, `openvpn`.
- **Beta** — human-reviewed, works against real clients (13 protocols).
- **Experimental** — LLM-authored, not human-reviewed. This is the overwhelming majority (~97).
- **Incomplete** — hidden from the LLM entirely (`is_available_to_llm()` returns false). Currently: `bgp`, `usb_smartcard`, `nfc`.

Treat `Experimental` as "compiles and has a test", not "works". Several are explicit
placeholders with zero actions and zero events (`amqp`, `mqtt`, and five
`bluetooth_ble_*` profile wrappers) — check `get_sync_actions()` before assuming behavior.

Per-protocol docs: `src/server/<protocol>/CLAUDE.md` and `tests/server/<protocol>/CLAUDE.md`.
**Read both before modifying a protocol.** Note that these files are frequently more
aspirational than the code — verify claims against the source.

## Architecture

**Modules**: `cli/` (TUI, startup, args) · `server/<protocol>/` · `client/<protocol>/` ·
`protocol/` (registries, metadata, spawn context) · `state/` (app state) · `llm/` (backends,
prompting, actions) · `events/` (coordination) · `scripting/` (deterministic handlers) ·
`mcp_stdio/` (MCP server) · `easy/` (simplified layer for small models).

**Decentralization (CRITICAL)**: no centralized per-protocol logic. Each protocol implements
traits independently. The only legitimate central touchpoints are:

- `protocol/server_registry.rs` / `protocol/client_registry.rs` — one feature-gated `register()` line
- `Cargo.toml` — feature flag
- `src/server/mod.rs` / `src/client/mod.rs` — feature-gated `pub mod`
- `tests/server/mod.rs` / `tests/client/mod.rs` — feature-gated `pub mod` (see the footgun below)

`cli/server_startup.rs` and `cli/client_startup.rs` are **fully generic** — they look the
protocol up in the registry and call `spawn(ctx)` / `connect(ctx)`. There is no
per-protocol match statement. Do not add one.

**`ProtocolConnectionInfo`** (`state/server.rs`) is a generic `serde_json::Value` wrapper,
not an enum. Adding a protocol does not require touching it.

**Connection I/O**: split `TcpStream` with `tokio::io::split()` (never clone). Never hold a
`Mutex`/`RwLock` guard across an `.await` that performs I/O or an LLM call — acquire, copy
out what you need, drop the guard in an inner scope, then await.

**Per-connection state machine** (Idle → Processing → Accumulating) prevents concurrent LLM
calls on one connection and queues data arriving mid-call. Note: `state/machine.rs` defines a
generic `StateMachine<S>` that **nothing uses** — every protocol hand-rolls its own copy.
Copy the TCP implementation (`src/server/tcp/mod.rs`) as the reference.

**Actions**: protocols implement `ProtocolActions` (`src/llm/actions/protocol_trait.rs`) with
async actions (user-triggered) and sync actions (network-event-triggered), in
`src/server/<protocol>/actions.rs`. Clients implement `Client` (`llm/actions/client_trait.rs`).

**Handling modes**, in priority order — a request takes the first that matches:
1. **Script handler** — inline Python/JS, runs in-process, no LLM call
2. **Static handler** — fixed actions, no LLM call
3. **LLM** — one model round-trip per event

Scripts and static handlers are the right default for deterministic behavior (echo, canned
responses, routing). Reserve the LLM for responses that genuinely require reasoning.

### Action & event design rules (CRITICAL)

**Never put raw bytes or base64 in action parameters or event data.** Models cannot reliably
produce or parse them. Use structured fields: `{"method": "GET", "path": "/", "headers": {…}}`,
not `{"data": "SGVsbG8="}`.

**If an action does document a hex or encoded field, the executor must actually decode it.**
There is a live bug of exactly this shape: `send_tcp_data` is documented as accepting
"text or hex-encoded binary" in three places (`src/server/tcp/actions.rs:355,359`, the
protocol docs served to the LLM, and `src/server/tcp/CLAUDE.md`), but
`execute_send_tcp_data` does `data.as_bytes()` (`src/server/tcp/actions.rs:242,276`) — hex is
never decoded, so a model following the documentation puts literal ASCII on the wire.
Inbound data *is* hex-encoded when non-printable, so the round-trip is asymmetric. When you
touch a protocol, verify its documented encoding matches its executor.

**Protocols must not implement storage** — no databases, filesystems, or persistence written
into a protocol's Rust implementation. The LLM supplies all data via actions, scripts, static
responses, or server memory. MySQL has no tables; the model answers every query.

The sanctioned exception is the generic SQLite facility (`src/state/sqlite.rs`, feature
`sqlite`, included in `all-protocols` and therefore in default builds). The LLM can call
`create_database` / `execute_sql` / `list_databases` / `delete_database` at runtime, scoped to
a server, a client, or globally. The point of the rule is that storage is a *generic runtime
capability the model opts into*, never something a protocol hardcodes. Two caveats worth
knowing: `create_database` defaults to **file-backed**, writing `./netget_db_<name>.db` into
the process's working directory, and server/client-scoped databases are deleted when their
owner closes while global ones persist.

### Privilege model

`ProtocolMetadataV2::privilege_requirement` declares `None` / `PrivilegedPort(u16)` /
`RawSockets` / `Root`, checked against `SystemCapabilities` (`src/privilege.rs`) in
`server_startup.rs` before spawn. Two known defects to avoid propagating:

- The gate is `requires_privileges && !system_caps.can_bind_privileged_ports`
  (`src/cli/server_startup.rs:348`) — it ANDs in the *port-binding* capability even for
  `RawSockets`/`Root` requirements, so a process holding `CAP_NET_BIND_SERVICE` but not
  `CAP_NET_RAW` skips the raw-socket check and fails later with an opaque `EPERM`.
- Only ~23 of 116 protocols declare a requirement at all. Protocols defaulting to privileged
  ports (`smtp` 25, `ldap` 389, `imap` 143, `pop3` 110, `ipp` 631, `syslog` 514) and `igmp`
  declare nothing and get no preflight check.

Declare `privilege_requirement` on any new protocol that needs raw sockets, a TUN device, or
a port below 1024. Raw-socket/pcap protocols (`arp`, `datalink`, `icmp`, `isis`) start via
fire-and-forget `spawn_blocking` and report `Running` even when the capture handle fails to
open — if you touch them, propagate that failure into `ServerStatus::Error`.

`get_dependencies()` / `ProtocolDependency` (`src/protocol/dependencies.rs`) is fully built
but **no protocol implements it and nothing calls it**. Either adopt it or delete it; don't
assume it's providing preflight checks.

## Adding a server protocol

1. `src/server/<protocol>/mod.rs` — server loop, dual logging, connection tracking, register
   the accept-loop `JoinHandle` via `AppState::register_server_task()` (required for
   `stop_server` to actually release the socket)
2. `src/server/<protocol>/actions.rs` — implement `ProtocolActions`: `metadata()` (state +
   privilege), `get_startup_parameters()`, async/sync actions, `get_event_types()`,
   `execute_action()`
3. `src/server/<protocol>/CLAUDE.md` — implementation notes, library choice, limitations
4. `src/server/mod.rs` — feature-gated `pub mod`
5. `src/protocol/server_registry.rs` — feature-gated `register()`
6. `Cargo.toml` — feature flag, optional deps, add to `all-protocols`; add to `dist` /
   `dist-darwin` / `dist-windows` only if it links no system library unavailable on that target
7. `tests/server/<protocol>/e2e_test.rs` — mocked E2E (see Testing)
8. `tests/server/<protocol>/CLAUDE.md` — strategy, mock expectations, LLM call budget
9. **`tests/server/mod.rs` — add `pub mod <protocol>;`** (see footgun below)

Client protocols follow the same shape against `client_registry.rs`, `src/client/mod.rs`,
`tests/client/mod.rs`, and the `Client` trait. Consult `CLIENT_PROTOCOL_FEASIBILITY.md` first.
Note clients are less finished than servers: their `JoinHandle` is never stored, so
`remove_client()` does not stop the network loop.

**Startup parameters**: every key a caller may pass must be declared in
`get_startup_parameters()`. `StartupParams` **panics** on an undeclared key or a wrong-typed
value (`src/protocol/spawn_context.rs:42` and all `get_*` accessors). Because the JSON comes
from the LLM or an MCP client, this is remotely triggerable: over MCP the panic kills the
per-request task before it can reply, so the caller hangs forever with no error and the
server is left stuck in `Starting`. Until those accessors return `Result`, be exhaustive in
`get_startup_parameters()`.

## Testing

Black-box and prompt-driven: the LLM (or a mock of it) interprets an instruction, and tests
validate the result with real protocol clients.

**Tests define expected behavior. When a test fails, fix the implementation, not the test.**
"The test passes but the implementation doesn't work" means you are not done. Only change a
test when its expectation is genuinely wrong.

### Test location policy (CRITICAL)

**All tests live in `tests/`. Never add `#[cfg(test)] mod tests` to `src/`.** Tests reach
internals via `use netget::` public APIs; make items public or refactor if needed.

This policy is currently violated by 8 files in `src/` containing ~23 unit tests
(`llm/config`, `llm/reference_parser`, `llm/hybrid_manager`, `protocol/event_logger`,
`protocol/log_template`, `system_stats`, `server/proxy/cert_cache`). Migrate them if you are
working nearby; do not add more.

### The mod.rs footgun (CRITICAL)

`tests/server.rs` and `tests/client.rs` only compile submodules explicitly declared in
`tests/server/mod.rs` / `tests/client/mod.rs`. **A test directory that exists on disk but is
not declared is silently never compiled and never run — no error, no warning.**

This is currently the single largest hole in the suite: **15 of 116 server test dirs and 61 of
83 client test dirs are orphaned**, including complete, correctly-gated E2E suites for `arp`,
`whois`, `bitcoin`, `igmp`, `tls`, `sip`, and every USB protocol. Verify with:

```bash
comm -23 <(ls -d tests/server/*/ | sed 's|tests/server/||;s|/||' | sort) \
         <(grep -oE "pub mod [a-z0-9_]+" tests/server/mod.rs | awk '{print $2}' | sort)
```

### Mocks

Default mode needs no Ollama. `tests/helpers/mock_ollama.rs` runs a real in-process axum
server implementing `/api/chat`, `/api/generate`, `/api/tags`. Configure with `.with_mock()`
and **always finish with `server.verify_mocks().await?`** — without it the test asserts
nothing about LLM interaction. An unmatched request returns HTTP 500 with a clear error, and
`verify_calls()` dumps full call history on mismatch.

UDP-style protocols (DNS, STUN, NTP, DHCP, BOOTP, TFTP…) **must** use
`.respond_with_actions_from_event()` to echo the client's random transaction/query ID back.
Static mocks with hardcoded IDs cause client timeouts. See `tests/server/dns/CLAUDE.md`.

```rust
.on_event("dns_query")
.and_event_data_contains("domain", "example.com")
.respond_with_actions_from_event(|e| serde_json::json!([{
    "type": "send_dns_a_response",
    "query_id": e["query_id"].as_u64().unwrap_or(0),   // ← must be dynamic
    "domain": "example.com", "ip": "93.184.216.34"
}]))
.expect_calls(1)
```

Keep each suite under ~10 LLM calls: reuse servers, bundle scenarios, prefer script mode.
Bind to localhost only (127.0.0.1 / ::1); never contact external endpoints.

### Running tests

```bash
# Single protocol (fast: 10-30s)
./cargo-isolated.sh test --no-default-features --features tcp \
    --test server::tcp::e2e_test -- --test-threads=100

# Full sweep (slow, 3GB+ RAM)
./cargo-isolated.sh test --all-features --no-fail-fast -- --test-threads=100
```

**Always pass `--test-threads=100`.** Single-threaded runs are 10-20x slower; if a test hangs,
fix the hang rather than serializing the suite.

`./test-e2e.sh <protocol>` runs mocked; `./test-e2e.sh --use-ollama <protocol>` uses a real model.

### CI reality

`.github/workflows/release.yml` is the only workflow. It triggers on `v*` tags and manual
dispatch, and runs `cargo build` for the `dist*` feature sets across 6 targets.
**No CI job ever runs `cargo test`.** There is no PR gate and no lint job — the entire
1,038-test suite is developer-run only. Run the relevant tests yourself before claiming done.

## Building

`./cargo-isolated.sh` wraps cargo with sccache, disabled incremental, stable path remapping,
and automatic logging. Despite the name it now uses the **shared `target/` directory** —
concurrent runs from different sessions contend on the same lock, so serialize your builds.

Logs land in `./tmp/netget-<command>-<PPID>.log`; the path is printed on each run.

```bash
./cargo-isolated.sh --print-last                     # whole log
./cargo-isolated.sh --print-last | grep "error\[E"   # all compile errors at once
./cargo-isolated.sh --print-last | grep -B2 -A5 "^error:"
./cargo-isolated.sh --print-last | grep -A5 "FAILED"
```

**Fix every error in one pass.** Builds cost 10s-2min; rebuilding after each individual fix
wastes hours. Build once, extract the full error list from the log, fix all of it, rebuild once.

Use minimal features. `--all-features` compiles 50+ protocols and their dependency trees
(1-2 min, 3GB+ RAM); `--no-default-features --features <protocol>` takes 10-30s. Reach for
`--all-features` only for release validation.

Kill stuck builds with `./cargo-isolated-kill.sh`, never `pkill cargo`.

### Features unavailable in Claude Code for Web

Detect with `./am_i_claude_code_for_web.sh` or `[ "$CLAUDE_CODE_REMOTE" = "true" ]`.
These need system libraries absent there — derive the current list from `Cargo.toml`
rather than a hardcoded copy:

| Group | Needs | Features |
|---|---|---|
| Bluetooth LE | `libdbus-1-dev` | all `bluetooth-ble*` (18) |
| USB | `libusb-1.0-dev` | `usb`, `usb-keyboard`, `usb-mouse`, `usb-serial`, `usb-msc`, `usb-fido2`, `usb-smartcard` |
| NFC | `pcsclite` | `nfc`, `nfc-client` |
| Protobuf | `protoc` | `etcd`, `grpc`, `kubernetes`, `zookeeper` |
| Packet capture | `libpcap` | `datalink`, `arp`, `isis` |
| Other | — | `kafka` (untested), `smb-client` (`libsmbclient`) |

```bash
# Safe pattern
./cargo-isolated.sh build --no-default-features --features tcp,http,dns
```

## MCP surface

`--mcp` (stdio) and `--mcp-http PORT` expose 12 tools sharing the TUI's code paths. See
`src/mcp_stdio/CLAUDE.md`. Current gaps to keep in mind when testing a protocol through MCP:

- **No client tools.** `list_protocols` lists client protocols and `get_protocol_docs`
  instructs the caller to use `open_client`, but no MCP tool starts a client.
- `start_server` cannot pass `interface` or `mac_address` (hardcoded `None`), so
  interface-bound protocols (`arp`, `datalink`, `icmp`, `isis`) can't be targeted at a real
  NIC; nor `scheduled_tasks`, `initial_memory`, or `feedback_instructions`.
- `send_first` is accepted by `start_server_from_action` as `_send_first` and **ignored
  entirely** — on every path, not just MCP.
- Unknown action names in `event_handlers` are accepted at startup and silently do nothing at
  runtime; the client gets no response and the access log records the action as if it ran.
- `get_protocol_docs` returns the TUI LLM's documentation (`open_server`, `base_stack`), which
  describes an API MCP callers cannot invoke.
- No state persists across process restarts, and `stop_server`/`stop_all` skip
  `cleanup_server_tasks()`, orphaning scheduled tasks.

A long-running `netget --mcp` process keeps executing its original binary image after a
rebuild. When behavior contradicts the source, confirm which build is actually running before
concluding there's a bug.

## Logging

**Dual logging everywhere**: tracing macros (`error!`/`warn!`/`info!`/`debug!`/`trace!`) →
`netget.log`, and `status_tx.send()` → TUI/MCP status stream. Levels: ERROR critical, WARN
non-fatal, INFO lifecycle, DEBUG summaries, TRACE full payloads.

Every status/event channel is an **unbounded** `mpsc` — there is no backpressure anywhere.
Don't add high-frequency per-byte messages to these channels.

## Scheduled tasks

Three scopes: **Global** (any server), **Server** (auto-cleaned on close), **Connection**
(auto-cleaned on close). Create via the `open_server` action's `scheduled_tasks` array or the
`schedule_task` action; add `connection_id` for connection scope. Parameters: `task_id`,
`recurring`, `interval_secs`/`delay_secs`, `instruction`. Use connection scope only for
long-lived connections (SSH, WebSocket); short-lived request/response protocols should use
server scope.

## Multi-instance collaboration

Assume other agents work in this repo concurrently.

- **Shared files** (`Cargo.toml`, both registries, `server/mod.rs`, `client/mod.rs`, both test
  `mod.rs` files, `state/server.rs`): use `Edit`, add incrementally, never overwrite wholesale.
- **Pause and report** if you hit an error in code you did not modify.
- `--ollama-lock` serializes LLM API access (default in tests). Concurrent `git` work should
  use worktrees.
- Never `pkill cargo`; use `./cargo-isolated-kill.sh`.
- The user runs `netget --mcp` interactively. **Never kill netget processes.**

## Known systemic issues

Read before assuming a subsystem is sound:

- `StartupParams` panics on malformed input, remotely reachable (above).
- Byte-index string truncation (`&s[..N]` guarded only by `s.len() > N`) across `src/llm/`
  panics on multi-byte UTF-8 at the cut point; the strings involved are LLM output and
  event descriptions.
- `call_llm_for_feedback` passes an empty action list to the validator while telling the model
  actions are available, so `feedback_instructions` can only no-op or hard-fail
  (`src/llm/action_helper.rs:752`).
- `git` and `mercurial` call `generate_with_retry` directly, bypassing the rate limiter, the
  retry/repair loop, and event-handler dispatch — script/static handlers are ignored for them.
- On LLM failure most protocols reset to Idle and write nothing, leaving the peer to hang
  until its own timeout.
- Per-connection tasks are untracked, so `stop_server` does not cancel in-flight connections.
- `AppState` is one global `RwLock` over everything — a throughput ceiling, not a deadlock.
- ~50 of the 63 root markdown files are one-off session/status reports last touched in 2025.
  `ARCHITECTURE.md`, `METADATA_EXAMPLES.md`, `CLIENT_PROTOCOL_FEASIBILITY.md`,
  `LICENSE_ANALYSIS.md`, `SYSTEM_DEPENDENCIES_macOS.md`, `TERMUX_INSTALL.md`, and
  `PROTOCOL_MIGRATION_GUIDE.md` are the durable ones. Do not add new status-report files.

## Git

- Never `git stash`. Always merge with `--no-ff`. Never amend, rebase, or squash shared history.
- Combine `git add` and `git commit` in one command chain.
- Commit as Matus Faro <matus@matus.io>, GPG-signed:
  ```bash
  git config user.name 'Matus Faro' && git config user.email 'matus@matus.io' && \
  git config commit.gpgsign true && git config user.signingkey matus@matus.io
  ```
- Conventional Commits, one logical change per commit. No co-author or bot attribution
  trailers of any kind.
