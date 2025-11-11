# MCP (Model Context Protocol) Server Implementation

## Overview

MCP server implementing the Model Context Protocol specification, where the LLM controls all server capabilities:
resources, tools, and prompts. Built on JSON-RPC 2.0 over HTTP with session management and three-phase initialization.

## Protocol Version

- **MCP**: 2024-11-05 specification
- **Transport**: HTTP POST with JSON-RPC 2.0 messages
- **Framework**: https://modelcontextprotocol.io/

## Library Choices

### Core Dependencies

- **axum** v0.7 - Modern HTTP framework
    - Chosen for: Clean routing, extractors, better ergonomics than hyper
    - Used for: HTTP server and JSON-RPC endpoint
- **serde_json** - JSON handling
- **uuid** - Session ID generation
- **tokio** - Async runtime

### Custom JSON-RPC Implementation

**Why Not Use a JSON-RPC Library?**

- MCP has specific JSON-RPC patterns (initialize flow, notifications)
- Custom implementation provides full control over MCP semantics
- Simple enough to implement directly

### Session Management

**In-Memory Session Store**:

```rust
HashMap<String, Arc<Mutex<McpSession>>>
```

- Sessions keyed by UUID
- Tracks initialization state, capabilities, subscriptions

## Architecture Decisions

### Three-Phase Initialization

**MCP Handshake**:

1. **Client → initialize request** (with clientInfo, capabilities)
2. **Server → initialize response** (with serverInfo, capabilities)
3. **Client → initialized notification** (confirms connection)

Only after phase 3 can client make resource/tool/prompt requests.

### LLM Control Points

**Complete Capability Control** - LLM declares all capabilities:

1. **initialize**: LLM declares supported resources, tools, prompts
2. **resources/list**: LLM returns available resources
3. **resources/read**: LLM provides resource content
4. **tools/list**: LLM returns available tools
5. **tools/call**: LLM executes tool and returns result
6. **prompts/list**: LLM returns available prompt templates
7. **prompts/get**: LLM provides prompt template

**Action-Based Responses**:

```json
{
  "actions": [
    {
      "type": "mcp_initialize",
      "response": {
        "protocolVersion": "2024-11-05",
        "capabilities": {
          "resources": {"subscribe": true},
          "tools": {},
          "prompts": {}
        },
        "serverInfo": {"name": "netget-mcp", "version": "0.1.0"}
      }
    }
  ]
}
```

### Capability System

**Three Capability Categories**:

- **Resources** - Files, URLs, data sources (with optional subscriptions)
- **Tools** - Executable functions (like calculator, search)
- **Prompts** - Template prompts (like "code-review", "summarize")

Each capability declared during initialize, then implemented via LLM.

### Connection Management

- One HTTP POST endpoint (`/`) handles all JSON-RPC messages
- Sessions created on initialize, tracked in shared state
- Axum handles concurrent connections efficiently

## State Management

### Server State

```rust
McpServerState {
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    server_id: ServerId,
    protocol: Arc<McpProtocol>,
    sessions: Arc<Mutex<HashMap<String, Arc<Mutex<McpSession>>>>>,
    local_addr: SocketAddr,
}
```

### Per-Session State

```rust
McpSession {
    session_id: String,
    connection_id: ConnectionId,
    initialized: bool,
    capabilities: Value,  // Client capabilities
}
```

### Per-Connection Protocol Info

```rust
ProtocolConnectionInfo::Mcp {
    session_id: String,
    initialized: bool,
    capabilities: Value,
    subscriptions: HashSet<String>,
    tools: HashMap<String, Value>,
    resources: HashMap<String, Value>,
    prompts: HashMap<String, Value>,
}
```

## Limitations

### Not Implemented

- **WebSocket transport** - Only HTTP POST supported
- **SSE (Server-Sent Events)** - No server push notifications
- **Sampling** - LLM sampling API not exposed
- **Roots** - File system roots not implemented
- **Progress notifications** - Progress tracking incomplete
- **Cancellation** - Request cancellation not fully implemented

### Session Management

- **In-memory only** - Sessions lost on restart
- **No expiration** - Sessions never timeout
- **No cleanup** - Closed sessions remain in memory

### Resource Subscriptions

- **Tracking only** - No actual change notifications
- **No polling** - Server doesn't monitor resources

## Example Prompts and Responses

### Startup

```
Listen on port 8000 via MCP.

You are an MCP server that provides:

Resources:
- file:///README.md - Project documentation
- file:///config.json - Configuration file

Tools:
- calculate(expression: string) - Evaluate mathematical expressions
- search(query: string) - Search files

Prompts:
- code-review - Generate code review prompts
- summarize(text: string) - Generate summarization prompts

When initialized, declare all these capabilities.
```

### Network Event (Initialize)

**Received**:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "initialize",
  "params": {
    "protocolVersion": "2024-11-05",
    "capabilities": {},
    "clientInfo": {"name": "test-client", "version": "1.0.0"}
  }
}
```

**LLM Response**:

```json
{
  "actions": [
    {
      "type": "mcp_initialize",
      "response": {
        "protocolVersion": "2024-11-05",
        "capabilities": {
          "resources": {"subscribe": true},
          "tools": {},
          "prompts": {}
        },
        "serverInfo": {"name": "netget-mcp", "version": "0.1.0"}
      }
    }
  ]
}
```

### Network Event (Resources List)

**Received**:

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "resources/list"
}
```

**LLM Response**:

```json
{
  "actions": [
    {
      "type": "mcp_resources_list",
      "response": {
        "resources": [
          {
            "uri": "file:///README.md",
            "name": "README",
            "description": "Project documentation"
          }
        ]
      }
    }
  ]
}
```

### Network Event (Tools Call)

**Received**:

```json
{
  "jsonrpc": "2.0",
  "id": 6,
  "method": "tools/call",
  "params": {
    "name": "calculate",
    "arguments": {"expression": "2+2"}
  }
}
```

**LLM Response**:

```json
{
  "actions": [
    {
      "type": "mcp_tools_call",
      "response": {
        "content": [
          {"type": "text", "text": "4"}
        ]
      }
    }
  ]
}
```

## References

- [Model Context Protocol Specification](https://modelcontextprotocol.io/)
- [MCP TypeScript SDK](https://github.com/modelcontextprotocol/typescript-sdk)
- [axum Documentation](https://docs.rs/axum/)

## Key Design Principles

1. **Full MCP Compliance** - Implements MCP 2024-11-05 spec
2. **LLM Capability Control** - All capabilities defined by LLM
3. **Session-Based** - Proper session management per MCP spec
4. **Action-Based** - Uses standard NetGet action system
5. **JSON-RPC Foundation** - Built on JSON-RPC 2.0 substrate
