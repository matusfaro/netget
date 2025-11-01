# NetGet - LLM-Controlled Network Protocol Server

Rust CLI where an LLM (via Ollama) controls 40+ network protocols. The LLM constructs raw protocol datagrams or high-level responses.

## Protocols (40+)

**Beta**: TCP, HTTP, UDP, DataLink, DNS, DoT, DoH, DHCP, NTP, SNMP, SSH
**Alpha**: IRC, Telnet, SMTP, IMAP, mDNS, LDAP, MySQL, PostgreSQL, Redis, Cassandra, DynamoDB, Elasticsearch, IPP, WebDAV, NFS, SMB, HTTP Proxy, SOCKS5, STUN, TURN, WireGuard (full VPN), OpenVPN (honeypot), IPSec (honeypot), BGP, OpenAI

See protocol-specific docs: `src/server/<protocol>/CLAUDE.md`, `tests/server/<protocol>/CLAUDE.md`

## Architecture Principles

**Decentralization (CRITICAL)**: Never create centralized protocol registries. Use trait-based patterns where each protocol implements traits independently. Exceptions: `BaseStack` enum, `Cargo.toml` features, `server_startup.rs` match statements.

**Modules**: `cli/` (TUI), `server/<protocol>/` (implementations), `protocol/` (BaseStack), `state/` (app state), `llm/` (Ollama), `events/` (coordination), `llm/actions/` (action system)

**Connection**: TcpStream split with `tokio::io::split()`. Never hold Mutex during I/O (deadlock risk).

**Data Queueing**: Per-connection state machine (Idle → Processing → Accumulating) prevents concurrent LLM calls.

**LLM Responses**: Action-based JSON format: `{"actions": [{"type": "send_tcp_data", "data": "..."}, ...]}`

**Actions**: Protocols implement `ProtocolActions` trait with async (user-triggered) and sync (network event) actions. Files: `src/server/<protocol>/actions.rs`

## Protocol Documentation (CRITICAL)

Each protocol has TWO CLAUDE.md files:
- `src/server/<protocol>/CLAUDE.md` - Implementation (library choices, architecture, LLM integration, limitations)
- `tests/server/<protocol>/CLAUDE.md` - Testing (strategy, LLM call budget, runtime, known issues)

**Always read both before modifying a protocol.**

## Testing Philosophy

Black-box, prompt-driven. LLM interprets prompts, tests validate with real clients.

**Status**: Unit tests: 12/12 passing. E2E: Infrastructure fixed, all compile. See `TEST_INFRASTRUCTURE_FIXES.md`, `TEST_STATUS_REPORT.md`.

### Organization & Feature Gating (CRITICAL)

- All tests in `tests/` (never `src/`), access public APIs only
- Protocol E2E tests: `tests/server/<protocol>/e2e_test.rs`
- **ALL tests MUST be feature-gated**: `#[cfg(all(test, feature = "<protocol>"))]` in mod.rs
- Unit tests (no Ollama): `tests/base_stack_test.rs`, etc.
- E2E tests (Ollama required): Real clients, use `{AVAILABLE_PORT}` placeholder

### Running Tests

```bash
# Unit tests
./cargo-isolated.sh test --lib

# Protocol-specific E2E (ALWAYS use --features, never run all tests)
./cargo-isolated.sh test --no-default-features --features <protocol> --test server::<protocol>::e2e_test
```

### E2E Test Efficiency (CRITICAL)

**Minimize LLM calls** (target < 10 per suite):
1. Reuse server instances across test cases (one comprehensive prompt vs. multiple servers)
2. Use scripting mode when available (0 LLM calls per request after startup)
3. Bundle related test scenarios

**Setup**: Build release binary first: `./cargo-isolated.sh build --release --all-features`

**Privacy**: All tests MUST use localhost only (127.0.0.1/::1), no external endpoints, works offline.

## Multi-Instance Concurrency

**Ollama Lock**: `--ollama-lock` flag serializes LLM API access (enabled by default in tests). Prevents Ollama overload when running concurrent tests.

**Safe**: Multiple E2E tests, multiple NetGet instances with `--ollama-lock`
**Unsafe**: Building to same `target/` (use `cargo-isolated.sh`), concurrent git operations (use worktrees)

### Build Isolation (CRITICAL)

**Always use `./cargo-isolated.sh`** instead of `cargo` - creates session-specific build dirs (`target-claude/claude-$$`)

**Kill builds safely**: `./cargo-isolated-kill.sh` (NEVER `pkill cargo` - kills all instances!)

**Feature flags for speed**:
- ✅ Fast (10-30s): `--no-default-features --features <protocol>`
- ❌ Slow (1-2min): `--all-features` (only for releases/full tests)

**Cleanup**: `rm -rf target-claude/` or `find target-claude/ -mtime +10 -exec rm -rf {} \;`

## Logging (CRITICAL)

**Dual logging required** - ALL logs to BOTH tracing macros (`debug!`, `trace!`, etc.) → `netget.log` AND `status_tx.send()` → TUI

```rust
debug!("TCP sent {} bytes", len);
let _ = status_tx.send(format!("[DEBUG] TCP sent {} bytes", len));
```

**Levels**: ERROR (critical), WARN (non-fatal), INFO (lifecycle), DEBUG (summaries), TRACE (full payloads)

## UI & Technical Details

**TUI**: Rolling terminal (scrolls like `tail -f`), sticky footer, Ctrl+L (log levels), Ctrl+W (web search), command history, multi-line input (Shift+Enter)

**Key Tech**:
- TcpStream: `tokio::io::split()`, never clone
- Mutex: Never hold during I/O (deadlock risk)
- Default model: `qwen3-coder:30b`
- Event flow: UserCommand → Parse → EventHandler → LLM → Protocol action

## Protocol Planning (Before Implementation)

Before implementing, research and document:
1. **Server library** - Rust crate evaluation (compliance, maturity, LLM control flexibility)
2. **Client library** - For E2E testing
3. **LLM control points** - Async (user-triggered) vs Sync (network event) actions
4. **Logging strategy** - What to log at each level
5. **Example prompts** - Comprehensive prompt covering main features (basis for E2E tests)

## Protocol Implementation Checklist (CRITICAL: ALL protocols MUST be feature gated)

**12-Step Implementation**:
1. **base_stack.rs**: Add `BaseStack` variant, parsing, unit tests
2. **rolling_tui.rs**: Add welcome message, mark Alpha/Beta
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

**Compilation errors**: PAUSE if error in code you didn't modify (another instance may be working on it). Only fix errors in your own edits.

**Shared files** (`Cargo.toml`, `base_stack.rs`, `server/mod.rs`, `server_startup.rs`, `state/server.rs`): NEVER overwrite. ALWAYS use Edit tool for surgical changes. Add incrementally without removing others' work.

**Kill builds**: Use `./cargo-isolated-kill.sh` (NEVER `pkill cargo` - kills all instances!)

## Git Commits

Only commit when user requests. DO NOT add AI references ("Generated with Claude Code", "Co-Authored-By"). Keep messages professional and concise.
