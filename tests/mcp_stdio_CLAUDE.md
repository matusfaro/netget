# MCP STDIO Test Strategy

## File

`tests/mcp_stdio_test.rs` (top-level integration binary, gated on
`#[cfg(all(feature = "mcp-stdio", feature = "tcp"))]`).

## Approach

In-process, **no Ollama, no real stdin/stdout**. The server service is served
over a `tokio::io::duplex` pipe and driven by an rmcp client (`serve_client((), ..)`)
on the other end. This exercises the real JSON-RPC handshake, tool discovery, and
tool dispatch without any external process or LLM.

The rmcp `client` feature is enabled via the `mcp-stdio` / `mcp-http` Cargo
features specifically so these tests can act as an MCP client.

## Test cases

1. `initialize_and_list_tools` — completes the MCP handshake, asserts server
   name is `netget`, and that the core management tools (`list_protocols`,
   `start_server`, `stop_server`, `get_status`) are advertised.
2. `call_list_protocols_returns_tcp` — calls the `list_protocols` tool with
   `type=server` and asserts the compiled-in `tcp` protocol appears in the output.

## LLM call budget

**Zero.** Both tests only call tools that read the protocol registry / app state.
`start_server` (which would invoke the LLM) is intentionally not exercised here —
that path is covered by the protocol E2E suites.

## Running

```bash
./cargo-isolated.sh test --no-default-features --features mcp-stdio,tcp \
    --test mcp_stdio_test -- --test-threads=100
```

## Expected runtime

< 1 second after compilation (no network, no LLM).

## Not covered here

- `start_server` end-to-end (it invokes the LLM) — covered by the protocol E2E suites.
- HTTP transport (`--mcp-http`) wiring — smoke-testable by binding a port and
  issuing an `initialize` over HTTP; not yet automated.
