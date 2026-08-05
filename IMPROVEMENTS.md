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
| 5 — 76 orphaned test directories | `87e02967`, `84a4fb24`, `1e842bf0`, `9fd32787` | All 15 server and 61 client directories wired in; both orphan sets are now empty, so the CI gate added in `45b8bde4` passes |
| 45 — mock harness misrouted the docs step | `98c54d4e` | Context extraction classified the *startup* call as an `http_request` event, because the documentation retry embeds `### Event: <id>` headings. Now classified on the prompt template rather than wording. **`--test server` 5/24 → 18/24, `server::tcp` 0/10 → 10/10, `server::dns` 0/4 → 4/4, `--test examples` 13/34 → 34/34, `--test client` 5/9 → 12/13** |
| — tests phoning external endpoints | `da4c86f5` | The DoT and Git client tests connect to `dns.google:853`, `1.1.1.1:853` and `github.com`. Harmless while orphaned; wiring them in made the calls live, violating the localhost-only policy |
| 51 — remaining HTTP test failures | `2528629` | 7/10 → 10/10 in `tests/server/http/`; mocks keyed on `uri` where the event emits `path`, and a `write_file` action HTTP does not have, replaced with the real `append_to_log` |
| 1 — `StartupParams` panicked on model input | `a9f03fd6` | All 16 panic sites return `Result`; 163 call sites across 74 protocol modules converted. Params are now validated *before* `add_server`, so a rejected request leaves no orphan. Verified over MCP: an undeclared key went from **no JSON-RPC response at all** (caller hangs, server stuck in `Starting`) to a clean tool error naming the key and listing the allowed ones, with `list_servers` empty |
| 36 — registry `resolve()` unadopted | `a158da2f` | `start_server` now answers `Protocol 'ARP' exists but is not compiled into this build (rebuild with --features arp)` and `Unknown protocol: 'htpp'. Did you mean 'HTTP'?` |
| 18 (rest) — gate ANDed the wrong capability | `aa713be3` | Both sites (`:72`, `:417`) now test `PrivilegeRequirement::is_met_by()` alone; a failed `spawn()` also drops its registration instead of leaving a zombie `Error` row |
| — test targets broken under narrow features | `57a4c42f` | `server_stop_releases_port_test` (stale arity after `ec79eda5`) and `examples` (empty feature-gated vec, E0282) failed to *compile* with `--features telnet`, silently disabling every test in those targets |
| — proxy/tunneling family | `2cf3d4f2`, `ec4231ab`, `a3e101a9`, `a0d8a6cb`, `32a30f83` | Suite went **21 failures → 5**. Proxy's certificate cache returned a regenerated certificate paired with the *cached* key, so every repeat MITM connection to a host failed with `KeyMismatch`; `certificate_mode: "load_from_file"` read the operator's key, **ignored the certificate file entirely** and minted a different CA, so clients trusting the real one rejected everything while the config looked correct; absolute-form request targets were forwarded verbatim, which `curl --proxy` caught and our tests never did. SOCKS5 ended three paths with no reply at all. TLS had the `d70bb5b5` defect again — hex documented, never decoded. TURN demoted to `Incomplete`: it cannot relay a byte, as no relay socket is ever bound |
| 56 (part) — STUN's actions were invisible | `a0d8a6cb` | STUN's event never called `.with_actions(...)`, so every response was rejected as unknown; five E2E tests failed. Its shipped static example also used three nonexistent fields and a 6-byte transaction id |
| 7 + 41 — action failures were invisible | `5deee649`, `0069d90c` | Static handlers naming a nonexistent action are now rejected at `start_server` with the valid list; execution failures are recorded as `FAILED: <name>` in the access log with the error and original action, and logged at `error!`. Batch semantics are continue-and-report, not abort — aborting would suppress the valid actions after a bad one |
| 18 (part) — raw-socket gate never fired | `c9a65c80` | `has_raw_socket_capability()` used `pcap::Device::list()`, a `getifaddrs` wrapper that succeeds for any user, so it always returned true and the pre-flight never ran. Now probes a real `SOCK_RAW` socket plus `/dev/bpf*`. `is_running_as_root()` also fixed — it stat'd `/root` and compared `$USER`, so `sudo -E` and most containers fooled it |
| — raw-socket family started but did nothing | `1e977cbe`, `b58b5788`, `21b09483`, `4204bbbc` | ARP, DataLink and ICMP opened their privileged handle in a fire-and-forget `spawn_blocking` and returned `Ok` unconditionally, so an unprivileged start sat in `Running` capturing nothing forever. Now report readiness. Also fixed IGMP's `from_raw_fd`-before-check unsoundness, a checksum underflow, a 100%-CPU spin on recv error, and a `"lo"` default that does not exist on macOS |
| — HTTP family defects | `3c414406`, `f2d4cf3e`, `e9eec1cb`, `e32cf485` | 204 responses were served as empty 200; a model status of 999 or a CRLF header panicked the connection task; HTTP/2's `request_filter` was accepted and silently ignored because the hyper path is dead code; HTTP/3 documented as the QUIC transport it actually is |

### Independent verification after landing

Each of these was re-checked against a freshly built binary, driven over MCP stdio, separately
from the agent that made the change — because several of the bugs above were ones our own
tests had been passing straight through.

- **Script handler + TCP encoding together**: a Python handler returning
  `{"data":"48454c4c4f","encoding":"hex"}` puts `HELLO` on the wire, with no LLM call.
- **DNS against `dig`**: `dig @127.0.0.1 -p … example.com A` returns `status: NOERROR` with the
  question section echoed and the transaction id matched — the real-resolver check the Beta
  rating always implied.
- **NTP against a raw client**: 48-byte reply, version 4 echoed, mode 4, and the origin
  timestamp byte-identical to the client's transmit timestamp (`1d7a39180519de39`). Driven
  through a *static* handler, so the per-datagram fix holds on the zero-LLM path too.
- **DHCP remote panic**: a datagram declaring `hlen = 255` no longer kills the socket task —
  the server answered a well-formed request afterwards, echoing xid `deadbeef`, confirming both
  the panic guard and the per-datagram correlation fix.
- **The committed tree**: `cargo check --all-features` in a clean worktree at HEAD finishes in
  5m00s with **zero errors**. Worth re-running this way rather than in the working tree, which
  during heavy parallel work is routinely mid-edit and produces failures belonging to nobody.
- **TUI logging**: the rotating writer is exercised on the interactive path — netget started
  under a real PTY produced a well-formed `netget.log` through `RotatingFileWriter`. The
  interactive branch of `init_logging` is the only consumer, so a compile check alone would not
  have shown this.

---

## P0 — Correctness bugs reachable from untrusted input

### 56. Sixteen protocols never show the model their own actions **[verified]**

`call_llm` builds the model's action list from `event.event_type.actions`
(`src/llm/action_helper.rs:455`) — **not** from `get_sync_actions()`. Only its sibling
`call_llm_with_actions` consults the protocol (`:130`). So a protocol that uses `call_llm` and
never calls `.with_actions(...)` on its `EventType`s offers the model nothing it can act on,
however many actions it declares. Anything protocol-specific the model returns is rejected as
an unknown action, retried twice, and fails.

Measured empirically. An npm server was pointed at a capturing endpoint and sent a real HTTP
request; the tool list in the prompt was exactly:

```
set_memory, append_memory, show_message, append_to_log
```

Zero npm actions, against five declared. The model cannot answer an npm request at all.

Affected — using `call_llm`, no `.with_actions(...)` anywhere, declaring sync actions:
`bluetooth_ble`, `dc` (10 actions), `ipsec`, `isis`, `nntp` (8), `npm` (5), `ollama`,
`openai` (**Beta**), `openvpn`, `rip`, `rss`, `sip` (6), `smb` (10), `stun`, `tftp`,
`wireguard`.

MongoDB had exactly this and was fixed in `b769f29a` — its six actions were all being rejected
as unknown, which is also why its E2E suite was failing silently against a broken protocol.

Two fixes are needed, and the second matters more:
1. Add `.with_actions(...)` to the affected event types.
2. Remove the trap. Two functions that differ only in whether the protocol's own actions reach
   the model is a footgun that has now caught at least 17 protocols. Either have `call_llm`
   consult the protocol too, or make `EventType` require its actions at construction so the
   omission cannot compile.

Reproduce the sweep with:
```bash
for f in src/server/*/actions.rs; do d=$(dirname $f); p=$(basename $d)
  ev=$(grep -c "EventType::new(" $f); wa=$(grep -c "\.with_actions(" $f)
  sy=$(sed -n '/fn get_sync_actions/,/^    }/p' $f | grep -cE "_action\(\)")
  pl=$(grep -rl "call_llm(" $d | wc -l); wi=$(grep -rl "call_llm_with_actions(" $d | wc -l)
  [ "$ev" -gt 0 ] && [ "$wa" -eq 0 ] && [ "$sy" -gt 0 ] && [ "$pl" -gt 0 ] && [ "$wi" -eq 0 ] && echo "$p ($sy invisible)"
done
```

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

### 20. The dependency system is plumbed but inert — adopting it is cheap **[verified]**

Corrected: an earlier version of this item said nothing calls it. Not so.
`get_excluded_protocols()` is genuinely called from `src/events/handler.rs:2155,2159` and
`src/cli/sticky_footer.rs:1327,1334`, and both registries expose `get_available_protocols()`
and `is_protocol_available()` on top of it. The plumbing reaches the TUI and the event handler
already.

What is missing is the *data*: `get_excluded_protocols` computes exclusions by calling
`protocol.get_dependencies()`, and **no protocol overrides it** — every one inherits the empty
default at `src/llm/actions/protocol_trait.rs:171`, so the exclusion map is always empty and
the whole mechanism silently does nothing.

That makes this cheap rather than a rewrite: add `get_dependencies()` to the protocols that
need it and the TUI, the event handler and the LLM's protocol list all start excluding them
with the install hints the design already produces. The trait doc at
`protocol_trait.rs:165-168` even spells out the intended values — pcap and raw sockets for
ARP/DataLink, TUN and root for WireGuard, `protoc` for gRPC. Start with the raw-socket and
system-library protocols, which are exactly the ones that currently fail with an opaque OS
error. `src/protocol/dependencies.rs` should be kept, not deleted.

<!-- superseded wording removed -->

### 20b. Startup still does not consult it **[static]**

Separately from the missing data above: neither `start_server_by_id` nor
`start_server_from_action` calls `is_protocol_available()` before `spawn()`. So even once
protocols declare their dependencies, the TUI and the LLM's protocol list will exclude an
unusable protocol while a direct `start_server` still attempts it and fails with whatever raw
error the underlying library produces. One call in the startup path closes that gap.

### 21. Five protocols expose zero actions — and the grep that found them was misleading **[verified]**

Corrected: an earlier version of this item listed nine, including `doh` and `dot`. Those two
are fine. They **delegate** — `get_sync_actions()` forwards `DnsProtocol`'s set verbatim, so
the model sees the full DNS vocabulary on every query. A grep for `vec![]` cannot tell
delegation from hollowness; measure both:

```
grep -A3 "fn get_sync_actions" src/server/<p>/actions.rs | grep -c 'vec!\[\]'   # hollow
grep -c "DnsProtocol\|BluetoothBle::" src/server/<p>/actions.rs                  # delegating
```

The genuinely hollow ones are `bluetooth_ble_proximity`, `_presenter`, `_gamepad`,
`_environmental` and `_file_transfer`: literally `vec![]` for both actions and events, zero
delegation, so the LLM gets nothing it can act on — while they are registered `Experimental`
and therefore offered. `bluetooth_ble_proximity` compounds it by shipping
`get_startup_examples()` referencing an event (`ble_proximity_detected`) and an action
(`wait_for_more`) it does not define, so the example is unusable copy-paste.

`amqp` and `mqtt` were the same shape (`// Placeholder` in source) and are being addressed
separately.

The rule worth keeping: a protocol the model can select but cannot control is worse than one
that is not offered — it will be chosen, accept connections, and never respond. Either
delegate like `doh`/`dot`, implement, or mark `Incomplete` so `is_available_to_llm()` hides it.


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

**The same blind spot applies to test targets, and there it is worse.** `--features telnet`
alone failed to *compile* two test targets (`57a4c42f`) — and a test target that does not
compile does not report failures, it simply contributes nothing, exactly like the orphaned
directories in item 5. So the sweep should be `cargo check --tests`, not `cargo check`, or the
next silent gap will be a whole target rather than a directory.

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

### 49. The DNS client loops until the process overflows its stack **[verified]**

`tests/client/dns/e2e_test.rs::test_dns_client_multiple_queries` drives the DNS **client**
into an unbounded loop: it issued **211 LLM calls** and then netget died with a stack
overflow. This is a client-side runaway, not a mock artifact — it surfaced only once the
mock stopped failing every test before it got that far.

Two things are wrong and both deserve fixing: whatever makes the client re-issue instead of
terminating, and the absence of any ceiling on LLM calls per client session. Servers have the
per-connection Idle/Processing/Accumulating machine to prevent re-entrancy; clients hand-roll
their own (item 7 in spirit), and evidently one of them does not converge. A hard cap with a
clear error beats an unbounded loop regardless of the root cause.

### 50. Two overlapping documentation gates, one of them dead **[verified]**

`REQUIRE_DOCS_FOR_OPEN_ACTIONS` (`src/events/handler.rs:20`) is a hardcoded `false` and only
controls whether `open_server`/`open_client` appear in the action list. The gate that actually
forces a `DocumentationRequired` retry is unconditional, at `src/events/handler.rs:807` and
`:1223`. That retry costs a full extra model round-trip on the first `open_server` of every
process, and it gives the model nothing: the handler already fetches the documentation itself
and calls `mark_server_protocols_documented` before returning the error, so it could simply
fall through and start the server.

Gate both behind the existing flag. Note the flag is process-global — `is_server_docs_read()`
returns `!documented_server_protocols.is_empty()` (`src/state/app_state.rs:728`) — so it fires
once per process regardless of which protocol is being started.

### 51. Remaining test failures after the mock fix **[verified]**

All test-side, none in `src/`:
- `server::http::test::{test_http_routing, test_http_error_responses}` key their mocks on
  `event_data["uri"]`, but the HTTP event emits `path`.
- `server::http::test::test_http_simple_get_with_logging` answers a network event with
  `write_file`, which is not in HTTP's sync action set.
- 3 × `server::http::e2e_scheduled_tasks_test` wait for the log line
  `[TASK] Created one-shot task` while NetGet emits `[TASK] Scheduled one-shot task`
  (`src/events/handler.rs:1237,1242`).

### 52. `scheduled_tasks` on `open_server` never reports task creation **[verified]**

`start_server_from_action` (`src/cli/server_startup.rs`, roughly `:480-520`) handles the
`scheduled_tasks` array by calling `state.add_task(task).await` with **no `status_tx.send(...)`
at all** — so creating a task this way is completely silent, in the TUI and over MCP alike.
The standalone `schedule_task` action does log (`src/events/handler.rs:1237,1242`), so the two
paths disagree.

Found while fixing item 51, where I had misdiagnosed three failing tests as a log-string
mismatch. The agent applied the exact string fix I specified, watched the tests still fail,
and traced it to this gap rather than accepting the premise — the right call. The tests now
assert on task *execution*, which does fire reliably, and the missing status line is left for
whoever owns `server_startup.rs`.

### 53. Test-suite state at HEAD **[verified]**

Measured in a clean worktree with `--features tcp,http,dns,udp,redis,mcp-stdio --no-fail-fast`,
so this is the committed tree, not the working tree.

`tests/server.rs` is **31 passed, 0 failed** — the protocol E2E suite is green for that feature
set, against 5/24 at session start. `--lib` 23/23, `examples` 34/34, `mcp_stdio` 9/9,
`static_handler_interpolation` 30/30, `truncate` 13/13, `prompt` and `prompt_snapshots` green.

Remaining failures, all attributed:

- **`ollama_model_test`: 20 of 25 fail.** This is a *model-evaluation* framework — it drives a
  real Ollama at `OLLAMA_BASE_URL` with `qwen2.5-coder:7b` and grades the model's answers. It
  is neither `#[ignore]`d nor feature-gated, so it fails for anyone without that exact setup,
  and would fail in CI. It should be `#[ignore]`d or moved behind the project's existing
  `--use-ollama` opt-in.
- **`logging_unit_test::test_append_to_log_action_definition`.** Asserts the action description
  contains the literal `"log file"`; the description was reworded to "append logs to a file" in
  `e75043df` (2025-12-29). Confirmed pre-existing — `e75043df` is an ancestor of this session's
  starting commit. Either assert on behavior or fix the string.
- **`scripting_executor_test`: 2 Perl cases.** This machine's Homebrew Perl lacks the `JSON`
  CPAN module (`perl -e 'use JSON'` fails standalone). Environmental, not a code defect, but the
  test should detect and skip rather than fail.
- **`client`: 5 of 21.** Includes the DNS client runaway (item 49), being fixed separately.

### 54. ARP and DataLink still cannot ship on Linux **[verified]**

`748fccca` added `arp` to `dist-darwin` and `icmp` to `dist`, which fixed macOS. But `arp` and
`datalink` remain absent from `dist` itself, so the Linux release binaries still cannot start
them at all — the same "Unknown protocol" dead end, just on a different platform. They need
libpcap at link time, which is the original reason for the exclusion; the options are bundling
it, dynamically loading it, or documenting the gap in the release notes rather than leaving
users to discover it.

### 55. `SystemCapabilities` conflates two capabilities **[static]**

One `has_raw_socket_access` flag covers both raw IP sockets (ICMP, IGMP, OSPF) and L2 capture
via BPF/AF_PACKET (ARP, DataLink, IS-IS). These genuinely differ — a macOS user in the ChmodBPF
group has `/dev/bpf*` without being root. `c9a65c80` probes both and grants the flag if either
succeeds, which is permissive in the right direction but still cannot refuse an ICMP server for
a capture-only user. Splitting the flag would let each protocol declare what it actually needs.

### 57. Proxy TLS interception: what it actually does with keys **[verified]**

Recorded because "MITM proxy" invites the question and the answer was not written down
anywhere. **No private key is written to disk, and there is no fixed or hardcoded key.** The CA
key pair is generated in memory at server start (`rcgen::KeyPair::generate`) and dropped when
the server stops; per-domain leaf keys are memory-only too. Intercepted plaintext reaches
`netget.log` only at TRACE, like every other protocol.

The one disk write is the new `ca_export_path` startup parameter, and it writes the CA
**certificate** — the public half — at `0666 & ~umask`, normally `0644`, which is right for
something meant to be distributed. The private half is not written by any code path and is not
reachable from any action. So interception cannot happen silently: without an explicit trust
grant the client aborts with unknown-issuer.

Worth revisiting only if CA persistence is ever added — at that point the key file's mode and
location become a real decision rather than a non-issue.

### 58. The base BLE server ignored every profile's protocol object **[verified]**

Corrected again, and this supersedes both earlier versions of item 21. The count was never
five, or twelve — it was **all sixteen**, and the reason is structural rather than per-profile:
`BluetoothBle::spawn_with_llm_actions` hardcodes `BluetoothBleProtocol` when it calls `call_llm`
(`src/server/bluetooth_ble/mod.rs:124,142,773`). So the base's events were the only ones
emitted, the base's `.with_actions(...)` lists the only vocabulary offered, and
`BluetoothBle::execute_action` the only executor. Every profile-specific action and event in
the family was unreachable no matter what it declared.

Fixed in `3a7bbb5a`, `691de8d8`, `f17e3f1c`: fifteen profiles now delegate explicitly the way
`doh`/`dot` do, and `bluetooth_ble_beacon` is `Incomplete` — `ble-peripheral-rust` 0.2 exposes
no advertising-payload API, and a beacon is nothing but its advertising payload, so delegating
would have handed it a working GATT vocabulary that is precisely not a beacon.

Along the way: two profiles discarded the user's instruction entirely, one had actions declared
but absent from its own match arm, several advertised a connection table that cannot be
constructed, and `execute_action` waved unknown actions through as `Custom` rather than
rejecting them.

**Lesson worth keeping:** three different shapes of "the model cannot act on this" have now
been found — hollow (`vec![]`, no delegation), declared-but-unadvertised (no `.with_actions()`),
and structurally overridden (a shared spawn hardcoding the base protocol). A single grep
detects none of them reliably. The check in CLAUDE.md now covers all three.

### 59. The BLE base teaches a UUID form that cannot parse **[verified]**

`Uuid::parse_str("180D")` fails and no 16-bit expansion helper exists in the tree, yet
`src/server/bluetooth_ble/CLAUDE.md:285` claims the shorthand is "expanded to" the 128-bit
form, and the base's own `BLUETOOTH_BLE_STARTED_EVENT` and `add_service` examples
(`src/server/bluetooth_ble/actions.rs:24,27,449`) both use it. A model copying the protocol's
own documented example gets "Invalid service UUID". Either add the expansion (`0000XXXX-0000-1000-8000-00805F9B34FB`)
or correct the examples — the same documented-but-unimplemented class as items 2 and 38.

### 60. `PrivilegeRequirement` cannot express device access **[static]**

`src/protocol/metadata.rs:7-16` offers `None`, `PrivilegedPort`, `RawSockets`, `Root`. BLE needs
Bluetooth adapter access (D-Bus/BlueZ on Linux, CoreBluetooth TCC on macOS); USB and NFC need
their own device permissions. None of these is a port or a raw socket, and claiming `Root` would
be false *and* would block users who genuinely have adapter access. All seventeen BLE protocols
are therefore left at `None`, which is also wrong. Needs an `AdapterAccess`/`DeviceAccess`
variant — related to item 55, which wants `has_raw_socket_access` split as well.

Compounding it: `src/server/bluetooth_ble/mod.rs:108` reports "Bluetooth adapter failed to power
on after 10 seconds" for all three of adapter-off, no-adapter, and permission-denied, so a user
cannot tell which.

### 61. The auth family claimed cryptography it never performed **[verified]**

None of `oauth2`, `openid`, `saml_idp`, `saml_sp` holds a signing key, and none ever did. That
is defensible for LLM-driven test servers — but three of them said otherwise in the text the
model and the user read:

- `saml_idp`'s `description()` said it "generates signed SAML assertions". A `<ds:Signature>`
  appears only if the model invents one, and it will not verify.
- `saml_sp`'s said it "validates SAML assertions", and `llm_control` listed "assertion
  validation". There is no signature, issuer, audience, expiry or replay check.
- `openid` advertised "LLM-generated JWT tokens". Nothing signs and nothing verifies; the JWKS
  is whatever keys the model invented, unrelated to any signature.

All corrected to state plainly that NetGet performs no cryptography here. Nothing is written to
disk in any of the five. Fixed across six commits.

### 62. OAuth2 turned a denial into an approval, and failed open **[verified]**

The most serious defect found this session. `/authorize` decided by scanning the model's JSON
for a `code` field. `oauth2_error_response` — the model's *only* way to refuse — has no such
field, so **a denial was indistinguishable from silence** and fell through to a hardcoded
`AUTH_CODE_123`. A client received a working authorization code for a request the model had
explicitly rejected. `/token` had the mirror defect, returning the error body with `200 OK`.

All three endpoints also failed open on no-answer: `AUTH_CODE_123`, `ACCESS_TOKEN_123`, and
introspection returning `{"active": true}` for **every** bearer token. So an LLM outage silently
issued credentials and validated anything presented to it. `src/server/oauth2/CLAUDE.md`
documented this as a feature — "ensures the server always responds correctly".

Fixed with a result envelope and fail-closed defaults; verified with `curl` that a denial now
yields `302 …?error=unauthorized_client` and `401 {"error":"invalid_client"}`. The general rule
is now in CLAUDE.md's systemic-issues list.

Note what this did to the tests: `tests/server/oauth2/e2e_test.rs:227` mocks `send_token_response`,
which is *openid*'s action name, so the action was always rejected as unknown — and the test
**passed at HEAD only because the fail-open default masked it**. It now correctly fails. A test
can be green *because* of the bug it should be catching.

### 63. Two credential-minting protocols have no tests at all **[verified]**

`saml_idp` and `saml_sp` have no `tests/` directory — no `e2e_test.rs`, no `CLAUDE.md`, no
`pub mod` line. Zero coverage for the two protocols in the repo that mint and consume
authentication assertions. Also: `tests/server/openid/e2e_test.rs:67,343` starts
`"base_stack": "http"` and mocks `http_request_received`, so it does not exercise `openid` at
all.

### 64. Injection surfaces in `saml_sp` and `oauth2` **[verified]**

`saml_sp` took `user_id` from an attacker-supplied assertion and wrote it unescaped into HTML
*and* raw into `Set-Cookie`. `oauth2`'s `parse_query_params` percent-decodes, so
`?redirect_uri=http://x/cb%0D%0A` put CRLF into the `Location` header and killed the connection
task — remotely reachable and unauthenticated. Both fixed and byte-verified with `od -c`.

### 65. `{"type": "placeholder"}` response examples remain in 54 files **[verified]**

Rendered verbatim into prompts (`src/llm/actions/tools.rs`) and MCP docs
(`src/mcp_stdio/docs.rs:399`), teaching the model an action type that does not exist. Fixed
piecemeal by protocol reviews; 54 `src/server/` files still carry one. Worth a sweep, or a
render-time suppression so a placeholder is omitted rather than shown.

### 66. `http_common` is gated too narrowly **[static]**

`src/server/mod.rs:23` gates it on `feature = "http"` or `"http2"`, so `oauth2`, `openid`,
`saml-idp` and `saml-sp` — all HTTP servers — cannot reach `build_safe_response`
(`src/server/http_common/handler.rs:135`) and have four local copies of it. Widening the gate
would collapse them.

---

## Suggested order

1. Items 1-4 — remotely-reachable panics and the broken feedback loop.
2. Items 5-6 — turn the dormant test suite on and gate it in CI; everything else is easier
   to land safely afterwards.
3. Items 11-12 — make the MCP surface self-describing and give it client parity.
4. Items 7-10, 18-19 — silent failures and the privilege/packaging gaps.
5. Items 13-17, 20-23 — LLM tooling quality and consistency.
