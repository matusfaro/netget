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
  |--- 10+ MCP tools (list_protocols, start_server, etc.)
  |--- AppState (shared server/client state)
  |--- Sampling forwarder (routes LLM calls to MCP client)
  v
Protocol Servers (tcp, http, dns, ...)
  |
  v
OllamaClient (3 backends)
  |--- Ollama /api/chat  (local)
  |--- OpenAI /v1/chat   (remote)
  |--- Sampling (MCP client's LLM)
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
| `stop_all` | Stop all servers | Iterate + remove all |

## Sampling Integration

When the MCP client supports sampling (`capabilities.sampling`):

1. A single sampling forwarder task is spawned once in `create_shared_state()`. It
   reads the *current* peer from `SharedState.peer` (an `Arc<Mutex<Option<Peer>>>`)
   on every request, so it always targets the most recently initialized client.
   This supports client reconnects (STDIO) and multiple sessions (HTTP).
2. During `initialize`, the server stores the peer into `SharedState.peer`.
3. `start_server` with `llm_provider: "sampling"` creates `OllamaClient::new_sampling()`.
4. Protocol servers make LLM calls via `OllamaClient.chat_with_tools()`.
5. The Sampling backend sends requests through a channel to the forwarder task.
6. The forwarder converts to `CreateMessageRequestParams` and calls `peer.send_request()`.
7. Response flows back through a oneshot channel to the protocol server.

This means the MCP client's LLM (e.g., Claude) directly controls protocol behavior.
If no client is connected when a request arrives, the forwarder returns an error
rather than hanging.

## LLM Provider Selection

The `start_server` tool's `llm_provider` parameter:
- `"sampling"` (default when client supports it) - MCP client's LLM
- `"ollama"` - Local Ollama instance
- `"openai"` - OpenAI-compatible API (requires --openai-url and --api-key)

## Logging

All logging goes to stderr (stdout is JSON-RPC). Status messages from protocol servers are drained to stderr with `[NETGET]` prefix.

## Limitations

- **STDIO is single-session**: one client at a time (HTTP supports many)
- **No elicitation yet**: Interactive config gathering not implemented
- **No dynamic tool list**: All tools exposed from start (no `tools/list_changed`)
- **Sampling tool calls**: When using sampling, native tool_calls from the MCP client LLM are not forwarded back to protocol servers (text content only)

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
