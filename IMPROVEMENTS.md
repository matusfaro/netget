# NetGet Improvement Backlog

Living backlog of known defects and hardening work, ordered by impact. Not a session
report — delete entries as they are fixed, add as they are found. Every entry cites a
file:line so it can be verified independently.

Findings marked **[verified]** were reproduced against a binary built from HEAD.
Findings marked **[static]** come from code reading only.

## Fixed

Items below are resolved; kept here with their commit so the reasoning stays findable.
Delete an entry once its context is no longer useful.

| Item | Commit | What landed |
|---|---|---|
| 2 — TCP hex never decoded | `d70bb5b5` | Explicit `encoding` field on `send_tcp_data`/`send_to_connection`, defaulting to `utf8`; inbound events now carry `encoding` too, so echo is symmetric |
| 3 — UTF-8 truncation panics | `b9aa1058` | `src/utils/truncate.rs` with char-boundary helpers; 24 sites across 8 files; model-facing cuts now carry a truncation notice |
| 4 — feedback loop always failed | `b7c9a204` | Advertised and validated action lists are now the same filtered list |
| 6 — no CI | `45b8bde4` | PR/master workflow: blocking clippy correctness+suspicious, tests on a feature subset, and an orphaned-test-dir gate |
| 24 — scripts parked worker threads | `efd990e5`, `c3d92a16` | `tokio::process`-based async executor; both call sites switched |
| 25 — unbounded stdin deadlock | `efd990e5` | stdin write, stdout/stderr drain and `wait()` joined under one timeout; child reaped on timeout |
| 26 — script trust boundary undocumented | `65ed6bcf` | `src/scripting/CLAUDE.md` leads with the arbitrary-code-execution boundary |
| Scheduled tasks could never act | `2789cb40` | Same empty-action-list bug as item 4, in the scheduled-task path; found while fixing item 4 |
| Runtime prompt was 81% irrelevant | `73d334c8` | Dropped the handler-configuration tutorial from the per-event template: system prompt 11853 → 2219 chars, request 16377 → 8230 bytes |
| 11 — MCP docs described the wrong API | `40907b06` | New `src/mcp_stdio/docs.rs` renders MCP-shaped docs (event field names, action schemas, startup params, privilege); `open_server`/`open_client`/`base_stack` no longer appear, asserted by a test |
| 12 — no MCP client surface | `f1f25528` | `start_client`/`stop_client`/`list_clients`/`client_status`; client half of `list_protocols` now carries maturity and description |
| 13 — `send_first` ignored | `de5d9e19` | Folded into `startup_params` for the 8 protocols that declare it; warns rather than silently ignoring elsewhere. `interface`, `mac_address`, `initial_memory`, `feedback_instructions`, `scheduled_tasks` also un-hardcoded |
| 19 — ARP/ICMP absent from releases | `748fccca` | `arp` added to `dist-darwin`, `icmp` to `dist`; ICMP deliberately excluded from Windows, where `pnet` unconditionally links WinPcap |
| — registry couldn't say "compiled out" | `d58854ca` | `resolve()` on both registries distinguishes not-compiled from unknown, with a did-you-mean suggestion (adoption pending, item 36) |
| — unbounded `netget.log` | `c14c8b71`, `e3617126` | Size-based rotation (50 MiB × 5 generations ≈ 300 MiB ceiling), wired into `init_logging`; the repo's log had reached 481 MB in a day |
| — clients unreachable by lowercase name | `f1f25528` | `CLIENT_REGISTRY` is keyed by each protocol's own casing and lookup was exact-match, so `protocol: "tcp"` failed. Also, `client_startup.rs` dropped its status receiver, silently discarding every client status message |
| — VPN family misrepresented | `10f1e2b4`, `2bce6e64`, `f013c510` | OpenVPN demoted to `Incomplete` (identical keys for all peers, no TLS handshake) plus a TUN deadlock fix; WireGuard's advertised LLM control did not exist and now does; IPSec now actually calls the LLM |
| — DNS answers rejected by real resolvers | `6a384617` | Responses carried no question section, so glibc/systemd-resolved/`dig` discarded them. Verified after the fix: `dig @127.0.0.1 example.com A` returns NOERROR with the question echoed and the transaction id matched |
| — remote unauthenticated panic in DHCP/BOOTP | udpinfra pass, `b06165fc` | dhcproto never validates `hlen` and `Message::chaddr()` slices a `[u8; 16]` with it, so **one datagram declaring `hlen > 16` panicked the socket task** — server dead, status still `Running`. Guarded on the server, and on the client's receive path where the same unguarded call let a malicious DHCP server kill the client |
| — NTP never echoed origin timestamps | udpinfra pass | Beta was false: real clients rejected every reply. The code mutated `execution_result.raw_actions` *after* `call_llm` had already executed the actions and built the packet, so the injection never touched a byte. Fixed with a per-datagram protocol instance; verified with a raw client |
| — DHCP correlation race across clients | udpinfra pass | `xid`/`chaddr` lived in one `Arc<Mutex<Option<_>>>` shared by the whole server, written just before a multi-second LLM call, so two overlapping clients echoed each other's transaction id |
| — SNMP BER lengths wrapped at 256 | udpinfra pass | Every length used `0x81 + len as u8`, so any response ≥256 bytes (a long sysDescr, a multi-binding reply) produced a corrupt packet |
| — Syslog filters could never match | udpinfra pass | `facility`/`severity` were emitted with `{:?}` as `log_kern`/`sev_err` while every doc and example prompt says `kern`/`err`. Numeric codes added alongside, which is how the docs describe filtering |
| — HTTP family defects | `3c414406`, `f2d4cf3e`, `e9eec1cb`, `e32cf485` | 204 responses were served as empty 200; a model status of 999 or a CRLF header panicked the connection task; HTTP/2's `request_filter` was accepted and silently ignored because the hyper path is dead code; HTTP/3 documented as the QUIC transport it actually is |

Verified together end to end after landing: a Python script handler returning
`{"data":"48454c4c4f","encoding":"hex"}` puts `HELLO` on the wire, with no LLM call.

---

## P0 — Correctness bugs reachable from untrusted input

### 1. `StartupParams` panics on malformed input; MCP callers hang forever **[verified]**

`src/protocol/spawn_context.rs:42` and every `get_*` accessor (`:63-311`) call `panic!` on an
undeclared key, a missing required key, or a wrong-typed value. The JSON originates from the
LLM (`open_server`) or an MCP client (`start_server`), so this is remotely triggerable.

Reproduced: an MCP `tools/call` with `startup_params: {"undeclared_xyz": 1}` panics the
per-request task, so **no JSON-RPC response is ever sent** — the caller blocks until its own
timeout. The `ServerInstance` was already registered (`src/cli/server_startup.rs:383`) before
params are built (`:457`), so it is left permanently in `ServerStatus::Starting`.

Fix: convert `StartupParams::new` and all accessors to return `Result`; surface the error
through `start_server_from_action`'s existing `Err` path so status becomes `Error` and MCP
returns a tool error. Validate before `add_server`.

### 2. `send_tcp_data` never decodes hex, contradicting its own documentation **[verified]**

`src/server/tcp/actions.rs:242,276` do `data.as_bytes()`. The field is documented as
"text string or hex-encoded for binary data" at `:355,359`, the docs served to the LLM show
`{"data": "48656c6c6f"}` as a `tcp_data_received` response example, and
`src/server/tcp/CLAUDE.md` states `"48656c6c6f"` sends `"Hello"`.

Reproduced: a static handler returning `data: "48656c6c6f"` put `34 38 36 35 36 63 36 63 36 66`
on the wire — the literal ASCII, not `48 65 6c 6c 6f`. Inbound data *is* hex-encoded when
non-printable (`src/server/tcp/mod.rs:154,301,430,487`), so the round-trip is asymmetric: the
model is shown hex, told to answer in hex, and its hex is sent verbatim. Any binary protocol
built on raw TCP is silently corrupt.

Fix: pick one contract and make code and docs agree. Recommended: an explicit
`encoding: "utf8" | "hex"` field defaulting to `utf8`, since heuristic sniffing cannot
distinguish the string `"48656c6c6f"` from the bytes it encodes. Audit the other protocols
that hex-encode inbound but never decode outbound: `socket_file`, `tls`, `cassandra`,
`http3`, `kafka`, `snmp`, `tor_relay`.

### 3. UTF-8 boundary panics on LLM-controlled strings **[static]**

The pattern `if s.len() > N { &s[..N] }` checks *byte* length and panics when the cut lands
inside a multi-byte character. Occurrences include `src/llm/conversation.rs:604,1213,1220,1361,1409,1419,1446,1504`,
`src/llm/action_helper.rs:153,464`, `src/llm/event_handler_executor.rs:204`,
`src/llm/actions/summary.rs` (9 sites), `src/llm/ollama_client.rs:764,1084`,
`src/mcp_stdio/tools.rs:783`, `src/cli/rolling_tui.rs:814`.

These run on the action-summary/logging path for essentially every LLM response, and the
strings are model output and event descriptions — any emoji, curly quote, or non-English text
near the offset crashes the handling task. `action_helper.rs:153` uses a 27-byte window.

Fix: one shared `truncate_chars(s, n)` helper using `char_indices()`, applied everywhere.

### 4. Feedback loop cannot succeed **[static]**

`src/llm/action_helper.rs:684-701` builds a prompt telling the model which actions are
available, then `:752` defines `let feedback_actions: Vec<ActionDefinition> = Vec::new();`
and passes that empty vector to both `with_native_tools` (`:761`) and
`generate_with_tools_and_retry` (`:781`). `valid_action_names` is derived from it
(`src/llm/conversation.rs:504-511`), so every action the model returns is rejected as unknown,
retried twice, then `bail!`s. The whole `feedback_instructions` feature can only no-op or fail.

Fix: pass the same action list used to build the prompt.

---

## P1 — Silent failures and lost coverage

### 5. 76 test directories are never compiled **[verified]**

`tests/server.rs` / `tests/client.rs` only compile modules declared in the respective
`mod.rs`. Directories present on disk but undeclared are silently skipped — no error.

Measured: **15 of 116** server dirs orphaned (`arp bitcoin igmp nfc openid sip svn tls
usb_fido2 usb_keyboard usb_mouse usb_msc usb_serial usb_smartcard whois`) and **61 of 83**
client dirs. These are real, feature-gated suites, not stubs.

Fix: add the missing `pub mod` lines (31 client dirs are ready as-is; 30 need a `mod.rs`
first), then add a CI check that the two sets match.

### 6. No CI runs tests **[verified]**

`.github/workflows/release.yml` is the only workflow; it triggers on `v*` tags and manual
dispatch and runs `cargo build` for the `dist*` sets. `cargo test` appears nowhere. The
1,038-test suite is developer-run only, with no PR gate and no lint job.

Fix: a PR workflow running `cargo clippy` plus a representative feature subset
(`tcp,http,dns,udp,redis`) with `--test-threads=100`, and the orphan check from item 5.

### 7. Unknown action names fail silently at runtime **[verified]**

A static `event_handler` naming a nonexistent action is accepted at startup and does nothing
when the event fires: the peer gets no response, no error reaches the MCP caller, and
`list_access_logs` records the action name as though it executed.

Fix: validate handler action names against the protocol's action catalog at parse time in
`EventHandler::parse_event_handlers` (`src/events/handler.rs:1682`), and record execution
failures in the access log.

### 8. Raw-socket protocols report `Running` when capture fails **[static]**

`arp`, `datalink`, `icmp`, `isis` start their capture loop in a fire-and-forget
`spawn_blocking`; a failure to open the pcap/raw handle never propagates, so the server shows
`Running` while doing nothing. Same class of problem as an accept-loop panic, which also
leaves status at `Running` with no supervision or restart.

Fix: have `spawn()` await a readiness signal before returning `Ok`, and set
`ServerStatus::Error` when the capture handle fails.

### 9. Scheduled tasks leak when servers stop via MCP **[static]**

`src/mcp_stdio/tools.rs:415,718` call `remove_server()` without `cleanup_server_tasks()`,
unlike the TUI paths (`src/cli/rolling_tui.rs:2421,2466`). Orphaned server- and
connection-scoped tasks keep firing every tick, each producing a failed LLM prompt. There is
also no reaper under `--mcp` (`cleanup_old_servers` is only wired into the TUI loop), so
LLM-initiated `CloseServer` leaves entries in `AppState` forever.

### 10. Client lifecycle is unfinished **[static]**

`ClientInstance.handle` (`src/state/client.rs:120`) is never populated — no
`register_client_task()` exists, and every client protocol discards its read-loop
`JoinHandle`. `remove_client()` therefore cannot stop a client's network activity. Per-connection
server tasks are likewise untracked, so `stop_server` does not cancel in-flight connections
(the listening socket *is* released correctly via `register_server_task`).

---

## P2 — LLM tooling quality

### 11. `get_protocol_docs` documents the wrong API to MCP callers **[verified]**

The MCP tool reuses the TUI LLM's `read_documentation` output
(`src/llm/actions/tools.rs:2260-2272`), which opens with guidance on choosing between
`open_server` and `open_client` and describes `base_stack` — none of which an MCP caller can
use. It explicitly recommends `open_client`, for which no MCP tool exists.

Fix: render MCP-shaped docs — `start_server` arguments, the protocol's event ids with field
names, its action names with parameter schemas, its startup parameters, and its privilege
requirement. This is the single highest-leverage change for making the MCP surface usable by
a calling model without trial and error.

### 12. No client control surface over MCP **[verified]**

`list_protocols` advertises ~75 client protocols and the docs tell the caller to use them,
but no MCP tool starts, lists, queries, or stops a client. The client list also carries no
maturity or description, unlike the server list.

### 13. `send_first` is silently ignored everywhere **[verified]**

`start_server_from_action` takes it as `_send_first` (`src/cli/server_startup.rs:217`) and
never uses it. TCP's banner feature is unreachable through the action API, and MCP hardcodes
`false` anyway (`src/mcp_stdio/tools.rs:368`). Either wire it through to
`ProtocolConnectionInfo`/spawn or delete the parameter and the documentation for it.

### 14. Prompt tells the model something false about re-invocation **[static]**

`ActionDefinition::is_tool()` (`src/llm/actions/mod.rs:252-262`) omits `read_documentation`,
`list_tasks`, `execute_sql`, `list_databases`. `build_actions_section_public`
(`src/llm/prompt.rs:139`) partitions on it, so `read_documentation` — the primary discovery
tool — is rendered under a heading stating "you will not be invoked again", while
`ToolAction::is_tool_action()` (`src/llm/actions/tools.rs:184-204`) does re-invoke it.

Fix: single source of truth for tool classification.

### 15. Unbounded prompt growth **[static]**

`ConversationHandler.messages` (`src/llm/conversation.rs:53`) is never trimmed across up to
5 tool iterations × 5 retries, and failed attempts plus their correction messages persist for
the rest of the conversation. `ConversationState`'s 8000-char window is a separate structure
applied only to cross-call history, never to what is actually sent. Server `memory` is an
unbounded `String` injected into every prompt (`src/llm/actions/executor.rs:174-194`). The
full base-stack catalog is re-sent on every call once any server is running
(`src/llm/prompt.rs:706-712`).

### 16. No circuit breaker on the LLM backend **[static]**

`is_available()` exists (`src/llm/ollama_client.rs:1315`) but nothing calls it. With Ollama
down, every request independently waits the full 120s timeout, and with `max_concurrent: 1`
(`src/llm/rate_limiter.rs:48`) N queued connections serialize into N×120s. On failure most
protocols reset to Idle and write nothing (`src/server/tcp/mod.rs:580-589`), so peers hang
until their own timeout rather than getting a protocol-appropriate error.

### 17. `git` and `mercurial` bypass the LLM infrastructure **[static]**

Both call `llm_client.generate_with_retry` directly (`src/server/git/mod.rs:335-362`) instead
of `call_llm`. They therefore skip the rate limiter, the unknown/malformed-action repair
loops, native tool schemas, and `try_execute_event_handler` — meaning script and static
handlers are silently ignored for these two protocols and every request hits the LLM.

---

## P3 — Consistency and hygiene

### 18. Privilege gate ANDs in the wrong capability **[verified]**

`src/cli/server_startup.rs:348` gates on `!system_caps.can_bind_privileged_ports` even when
the requirement is `RawSockets` or `Root`. A process with `CAP_NET_BIND_SERVICE` but not
`CAP_NET_RAW` skips the check and fails later with an opaque `EPERM`. Use
`PrivilegeRequirement::is_met_by()` alone — it already handles each variant.

Also: only ~23 of 116 protocols declare a requirement. `smtp`, `ldap`, `imap`, `pop3`, `ipp`,
`syslog` (privileged default ports) and `igmp` declare none. And the actionable remediation
text ("run as root", "use a port ≥1024") lives only in `start_server_by_id:82-100`, which MCP
never calls — the MCP path returns the bare description (`:348-355`).

### 19. ARP and ICMP are absent from every shipped binary **[verified]**

`dist` excludes `datalink`, `arp`, `isis` (libpcap) and `icmp`. `dist-darwin` adds back
`datalink` and `isis` — because macOS ships libpcap — but **not `arp`**, which needs the same
library, nor `icmp`, which needs no system library at all (`pnet` + `socket2` only). Both
look like oversights. Users of the released binary get `Unknown protocol: arp`, with no
indication the protocol exists but was compiled out.

Fix: add `arp` to `dist-darwin` and `icmp` to `dist`; make the registry distinguish "no such
protocol" from "compiled out of this build" in its error.

### 20. `ProtocolDependency` is fully dead code **[static]**

`src/protocol/dependencies.rs` plus `get_dependencies()`/`is_protocol_available()`/
`get_excluded_protocols()` (`src/protocol/server_registry.rs:754-806`) are well-designed and
produce good install hints, but no protocol overrides `get_dependencies()` and nothing calls
the checks before `spawn()`. Adopt or delete.

### 21. Nine protocols expose zero actions **[static]**

`amqp`, `mqtt`, `doh`, `dot`, and five `bluetooth_ble_*` profiles return empty vectors from
`get_sync_actions()`/`get_event_types()` — `amqp` and `mqtt` say `// Placeholder` in source.
`bluetooth_ble_proximity` ships `get_startup_examples()` referencing an event and action it
does not define. They are registered and offered to the LLM as if functional.

Fix: mark them `Incomplete` so `is_available_to_llm()` hides them, or implement them.

### 22. Documentation drift **[verified]**

- **Correction to an earlier claim in this file**: it previously said 27 server protocols had
  no E2E test. That was wrong — it counted only files named `e2e_test.rs`. Every directory
  under `tests/server/` has at least one test file; 25 protocols simply name theirs `test.rs`,
  including `tcp`, `http`, `dns`, `ssh`, `snmp`, `ntp`, `dhcp`, `udp`, `mysql`, `postgresql`,
  `smtp`, `imap` and `nfs`, and those are correctly declared in both their own `mod.rs` and
  `tests/server/mod.rs`. The coverage problem is not absence of tests; it is item 5 (15 server
  and 61 client directories orphaned from the parent `mod.rs`) plus tests that are wired and
  failing — all 4 of `tests/server/dns/test.rs` fail at HEAD, independent of any change made
  in this pass. Verify with `ls tests/server/*/` rather than grepping for `e2e_test.rs`.
- `openvpn` declares `DevelopmentState::Stable` and describes itself as "production-ready",
  but its key material is HKDF-derived from hardcoded constants
  (`src/server/openvpn/mod.rs:441-461`) — every peer gets identical keys — and its control
  channel handlers are no-op stubs. Data-plane encryption is real; the handshake is not.
  It should not be `Stable`.
- `ipsec` documents LLM-driven accept/reject decisions but never calls `call_llm`.
- `isis` fabricates a MAC on non-Linux (`src/server/isis/mod.rs:638-663`); the OSPF and IGMP
  E2E tests run over plain UDP and never exercise the raw-socket path they claim to test.
- 8 files in `src/` contain ~23 unit tests, violating the project's own "no tests in `src/`"
  policy.
- ~50 of 63 root markdown files are one-off session reports last touched in 2025; two files
  the old CLAUDE.md referenced (`TEST_INFRASTRUCTURE_FIXES.md`, `TEST_STATUS_REPORT.md`)
  do not exist.

### 23. Architectural ceilings **[static]**

- `AppState` is a single `RwLock` over servers, clients, connections, tasks, LLM config, and
  access logs (`src/state/app_state.rs:261`). No deadlock (no `.await` under the guard), but
  every per-connection stat update from every server serializes through one writer.
- All 27 `mpsc` channels are unbounded; there is no backpressure anywhere. A stalled consumer
  grows memory without bound.
- `state/machine.rs` defines a generic `StateMachine<S>` that nothing instantiates; every
  protocol hand-rolls Idle/Processing/Accumulating, so a fix in one does not propagate.
- `all-protocols` pulls in `rdkafka` to gate the Kafka *client*, though the Kafka server
  feature comment says it was removed for causing malloc crashes (`Cargo.toml:239,514`).

---

## P1b — Scripting subsystem (the recommended deterministic path)

The MCP `start_server` tool description actively steers callers toward `event_handlers` with
script handlers over the LLM path. This code is therefore on the hot path, not a corner case.

### 24. Script execution parks a tokio worker thread for up to 30s **[verified]**

`src/scripting/executor.rs::execute_script` is synchronous and is called directly from
`async fn execute_script_handler` (`src/llm/event_handler_executor.rs:194`) with no
`spawn_blocking`. `wait_with_timeout` (`:310-341`) polls with
`std::thread::sleep(100ms)` until the child exits or 30s elapses.

`src/bin/netget.rs` uses bare `#[tokio::main]`, so worker_threads equals the CPU count. Each
in-flight script parks one worker for its full duration; on an 8-core machine, 8 concurrent
script-handled requests stall every protocol server, the TUI, and the MCP stdio loop.

Fix: async execution via `tokio::process::Command` + `tokio::time::timeout`.

### 25. Unbounded blocking write to child stdin has no timeout **[static]**

`execute_with_command` (`src/scripting/executor.rs:271-276`) does `write_all` of the entire
event JSON to the child's stdin *before* reading stdout and *before* entering the timeout
loop. If the payload exceeds the pipe buffer (~64KB) while the child is blocked writing
stdout, both deadlock — and since the timeout only starts afterwards, that hang is unbounded.
Event payloads can be large (HTTP bodies, accumulated TCP buffers). Also, `child.kill()` at
`:332` is never awaited, so timed-out scripts can linger as zombies.

### 26. Script execution is unsandboxed and undocumented **[verified]**

`python3 -c <code>`, `node -e <code>`, and `go run` are spawned with the code passed straight
through (`src/scripting/executor.rs:108-135,205`). There is no sandbox, no allowlist, and no
privilege reduction — scripts run with the full privileges of the netget process.

That may be the right choice for a local tool, but it is currently undocumented, and it means
**the MCP `start_server` tool is an arbitrary-code-execution surface**: any MCP client, or any
model driving one, can execute arbitrary code as the user via a script handler. Document the
trust boundary explicitly; decide separately whether a sandbox is wanted.

---

## P2b — Client half is substantially less finished than the server half

### 27. A client can only be started by an LLM call **[verified]**

There is no MCP tool for clients, no CLI flag, and no deterministic entry point —
`open_client` exists solely as an LLM action (`src/events/handler.rs`,
`src/cli/non_interactive.rs:380`). Starting a client always costs a model round-trip and
cannot be scripted or tested deterministically.

Combined with items 10 (no `JoinHandle`, so clients cannot be stopped) and 5 (61 of 83 client
test directories never compiled), the client half needs a deliberate completion pass: an MCP
`start_client`/`stop_client`/`list_clients` surface, task-handle tracking, and its test suite
turned on.

### 28. `--api-key` on the command line is world-readable **[static]**

`src/cli/args.rs` accepts `--api-key <KEY>`, which places the secret in the process table for
every local user to read via `ps`. The env-var path (`NETGET_API_KEY` / `OPENAI_API_KEY`) is
already supported and should be the documented default; consider warning when the flag form
is used.

### 29. `REQUIRE_DOCS_FOR_OPEN_ACTIONS` is dead configuration **[verified]**

`src/events/handler.rs:20` is a hardcoded `const … = false`, so the "model must read the docs
before it may open a server" gate never engages, while `is_server_docs_read()` /
`is_client_docs_read()` state is still tracked to feed it. Either make it a real runtime
setting or remove it and the state it depends on.

---

## P2c — Found during the fix pass

### 30. `register_server_task()` stores only one handle per server **[static]**

`src/state/app_state.rs:487-493` overwrites any previously registered `JoinHandle`, so a
protocol with two long-lived loops (OpenVPN's UDP listener plus its TUN reader) silently leaks
the first — and `stop_server` then only aborts the second. OpenVPN worked around it by joining
both loops under one task with `select!`. Either document the single-handle contract at the
call site or change the field to a `Vec<JoinHandle<()>>`.

### 31. Protocol async actions are structurally unreachable **[static]**

`src/llm/actions/executor.rs:100` dispatches async actions through `Server::execute_action()`,
which is invoked on a *stateless* protocol struct with no handle to the running server
instance. Any async action needing live state (`list_peers`, `get_server_info`, …) can only
return `NoAction`. Several protocols advertised such actions to the model; the VPN family's
dead advertisements were removed, but the general fix needs a server-instance registry reachable
from `src/llm/actions/protocol_trait.rs`.

### 32. Validator accepts a superset of what the prompt advertises **[static]**

Items 4 and the scheduled-task fix closed two cases where the advertised and validated action
lists diverged outright. A milder version remains: several call sites pass the *unfiltered*
list to the validator while the prompt builder applies `filter_actions_by_scripting_mode` to
what it renders, so e.g. `update_script` is accepted with scripting Off. Permissive rather
than broken, but it is the same drift. Clearest at `src/events/handler.rs:611-637` and
`src/llm/action_helper.rs:143-167`.

### 33. Development builds default to TRACE, which writes full payloads to disk **[verified]**

`src/cli/args.rs` defaults dev builds to `trace`; per CLAUDE.md, TRACE logs full payloads.
That is what produced a 481 MB `netget.log` in a day. Rotation now bounds total size
(`e3617126`), but `debug` would be a better default, with `--log-level trace` still available
when payload-level detail is genuinely wanted.

### 34. `src/server/vpn_util/` is dead code **[static]**

`TunManager` is declared at `src/server/mod.rs:511` and used by nothing — WireGuard uses
defguard, OpenVPN uses the `tun` crate directly. It keeps a `tokio_tun` dependency alive for
no consumer.

### 35. The `easy` layer is a parallel subsystem serving one protocol **[verified]**

`src/easy/` contains exactly one protocol (HTTP, 378 LOC) but carries its own trait, registry
(`src/protocol/easy_registry.rs`), startup path (`src/cli/easy_startup.rs`), prompt templates
(`prompts/easy_request/`), and snapshot tests. In `src/llm/action_helper.rs:361-394` it is
checked *before* `try_execute_event_handler` and returns early, so for an easy-managed server
the deterministic script/static path would be bypassed in favour of an LLM call. Not currently
reachable — `easy_startup.rs` accepts no `event_handlers` — but the ordering inverts the
project's stated preference and is a trap if easy servers ever gain handler support.

### 36. Registry `resolve()` needs adopting in the startup path **[static]**

`d58854ca` added `ServerRegistry::resolve()` / `ClientRegistry::resolve()`, which distinguish
"not compiled into this build" from "no such protocol" and offer a did-you-mean suggestion.
`src/cli/server_startup.rs` still uses the older `parse_from_str` + `get` pair at roughly
`:52` and `:227`, so callers keep getting the bare `Unknown protocol: arp`. Two-line adoption,
described in that commit.

### 37. Static handlers cannot interpolate event data **[verified]**

`execute_static_handler` (`src/llm/event_handler_executor.rs:243`) emits its configured actions
verbatim, with no substitution from the triggering event. That makes static mode unusable for
every request/response protocol needing a correlation id — DNS `query_id`, DHCP/BOOTP `xid`,
SNMP `request-id`, STUN/NTP transaction fields — because the reply cannot echo the client's
random value. The DNS family's own `StartupExamples` taught `"query_id": 0`, i.e. actively
documented a broken configuration, until `6a384617` replaced them.

Script handlers do not have this problem (they receive the event on stdin), so the workaround
is "use a script", but that means spawning an interpreter to copy one field. A small
templating substitution (e.g. `{{event.query_id}}`) in static actions would make the cheapest
deterministic path viable for the whole UDP family. Until then, say so plainly in the static
handler's documentation.

### 38. DNS responses omitted the question section **[fixed — `6a384617`, recorded as a lesson]**

RFC 1035 §4.1.2 requires a response to repeat the question, and glibc, systemd-resolved and
`dig` all discard responses whose question doesn't match. A protocol rated **Beta** — defined
in CLAUDE.md as "human reviewed, works with real clients" — therefore did not work with real
clients, and its tests passed because they used a lenient client. Worth treating as the
canonical argument for validating Beta claims against a real off-the-shelf client rather than
against our own test harness.

### 39. `open_server`'s documentation gate makes mocked tests fragile **[static]**

`src/events/handler.rs:844` forces a `DocumentationRequired` retry on first use of
`open_server`. Mock configurations that don't answer that retry never start their server, which
is why all 4 `tests/server/dns/test.rs` tests and 7 in `tests/examples/` fail at HEAD while
DoT/DoH/mDNS survive. Either the gate should be off by default (compare
`REQUIRE_DOCS_FOR_OPEN_ACTIONS` at `:20`, which is a hardcoded `false` — see item 29) or the
mock helper should answer it centrally so every protocol's tests don't have to.

### 40. Smaller protocol-level defects found in review **[static]**

- `src/server/svn/actions.rs:56` declares `PrivilegedPort(3690)`; 3690 > 1024, so the check can
  never fire. Any `PrivilegedPort` above 1023 is dead by construction — worth a debug assertion.
- `tests/server/dot/e2e_test.rs:132,155,172` hardcode `"query_id": 1` instead of using
  `respond_with_actions_from_event`, violating CLAUDE.md's dynamic-mock rule. They pass only
  because the raw TLS client never checks the id, so they prove nothing about correlation.
- No `ProtocolConnectionInfo` data is recorded for DNS, and DoT/DoH register no connections at
  all, so they are invisible to the connection list and to connection-scoped scheduled tasks.
- `'secure dns'` is claimed as a keyword by both DoT and DoH; the collision is warned about at
  startup and never resolved.

### 41. Action execution failures are swallowed **[verified]**

`src/llm/actions/executor.rs:114` drops a protocol action whose executor returns an error with
only a `warn!`. The peer then receives the protocol's default — an empty 200 for HTTP, nothing
at all for TCP — and neither the MCP caller nor the access log learns the action failed; the
log records it as though it ran. This is why the HTTP executor was deliberately made lenient
rather than strict, and it is the same root cause as item 7. Fixing it properly means
propagating action errors into the access log and the tool result.

### 42. `http3` is the QUIC transport, not HTTP/3 **[static]**

The server implements QUIC streams, not HTTP/3 semantics. Metadata and docs now say so
(`e32cf485`), but two consequences remain: `Cargo.toml:240` pulls `h3`/`h3-quinn` into the
`http3` feature although only `src/client/http3/` uses them, and NetGet's own HTTP/3 client
therefore cannot talk to NetGet's own HTTP/3 server. Decide whether to implement HTTP/3 over
the existing QUIC layer or rename the protocol to `quic`.

### 43. `Http2Server` is dead code **[static]**

`Http2Protocol::spawn()` calls `H2Server::spawn_with_push_support`, never the hyper-based
`Http2Server` still re-exported at `src/server/mod.rs:64`. That dead path is why HTTP/2's
`request_filter` was silently inert. Remove the re-export and the module.

### 44. HTTP connection statistics are never updated **[static]**

For all three HTTP protocols, `bytes_sent`, `bytes_received`, `packets_*`, `last_activity` and
`recent_requests` keep their initial values, so the TUI's per-connection counters stay at zero
and only connect/disconnect are visible. Needs accessors in `src/state/server.rs`.

### 45. The mock harness misroutes the pre-`open_server` documentation step **[verified]**

Reproduced at session-start commit `ea950dca` in a clean worktree, so this predates all of
today's work. The documentation step forced before `open_server` produces a prompt whose text
makes `tests/helpers/mock_ollama.rs` context extraction report `event_type: Some("http_request")`.
An `instruction contains` rule therefore misses, an `on_event` rule matches instead, and
`send_http_response` is returned to the *startup* call, which rejects it as an unknown action —
so no server ever starts. This breaks all 10 `tests/server/http/*` tests, all 4 DNS tests, and
7 in `tests/examples/`. It is the single highest-value test fix available: one change in the
mock helper or the docs-gate flow turns several protocol suites green at once. Closely related
to item 39.

### 46. Feature declarations are incomplete, and `--all-features` hides it **[verified — 2 found, both fixed]**

Two protocols did not build with the single-protocol command CLAUDE.md mandates
(`--no-default-features --features <protocol>`), while building fine under `--all-features`
because another enabled feature happened to supply what they needed through Cargo feature
unification:

- `openid = []` declared no dependencies although `src/server/openid/mod.rs` uses `urlencoding`
  at 8 sites — `E0433`, fixed in `e6353886`. Every other feature using that crate declares it.
- `usb-fido2` did not enable `ring/std`, without which `ring` does not implement
  `std::error::Error` for `Unspecified`/`KeyRejected`, so `?` into `anyhow` failed at 8 sites
  in `ctap2.rs`/`u2f.rs` — `E0277`, fixed in `b7b1d5ae`.

The class matters more than the two instances: `--all-features` is structurally incapable of
catching an under-declared feature, and it is the only build CI runs. A sweep of 30 other
protocol features found no further cases, so this is not endemic — but nothing prevents the
next one. Add a CI job that builds a rotating sample of individual protocol features, or all of
them on a schedule; it is the only thing that can catch this.

### 47. Several tests pass while the protocol is broken **[verified]**

`tests/server/ntp/test.rs:63-96` catches the rsntp client's failure, prints a note and asserts
nothing — so it passed for the entire life of the origin-timestamp bug, while every real NTP
client rejected the server's replies. `tests/server/dot/e2e_test.rs` has the same shape for
transaction ids (item 40).

This is the same lesson as item 38 stated twice: a mocked test that only checks our own
plumbing cannot substantiate a **Beta** rating, which this project defines as "works with real
clients". Suggested rule: no protocol may be rated Beta without at least one test that fails
when a real off-the-shelf client would reject the output — and that test must assert, not log.

### 48. Library panics reachable from the wire need auditing per dependency **[static]**

The dhcproto `chaddr()` panic was found in the DHCP server and then existed unfixed in the
DHCP client. The general shape — a parsing crate that trusts a length field the wire controls
— will recur. Worth a pass over the other binary-format decoders (`rasn-snmp`, `hickory-proto`,
`kafka-protocol`, `cassandra-protocol`, `bson`, `pgwire`, `opensrv-mysql`) asking specifically
which of their accessors panic on malformed input, since all of them run inside a socket task
where a panic silently kills the server while its status still reads `Running`.

---

## Suggested order

1. Items 1-4 — remotely-reachable panics and the broken feedback loop.
2. Items 5-6 — turn the dormant test suite on and gate it in CI; everything else is easier
   to land safely afterwards.
3. Items 11-12 — make the MCP surface self-describing and give it client parity.
4. Items 7-10, 18-19 — silent failures and the privilege/packaging gaps.
5. Items 13-17, 20-23 — LLM tooling quality and consistency.
