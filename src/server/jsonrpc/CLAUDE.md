# JSON-RPC 2.0 Server Implementation

## Overview

JSON-RPC 2.0 server over HTTP POST where the LLM controls all RPC method execution and response generation. Supports
single requests, batch requests, and notifications per the JSON-RPC 2.0 specification.

## Protocol Version

- **JSON-RPC**: 2.0 (https://www.jsonrpc.org/specification)
- **Transport**: HTTP/1.1 POST with JSON request/response bodies
- **Content-Type**: `application/json`

## Library Choices

### Core Dependencies

- **hyper** (v1) - HTTP/1.1 server implementation
    - Chosen for: async/await support, efficient connection handling
    - Used for: HTTP request/response processing
- **serde_json** - JSON serialization/deserialization
    - Chosen for: Standard Rust JSON library
    - Used for: Parsing JSON-RPC requests and building responses
- **tokio** - Async runtime
    - Chosen for: Concurrent connection handling

### Why No JSON-RPC Library?

- JSON-RPC 2.0 specification is simple (request/response format)
- Direct implementation provides full control over LLM integration
- No suitable Rust library for *server-side* JSON-RPC 2.0 with LLM control

## Architecture Decisions

### Request Types

**Three JSON-RPC Message Types**:

1. **Single Request** - Object with `jsonrpc`, `method`, `params`, `id`
    - Expects response with matching `id`
2. **Batch Request** - Array of request objects
    - Returns array of responses (order not guaranteed per spec)
3. **Notification** - Request without `id` field (or `id: null`)
    - No response expected (HTTP 204 No Content)

### LLM Control Points

**Complete Method Control** - LLM implements all RPC methods:

1. **Method Call**: Parse JSON-RPC request → send to LLM
2. **LLM Decision**: Implement method logic or return error
3. **Response Generation**: LLM returns JSON-RPC success or error

**Action-Based Responses**:

```json
{
  "actions": [
    {
      "type": "jsonrpc_success",
      "result": {"sum": 8},
      "id": 1
    }
  ]
}
```

Or for errors:

```json
{
  "actions": [
    {
      "type": "jsonrpc_error",
      "code": -32601,
      "message": "Method not found",
      "id": 1
    }
  ]
}
```

### Error Code Handling

**Standard JSON-RPC 2.0 Error Codes**:

- `-32700` - Parse error (invalid JSON)
- `-32600` - Invalid Request (malformed JSON-RPC)
- `-32601` - Method not found
- `-32602` - Invalid params
- `-32603` - Internal error
- `-32000 to -32099` - Server error (reserved range)

LLM can return any error code with custom message and optional `data` field.

### Connection Management

- Each HTTP connection spawned as separate tokio task
- Connections tracked in `ProtocolConnectionInfo::JsonRpc` with `recent_methods` Vec
- HTTP/1.1 keep-alive handled by hyper
- No session state (each request is independent)

### Batch Request Processing

**Sequential Execution**:

- Process each request in batch sequentially
- Collect responses in array
- Notifications in batch produce no response entry
- Empty batch returns error (per spec)

## State Management

### Per-Connection State

```rust
ProtocolConnectionInfo::JsonRpc {
    recent_methods: Vec<String>,  // Track last 10 method calls
}
```

### No Session State

- Each JSON-RPC call is stateless
- No method call history maintained across requests
- Methods cannot access previous call results

## Limitations

### Not Implemented

- **Transport negotiation** - Only HTTP POST supported (no WebSocket, TCP, etc.)
- **Authentication** - No API key or token validation
- **Rate limiting** - No request throttling
- **Method discovery** - No standard way to list available methods
- **JSON-RPC 1.0 compatibility** - Only version 2.0 supported

### Specification Deviations

- **Response order** - Batch responses may not match request order
    - Spec allows this, but some clients expect order preservation
- **Notification handling** - Returns 204 instead of 200 with empty body
    - Both are acceptable per HTTP, but non-standard for JSON-RPC

### LLM Interpretation Challenges

- **Error code selection** - LLM must choose appropriate error codes
- **Type handling** - JSON types must match method expectations
- **Batch complexity** - LLM sees each batch item individually

## Example Prompts and Responses

### Startup

```
open_server port 8080 base_stack jsonrpc. This is a JSON-RPC 2.0 server.

Implement these methods:
- add(a, b): Return the sum of a and b
- greet(name): Return "Hello, {name}!"
- version(): Return "1.0.0"

For unknown methods, return error code -32601 (Method not found).
```

### Network Event (Single Request)

**Received**:

```json
{
  "jsonrpc": "2.0",
  "method": "add",
  "params": [5, 3],
  "id": 1
}
```

**LLM Response**:

```json
{
  "actions": [
    {
      "type": "show_message",
      "message": "Calculating 5 + 3"
    },
    {
      "type": "jsonrpc_success",
      "result": 8,
      "id": 1
    }
  ]
}
```

**Client Receives**:

```json
{
  "jsonrpc": "2.0",
  "result": 8,
  "id": 1
}
```

### Network Event (Notification)

**Received**:

```json
{
  "jsonrpc": "2.0",
  "method": "log_event",
  "params": {"event": "user_login", "user_id": 123}
}
```

**LLM Response**:

```json
{
  "actions": [
    {
      "type": "show_message",
      "message": "Logged event: user_login for user 123"
    }
  ]
}
```

**Client Receives**: HTTP 204 No Content (no body)

### Network Event (Batch Request)

**Received**:

```json
[
  {"jsonrpc": "2.0", "method": "add", "params": [1, 2], "id": 1},
  {"jsonrpc": "2.0", "method": "greet", "params": ["Alice"], "id": 2},
  {"jsonrpc": "2.0", "method": "unknown", "params": [], "id": 3}
]
```

**LLM Processes Each Individually** (3 separate LLM calls)

**Client Receives**:

```json
[
  {"jsonrpc": "2.0", "result": 3, "id": 1},
  {"jsonrpc": "2.0", "result": "Hello, Alice!", "id": 2},
  {"jsonrpc": "2.0", "error": {"code": -32601, "message": "Method not found"}, "id": 3}
]
```

### Error Response

**Received**:

```json
{
  "jsonrpc": "2.0",
  "method": "divide",
  "params": [10, 0],
  "id": 4
}
```

**LLM Response**:

```json
{
  "actions": [
    {
      "type": "jsonrpc_error",
      "code": -32000,
      "message": "Division by zero",
      "data": {"dividend": 10, "divisor": 0},
      "id": 4
    }
  ]
}
```

**Client Receives**:

```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32000,
    "message": "Division by zero",
    "data": {"dividend": 10, "divisor": 0}
  },
  "id": 4
}
```

## References

- [JSON-RPC 2.0 Specification](https://www.jsonrpc.org/specification)
- [JSON Schema](https://json-schema.org/)

## Key Design Principles

1. **Strict Spec Compliance** - Follows JSON-RPC 2.0 exactly
2. **LLM Method Implementation** - All business logic in LLM
3. **Stateless Design** - No cross-request state
4. **Error Code Precision** - Uses standard error codes
5. **Batch Support** - Handles single and batch requests uniformly
