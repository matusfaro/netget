# NetGet - LLM-Controlled Network Protocol Server

Rust CLI where an LLM (via Ollama) controls 40+ network protocols. The LLM constructs raw protocol datagrams or high-level responses.

## Protocols (50+)

**Beta**: TCP, HTTP, UDP, DataLink, DNS, DoT, DoH, DHCP, NTP, SNMP, SSH, OpenAI
**Experimental**: IRC, Telnet, SMTP, IMAP, mDNS, LDAP, MySQL, PostgreSQL, Redis, Cassandra, DynamoDB, Elasticsearch, IPP, WebDAV, NFS, SMB, HTTP Proxy, SOCKS5, STUN, TURN, Tor Directory, gRPC, MCP, JSON-RPC, XML-RPC, VNC, etcd, Kafka, MQTT, Git, S3, SQS, BOOTP
**Stable**: WireGuard (full VPN), Tor Relay
**Incomplete**: OpenVPN (honeypot), IPSec (honeypot), BGP

See `/docs` command for protocol details and metadata. Use `METADATA_EXAMPLES.md` for classification reference.

See protocol-specific docs: `src/server/<protocol>/CLAUDE.md`, `tests/server/<protocol>/CLAUDE.md`

## Architecture Principles

**Decentralization (CRITICAL)**: Never create centralized protocol registries. Use trait-based patterns where each protocol implements traits independently. Exceptions: Protocol registry (`protocol/registry.rs`), `Cargo.toml` features, `server_startup.rs` match statements.

**Modules**: `cli/` (TUI), `server/<protocol>/` (implementations), `protocol/` (registry, metadata), `state/` (app state), `llm/` (Ollama), `events/` (coordination), `llm/actions/` (action system)

**Connection**: TcpStream split with `tokio::io::split()`. Never hold Mutex during I/O (deadlock risk).

**Data Queueing**: Per-connection state machine (Idle → Processing → Accumulating) prevents concurrent LLM calls.

**Actions**: Protocols implement `ProtocolActions` trait with async (user-triggered) and sync (network event) actions. Files: `src/server/<protocol>/actions.rs`

**Actions/Events Design (CRITICAL)**: NEVER use bytes (`Vec<u8>`) or base64-encoded strings in action parameters or event data. LLMs cannot effectively parse or construct binary data. Instead, use structured data (JSON objects, fields, enums) that you construct into bytes. Example: Instead of `{"data": "SGVsbG8="}`, use `{"method": "GET", "path": "/", "headers": {...}}`.

## Protocol Documentation (CRITICAL)

Each protocol has TWO CLAUDE.md files:
- `src/server/<protocol>/CLAUDE.md` - Implementation (library choices, architecture, LLM integration, limitations)
- `tests/server/<protocol>/CLAUDE.md` - Testing (strategy, LLM call budget, runtime, known issues)

**Always read both before modifying a protocol.**

## Testing Philosophy

Black-box, prompt-driven. LLM interprets prompts, tests validate with real clients. **Status**: Unit 12/12 passing, E2E infrastructure fixed. See `TEST_INFRASTRUCTURE_FIXES.md`, `TEST_STATUS_REPORT.md`.

### Organization & Feature Gating (CRITICAL)

- All tests in `tests/` (never `src/`), access public APIs only
- Protocol E2E tests: `tests/server/<protocol>/e2e_test.rs`
- **ALL tests MUST be feature-gated**: `#[cfg(all(test, feature = "<protocol>"))]` in mod.rs
- Unit tests (no Ollama): `tests/base_stack_test.rs` (registry parsing), etc.
- E2E tests (Ollama required): Real clients, use `{AVAILABLE_PORT}` placeholder

### Running Tests

```bash
# Unit tests
./cargo-isolated.sh test --lib

# Protocol-specific E2E (ALWAYS use --features, never run all tests)
./cargo-isolated.sh test --no-default-features --features <protocol> --test server::<protocol>::e2e_test
```

### E2E Test Efficiency (CRITICAL)

**Minimize LLM calls** (< 10 per suite): Reuse servers, use scripting mode, bundle scenarios. **Setup**: `./cargo-isolated.sh build --release --all-features`. **Privacy**: Localhost only (127.0.0.1/::1), no external endpoints.

## Multi-Instance Concurrency

**Ollama Lock**: `--ollama-lock` serializes LLM API (default in tests). **Safe**: Multiple E2E tests/instances with lock. **Unsafe**: Same `target/` (use `cargo-isolated.sh`), concurrent git (use worktrees).

### Build Isolation (CRITICAL)

**Use `./cargo-isolated.sh`** (session-specific `target-claude/claude-$$`). **Kill**: `./cargo-isolated-kill.sh` (NEVER `pkill cargo`). **Speed**: Fast with `--no-default-features --features <protocol>` (10-30s), slow with `--all-features` (1-2min). **Cleanup**: `rm -rf target-claude/`.

## Logging (CRITICAL)

**Dual logging**: ALL logs to tracing macros (`debug!`, `trace!`, etc.) → `netget.log` AND `status_tx.send()` → TUI. **Levels**: ERROR (critical), WARN (non-fatal), INFO (lifecycle), DEBUG (summaries), TRACE (payloads).

## UI & Technical Details

**TUI**: Rolling terminal, sticky footer, Ctrl+L (log levels), Ctrl+W (web search), multi-line (Shift+Enter). **Tech**: TcpStream via `tokio::io::split()` (never clone), never hold Mutex during I/O, default model `qwen3-coder:30b`, flow: UserCommand → Parse → EventHandler → LLM → Protocol action.

## Scheduled Tasks

Tasks execute at intervals/delays. Three scopes: **Global** (any server, all actions), **Server** (specific server, auto-cleaned on close), **Connection** (specific connection, auto-cleaned on close).

**Connection tasks** for long-lived connections (SSH, WebSocket): idle timeouts, session cleanup, rate limiting, monitoring. Short-lived (HTTP GET) use server-level instead.

**Creation**: Via `open_server` action (`scheduled_tasks` array) or `schedule_task` action. Add `connection_id` for connection-scoped tasks. Parameters: `task_id`, `recurring`, `interval_secs`/`delay_secs`, `instruction`.

## Protocol Planning (Before Implementation)

Research: **Server library** (crate eval: compliance, maturity, LLM control), **Client library** (E2E testing), **LLM control points** (async vs sync actions), **Logging strategy**, **Example prompts** (comprehensive, basis for E2E).

## Protocol Implementation Checklist (CRITICAL: ALL protocols MUST be feature gated)

**12-Step Implementation**:
1. **protocol/registry.rs**: Register protocol implementation (feature-gated)
2. **rolling_tui.rs**: Add welcome message (state will be Experimental by default)
3. **src/server/<protocol>/mod.rs**: Implement server with dual logging, track connections
4. **src/server/<protocol>/actions.rs**: Implement `ProtocolActions` trait (async/sync actions)
5. **src/server/<protocol>/CLAUDE.md** (MANDATORY): Document implementation, libraries, LLM integration, limitations
6. **src/server/mod.rs**: Add feature-gated module declaration
7. **cli/server_startup.rs**: Add feature-gated match arm
8. **state/server.rs**: Add `ProtocolConnectionInfo` variant
9. **Cargo.toml** (MANDATORY): Add feature flag, optional deps, include in all-protocols
10. **tests/server/<protocol>/e2e_test.rs**: Create feature-gated E2E test (< 10 LLM calls)
11. **tests/server/<protocol>/CLAUDE.md** (MANDATORY): Document test strategy, LLM budget, runtime
12. **tests/server/helpers.rs**: Update if needed

**Validation**: Compiles with feature, tests pass, both CLAUDE.md files exist, < 10 LLM calls

**Common Pitfalls**: Missing feature flags/gates, missing CLAUDE.md files, inefficient E2E tests, forgetting dual logging, using `--all-features` for single protocol

## Multi-Instance Collaboration (CRITICAL)

**Errors**: PAUSE if error in unmodified code. **Shared files** (`Cargo.toml`, `protocol/registry.rs`, `server/mod.rs`, `server_startup.rs`, `state/server.rs`): NEVER overwrite, use Edit tool, add incrementally. **Kill**: `./cargo-isolated-kill.sh` (NEVER `pkill cargo`).

## Git Commits

Only commit when user requests. DO NOT add AI references ("Generated with Claude Code", "Co-Authored-By"). Keep messages professional and concise.
