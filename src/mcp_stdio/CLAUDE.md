# MCP Server Implementation (STDIO + HTTP)

## Overview

Runs NetGet as an MCP (Model Context Protocol) server, allowing MCP clients like
Claude Desktop and Claude Code to control protocol servers programmatically. Two
transports share the exact same tool implementation:

- **STDIO** (`--mcp`, feature `mcp-stdio`): one client over stdin/stdout. Used by
  Claude Desktop / Claude Code.
- **HTTP/SSE** (`--mcp-http PORT`, feature `mcp-http`): the MCP Streamable HTTP
  transport for remote/web clients. Multiple sessions share one `SharedState`
  (servers started in one session are visible to all). Bind address comes from
  `--listen-addr` (default `127.0.0.1`).

The two flags are mutually exclusive.

## Architecture

```
MCP Client (Claude Desktop/Code, or remote HTTP client)
  | stdin/stdout OR HTTP/SSE (JSON-RPC 2.0)
  v
rmcp STDIO transport  /  rmcp StreamableHttpService
  |
  v
NetGetMcpService (tools.rs)
  |--- 10 MCP tools (list_protocols, start_server, etc.)
  |--- AppState (shared server/client state)
  v
Protocol Servers (tcp, http, dns, ...)
  |
  v
OllamaClient (2 backends)
  |--- Ollama /api/chat  (local)
  |--- OpenAI /v1/chat   (remote)
```

## Library

- **rmcp** v1.2 - Official Rust MCP SDK
- Base features (`mcp-stdio`): `server`, `transport-io`, `client`
  (the `client` feature is enabled so the in-process smoke tests can drive the server)
- Added by `mcp-http`: `transport-streamable-http-server` (+ `axum` for the router/listener)
- **schemars** v1.0 - JSON Schema generation for tool parameters

## Entry Points

`netget --mcp` (alias `--mcp-stdio`) triggers `mcp_stdio::run_mcp_stdio()`:
1. Creates `NetGetMcpService` with AppState and tool router
2. Serves via `rmcp::transport::stdio()` (stdin/stdout)
3. Waits until client disconnects (stdin EOF)

`netget --mcp-http PORT` triggers `mcp_stdio::run_mcp_http()`:
1. Builds `SharedState` once via `NetGetMcpService::create_shared_state()`
2. Wraps it in an rmcp `StreamableHttpService` (one `NetGetMcpService` per session,
   all sharing the same `SharedState`) mounted at `/mcp` on an axum router
3. Serves until the process is stopped

## MCP Tools

| Tool | Description | Maps To |
|------|-------------|---------|
| `list_protocols` | List available server/client protocols | `ServerRegistry`, `ClientRegistry` |
| `start_server` | Start protocol server with LLM | `server_startup::start_server_from_action()` |
| `stop_server` | Stop server by ID | `AppState::remove_server()` |
| `list_servers` | List running servers | `AppState::get_all_servers()` |
| `server_status` | Detailed server status | `AppState::get_server()` |
| `get_status` | Overall NetGet status | AppState model + server count |
| `set_model` | Change LLM model | `AppState::set_ollama_model()` |
| `get_protocol_docs` | Protocol documentation | `generate_base_stack_documentation()` |
| `update_server_instruction` | Update server instruction | `AppState::with_server_mut()` |
| `list_access_logs` | Recent request/response entries (newest first) | `AppState::list_access_logs()` |
| `get_access_log` | Full request + response for one entry by id | `AppState::get_access_log()` |
| `stop_all` | Stop all servers | Iterate + remove all |

## Access Logs

Every network event a protocol server handles is recorded as an `AccessLogEntry`
in a bounded ring buffer on `AppState` (last 200, oldest dropped). The **request**
is the structured event data (e.g. HTTP method/path/headers); the **response** is
the action JSON the LLM produced (e.g. `send_http_response`). Recording happens
centrally in `action_helper::call_llm` after the event is handled, so it works for
every protocol without per-protocol wiring.

- `list_access_logs { limit? }` — newest-first summaries (id, age, server, protocol, request→response).
- `get_access_log { id }` — full request and response JSON for one entry.

Limitation: only successfully-handled events are logged; a request whose LLM call
errors out does not produce an entry.

## LLM Backend

Protocol servers started via `start_server` are driven by NetGet's own configured
LLM client (`SharedState.llm_client`), built once from the CLI args:

- **Ollama** (default) - local Ollama instance, or
- **OpenAI-compatible** - when `--openai-url` / `--api-key` are provided.

MCP **sampling** (routing protocol-server LLM calls back to the MCP client's model)
is intentionally **not** supported: the capability is being removed from the MCP
spec, so NetGet always uses its own LLM backend.

## Logging

All logging goes to stderr (stdout is JSON-RPC). Status messages from protocol servers are drained to stderr with `[NETGET]` prefix.

## Limitations

- **STDIO is single-session**: one client at a time (HTTP supports many)
- **No elicitation yet**: Interactive config gathering not implemented
- **No dynamic tool list**: All tools exposed from start (no `tools/list_changed`)
- **No sampling**: protocol servers always use NetGet's own LLM backend, never the MCP client's model

## Testing

Automated in-process smoke tests (no Ollama, no real stdio) live in
`tests/mcp_stdio_test.rs` — see `tests/mcp_stdio_CLAUDE.md`.

```bash
# Build
cargo build --features mcp-stdio,tcp,http,dns          # STDIO
cargo build --features mcp-http,tcp,http,dns           # HTTP

# Smoke tests
./cargo-isolated.sh test --no-default-features --features mcp-stdio,tcp \
    --test mcp_stdio_test -- --test-threads=100

# Claude Desktop config (claude_desktop_config.json):
{
  "mcpServers": {
    "netget": {
      "command": "/path/to/netget",
      "args": ["--mcp", "--model", "qwen3-coder:30b"]
    }
  }
}

# HTTP transport:
netget --mcp-http 8080          # serves MCP at http://127.0.0.1:8080/mcp
```
