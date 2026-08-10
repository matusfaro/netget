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

- **Stable** — real spec compliance, good LLM prompting, scripting support. Currently **only
  `wireguard`**. `tor_relay` and `openvpn` were rated Stable and neither had ever been
  validated against a real client; both were demoted on inspection.
- **Beta** — human-reviewed, works against real clients (12 protocols).
- **Experimental** — LLM-authored or newly implemented, not fully reviewed. The overwhelming
  majority (~99).
- **Incomplete** — hidden from the LLM entirely (`is_available_to_llm()` returns false). Down to
  **one**, and it is a platform limit rather than unfinished work:
  - `bluetooth_ble_beacon` — **macOS cannot do this at all.** A beacon *is* its advertising
    payload, and `CBPeripheralManager.startAdvertising:` accepts only
    `CBAdvertisementDataLocalNameKey` and `CBAdvertisementDataServiceUUIDsKey`; every other key
    is documented as ignored, so writing our own CoreBluetooth bindings would change nothing.
    Linux/BlueZ *can* (`org.bluez.LEAdvertisement1` exposes `ManufacturerData`/`ServiceData`),
    so it is implementable as a Linux-only path if someone wants it.

Twelve protocols left `Incomplete` in August 2026 — `amqp`, `bgp`, `kafka`, `nfc`, `openvpn`,
`turn`, `usb/serial`, `usb/smartcard`, `vnc`, `webdav`, `webrtc`, `zookeeper`. Each was verified
against a **real client** where one exists (`lapin`, `reqwest_dav`, `zookeeper-async`, `webrtc-rs`
as a peer, a real `openvpn` 2.7.4 binary, two UDP sockets proving TURN relays a payload), and
against an **independent codec or RFC-derived literal bytes** where none does (BGP via
`netgauze`, Kafka via `kafka-protocol`'s client-side codecs). Each `metadata()` note says which,
and what is still untested.

Derive these rather than trusting the counts, which drift. **Match the fully-qualified form
too** — roughly half the protocols write `crate::protocol::metadata::DevelopmentState::X`, and a
pattern anchored on the bare `DevelopmentState::` silently reports them as declaring nothing.
An earlier version of this section claimed four USB protocols declared no state for exactly that
reason; all four declare one:

```bash
python3 - <<'EOF'
import re, pathlib
from collections import Counter
rows=[]
for f in sorted(list(pathlib.Path('src/server').glob('*/actions.rs'))
              + list(pathlib.Path('src/server').glob('*/*/actions.rs'))):
    m=re.search(r'\.state\(\s*(?:crate::protocol::metadata::)?DevelopmentState::([A-Za-z]+)',
                f.read_text(errors='ignore'))
    rows.append((m.group(1) if m else 'NONE', str(f)))
print(Counter(r[0] for r in rows))
EOF
```

Treat `Experimental` as "compiles and has a test", not "works". Before assuming a protocol
behaves, check what it actually offers the model — and check it two ways, because one grep
lies in both directions:

```bash
grep -A3 "fn get_sync_actions" src/server/<p>/actions.rs | grep -c 'vec!\[\]'  # hollow?
grep -cE "DnsProtocol|BluetoothBle::" src/server/<p>/actions.rs                # or delegating?
grep -c "\.with_actions(" src/server/<p>/actions.rs                            # reachable at all?
```

An empty `get_sync_actions()` is fine if the protocol **delegates** — `doh` and `dot` forward
`DnsProtocol`'s set verbatim, so the model sees the full DNS vocabulary. It is a trap if it
does not. And actions can be declared yet unreachable: `call_llm` builds the model's tool list
from `event.event_type.actions`, so a protocol whose event types never call `.with_actions(...)`
leaves the model unable to answer at all. That was found in 17 protocols, is now fixed
everywhere, and is guarded two ways: `EventType::with_no_actions()` marks the deliberate case,
anything else logs at ERROR and falls back, and `tests/event_action_declarations_test.rs` fails
the build on any new occurrence across every registered protocol.

**A fourth variant, still open: an event can be declared and never emitted.** Declaring actions
on it then buys nothing, because it never fires. The USB family is the live case — only
`*_attached` is ever raised for `usb-mouse`/`usb-keyboard`/`usb-msc` (one `call_llm_on_attach`
per `mod.rs`), and `usb-serial` raises nothing at all, so `usb_*_detached`, `usb_msc_read`,
`usb_msc_write`, `usb_keyboard_led_status` and all three `usb_serial_*` events are advertised to
the model and cannot fire. Check the emit side, not just the declaration:

```bash
grep -rn "_EVENT" src/server/<p>/mod.rs   # which events does the server actually raise?
```

The whole `bluetooth_ble_*` family was a third variant: `BluetoothBle::spawn_with_llm_actions`
hardcodes `BluetoothBleProtocol` when it calls `call_llm`, so all sixteen profiles' own actions
and events were unreachable regardless of what they declared. Fifteen now delegate explicitly;
`bluetooth_ble_beacon` is `Incomplete`, because the underlying crate cannot set an advertising
payload and a beacon is nothing else.

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
TCP had a bug of exactly this shape and it is worth knowing as the reference case (fixed in
`d70bb5b5`): `send_tcp_data` was documented in three places as accepting "text or hex-encoded
binary", but its executor did `data.as_bytes()` and never decoded hex, so a model following the
documentation put literal ASCII on the wire. Inbound data *was* hex-encoded when non-printable,
making the round-trip asymmetric — an echo server could not echo. The fix was an explicit
`encoding` field (`"utf8"` default, `"hex"`) on both directions rather than sniffing, because
`"48656c6c6f"` is simultaneously valid text and valid hex and only the sender knows which it
means. When you touch a protocol, verify its documented encoding matches its executor.

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
`server_startup.rs` before spawn, which now gates on `PrivilegeRequirement::is_met_by()` alone
and detects capabilities by actually probing (a `SOCK_RAW` socket, `/dev/bpf*`, `geteuid()`)
rather than inferring. Both were broken until this pass — the probe used `pcap::Device::list()`,
which any unprivileged user can call, so the check never fired for anything.

Declare `privilege_requirement` on any new protocol that needs raw sockets, a TUN device, or a
port below 1024. Two failure modes to avoid:

- **A `PrivilegedPort` above 1023 can never fire.** `svn` declared `PrivilegedPort(3690)`, which
  read as protection and was dead code. Declare `None` if the default port is unprivileged.
- **Don't claim more than you need.** `ospf` declared `Root` when it wants `CAP_NET_RAW`, which
  would refuse to start on a capability-only process that could in fact run it.

There is no variant for *device* access — Bluetooth adapters, USB, NFC readers. Those seventeen
protocols sit at `None` because every other option would be a lie; see IMPROVEMENTS item 60.

Startup must report failure. `spawn()` has to await readiness and return `Err` so
`server_startup` sets `ServerStatus::Error`. ARP, DataLink and ICMP used fire-and-forget
`spawn_blocking` and sat in `Running` having captured nothing — a server that lies about being
up is worse than one that refuses to start.

`get_dependencies()` / `ProtocolDependency` (`src/protocol/dependencies.rs`) is plumbed —
`get_excluded_protocols()` is called from the event handler and the TUI — but **no protocol
overrides `get_dependencies()`**, so the exclusion map is always empty and the mechanism does
nothing. Adopting it is cheap: declare dependencies and the existing plumbing starts excluding
unusable protocols with install hints.

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
`get_startup_parameters()`. `StartupParams::new` and all `get_*` accessors return
`Result<_, StartupParamError>` (`src/protocol/spawn_context.rs`), and params are validated
*before* `add_server`, so an undeclared key or wrong-typed value produces a clean error naming
the key and listing the allowed ones, and leaves no half-registered server behind. They used to
panic, which over MCP killed the per-request task before it could reply — the caller hung with
no error and the server stuck in `Starting`. Propagate the error with `?`; never `unwrap()` it.

Two related traps when declaring parameters: a parameter that is declared but never read is
dead weight the model will try to use (nine were found in the cloud protocols alone), and a
parameter read but never declared is rejected at startup. Both are worth a grep when you touch
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

This policy is currently violated by 9 files in `src/` (`llm/config`, `llm/reference_parser`,
`llm/hybrid_manager`, `llm/embedded_inference`, `protocol/event_logger`, `protocol/log_template`,
`system_stats`, `server/proxy/cert_cache`, `server/bluetooth_ble/mod`). Migrate them if you are
working nearby; do not add more. Current list:

```bash
grep -rln "#\[cfg(test)\]" src/ --include='*.rs'
```

### The mod.rs footgun (CRITICAL)

`tests/server.rs` and `tests/client.rs` only compile submodules explicitly declared in
`tests/server/mod.rs` / `tests/client/mod.rs`. **A test directory that exists on disk but is
not declared is silently never compiled and never run — no error, no warning.**

This was the single largest hole in the suite — **15 of 116 server test dirs and 61 of 83
client test dirs were orphaned**, including complete, correctly-gated E2E suites for `arp`,
`whois`, `bitcoin`, `igmp`, `tls`, `sip`, and every USB protocol. All 76 are now declared, and
the `orphaned-tests` job in `.github/workflows/ci.yml` fails the build if it happens again.
Verify locally with:

```bash
comm -23 <(ls -d tests/server/*/ | sed 's|tests/server/||;s|/||' | sort) \
         <(grep -oE "pub mod [a-z0-9_]+" tests/server/mod.rs | awk '{print $3}' | sort)
```

Note `$3`, not `$2`: `grep -oE "pub mod <name>"` yields three fields and `$2` is the literal
word `mod`, so the version this file carried until now reported every directory as orphaned.

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

Two workflows. `release.yml` triggers on `v*` tags and manual dispatch, and runs `cargo build`
for the `dist*` feature sets across 6 targets. `ci.yml` is the PR/push-to-master gate, added
after a long period when no CI job ran `cargo test` at all:

| Job | Blocking | What it does |
|---|---|---|
| `lint` | yes | `cargo fmt --check`; `clippy -D correctness -D suspicious`. A full default clippy runs advisory-only — ~50 style/complexity warnings predate the gate |
| `test` | yes | `cargo test` on `tcp,http,dns,udp,redis,mcp-stdio` |
| `single-feature` | yes | `cargo check --tests` on 14 protocol features **one at a time** — catches a feature whose deps are under-declared, which no multi-feature build can |
| `orphaned-tests` | yes | Fails if a test dir on disk is undeclared in `mod.rs` (see the footgun above) |

The gate is deliberately not `--all-features`: that needs system libraries the runner does not
install (`protoc`, `libpcap`, `dbus`, `libusb`, `pcsclite`). So **the CI feature set covers 6 of
116 protocols** — a green PR says nothing about the other 110. Run the relevant tests yourself
before claiming done.

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

- **Never `git add -A`, `git add .`, `git add -u`, or `git commit -a`.** Stage explicit paths
  only — `git add <path> && git commit -m …`, listing every file. This is not style: a broad
  `git add` sweeps up whatever other agents have half-written, and a half-written change
  committed without its other half breaks `master` for everyone.

  It has happened three times. Twice it broke the build: once landing a caller without its
  callee (`CertificateCache::new` gaining a third argument in `proxy/mod.rs` while
  `proxy/cert_cache.rs` stayed uncommitted), once landing 24 import swaps without the module
  they imported. The third time, a `git add -u` intended for a documentation cleanup swept
  fourteen source files from five different agents into a commit titled "docs: remove 46
  one-off session and status reports" — it happened to compile, but the history now
  misattributes that work.

  **`git add -u` counts.** It stages every tracked modification in the tree, which during
  parallel work is everyone's. The `-u` flag reads as narrower than `-A` and is not.

- **Staging explicit paths is NOT enough — pass the paths to `git commit` too.** This is the
  fourth incident and the subtlest: `git add <paths> && git commit -m ...` stages what you
  listed and then commits **the whole index**, including anything another agent staged and had
  not yet committed. In August 2026 that swept a *deletion* of `src/server/usb/smartcard/crypto.rs`
  — staged by an agent mid-edit — into an unrelated BGP-client commit, leaving HEAD referencing
  a module whose file was gone. It took a follow-up revert to repair.

  Use the pathspec form, which commits only those paths regardless of what else is staged:

  ```bash
  git add <paths> && git commit <paths> -m "..."     # or: git commit -- <paths>
  ```

  And check before you commit, because the index is shared state:

  ```bash
  git diff --cached --name-only     # must list only your files
  ```
- **Shared files** (`Cargo.toml`, both registries, `server/mod.rs`, `client/mod.rs`, both test
  `mod.rs` files, `state/server.rs`): use `Edit`, add incrementally, never overwrite wholesale.
- **Pause and report** if you hit an error in code you did not modify. It is almost always
  another agent mid-edit; retry rather than "fixing" their file.
- **Verify HEAD, not the working tree.** During parallel work the working tree is routinely
  mid-edit and its failures belong to nobody. Check the committed state in a throwaway
  worktree: `git worktree add --detach <tmp> HEAD && cargo check --all-features` with its own
  `CARGO_TARGET_DIR`. Remove the worktree afterwards.
- `--ollama-lock` serializes LLM API access (default in tests). Concurrent `git` work should
  use worktrees.
- Never `pkill cargo`; use `./cargo-isolated-kill.sh`.
- The user runs `netget --mcp` interactively. **Never kill netget processes.**

## Known systemic issues

Read before assuming a subsystem is sound:

- **Fail-open defaults are the most dangerous pattern in this codebase.** When the LLM returns
  nothing usable, a protocol must not fall through to a permissive default. OAuth2 did: no
  action meant a hardcoded authorization code, a hardcoded access token, and introspection
  answering `{"active": true}` for *every* bearer token — so an LLM outage silently issued
  credentials, and a model's explicit denial was indistinguishable from silence and became an
  approval. Its own CLAUDE.md documented this as a feature ("ensures the server always responds
  correctly"). Default to refusal, and make the model's rejection path structurally distinct
  from its no-answer path.
- Byte-index string truncation (`&s[..N]` guarded only by `s.len() > N`) panicked on multi-byte
  UTF-8 at the cut point, on LLM output and event descriptions. Fixed in `b9aa1058` —
  `src/utils/truncate.rs` has char-boundary helpers; use `truncate_for_log` rather than slicing.
  `src/protocol/log_template.rs` was the important one (`c1515188`): being shared
  infrastructure, it defeated protocols' own local fixes.
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
