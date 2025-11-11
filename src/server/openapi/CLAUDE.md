# OpenAPI 3.1 Server Implementation

## Overview

OpenAPI 3.1 spec-driven HTTP server where the LLM provides an OpenAPI specification and generates responses based on
validated requests. Supports route matching, path parameters, request validation, and intentionally non-compliant
responses for testing/honeypot purposes.

## Protocol Version

- **OpenAPI**: 3.1.0 (compatible with 3.0.x)
- **Transport**: HTTP/1.1 with JSON request/response bodies
- **Specification**: https://spec.openapis.org/oas/v3.1.0.html

## Library Choices

### Core Dependencies

- **openapi-rs** v1.0 - OpenAPI 3.x parser
    - Chosen for: Native Rust OpenAPI parsing, no code generation
    - Used for: Parsing YAML/JSON specs
- **matchit** v0.8 - Fast path router with parameter extraction
    - Chosen for: High performance, path template support (`/users/{id}`)
    - Used for: Route matching against OpenAPI paths
- **hyper** v1 - HTTP/1.1 server
- **serde_json** - JSON handling

### Why Not Use an OpenAPI Server Framework?

- No Rust framework provides LLM-controlled responses
- Need full control over spec compliance vs. intentional violations
- OpenAPI validation libraries exist but don't fit dynamic LLM responses

## Architecture Decisions

### Two Operating Modes

**1. With Spec (Loaded)** - Fast path:

- LLM provides OpenAPI spec during startup via `startup_params.spec`
- Server builds route matcher from paths
- Invalid requests (404/405/400) rejected immediately (no LLM call)
- Matched requests receive only relevant operation spec, not full spec

**2. Without Spec (Dynamic)** - Flexible mode:

- Server starts without spec
- First request calls LLM, which can load spec via `reload_spec` action
- All requests call LLM (including 404/405)

### Route Matching System

**Fast Router with matchit**:

```rust
Router<RouteMetadata>  // Maps "METHOD:PATH" -> operation metadata
```

**Match Results**:

- `Found` - Route exists, extract path params
- `MethodNotAllowed` - Path exists but wrong method (405)
- `NotFound` - Path doesn't exist (404)

### LLM Control Points

**Spec-Driven + LLM Responses**:

1. **Startup**: LLM provides OpenAPI spec (optional)
2. **Request Validation**: matchit validates path/method (if spec loaded)
3. **Response Generation**: LLM generates response based on operation
4. **Compliance Control**: LLM can intentionally violate spec (for testing)

**Action-Based Responses**:

```json
{
  "actions": [
    {
      "type": "send_openapi_response",
      "status_code": 200,
      "headers": {"Content-Type": "application/json"},
      "body": "{\"todos\": [...]}"
    }
  ]
}
```

Or for spec loading:

```json
{
  "actions": [
    {
      "type": "load_openapi_spec",
      "spec": "openapi: 3.1.0\n..."
    }
  ]
}
```

### Error Handling Configuration

**`llm_on_invalid` Flag**:

- `false` (default) - Immediate 404/405/400 without LLM call (fast)
- `true` - LLM handles all errors (flexible, allows custom error responses)

Configurable via `configure_error_handling` action.

### Connection Management

- Each HTTP connection spawned as tokio task
- Connections tracked in `ProtocolConnectionInfo::OpenApi` with operation metadata
- Route matching happens per-request

## State Management

### Server State

```rust
OpenApiState {
    spec: Option<String>,              // Raw YAML/JSON spec
    spec_valid: bool,                  // Parsing succeeded
    parsed_spec: Option<OpenAPI>,      // Parsed structure
    router: Option<Router<RouteMetadata>>,  // Route matcher
    llm_on_invalid: bool,              // Error handling mode
}
```

### Per-Connection State

```rust
ProtocolConnectionInfo::OpenApi {
    operation_id: Option<String>,  // Matched operation
    method: Option<String>,        // HTTP method
    path: Option<String>,          // Request path
    validated: bool,               // Validation passed
}
```

## Limitations

### Not Implemented

- **Request body validation** - Schema validation not enforced
- **Response validation** - LLM can return non-compliant responses
- **Parameter validation** - Types/formats not checked
- **Content negotiation** - Accept header not processed
- **OAuth/API key auth** - No authentication layer
- **Multipart/form-data** - Only JSON supported

### Schema Validation

Currently a no-op - LLM trusted to generate valid responses. Future enhancement: use `jsonschema` crate for validation.

### Performance Considerations

- **Route matching overhead** - matchit is fast but still per-request
- **Spec parsing** - YAML parsing on startup (5-50ms depending on spec size)
- **No caching** - LLM generates fresh response each time

## Example Prompts and Responses

### Startup (Inline Spec)

```
open_server port 3000 base_stack openapi.

Create an OpenAPI 3.1 server for a TODO API:

openapi: 3.1.0
info:
  title: TODO API
  version: 1.0.0
paths:
  /todos:
    get:
      operationId: listTodos
      responses:
        '200':
          description: List of todos
          content:
            application/json:
              schema:
                type: array
                items:
                  type: object
                  properties:
                    id: {type: integer}
                    title: {type: string}
                    done: {type: boolean}

When GET /todos is requested, return 3 sample todos.
```

### Startup (Spec File)

```
open_server port 3000 base_stack openapi.
Load OpenAPI spec from /path/to/openapi.yaml
```

### Network Event (Matched Request)

**Event to LLM**:

```json
{
  "event_type": "openapi_request",
  "method": "GET",
  "path": "/todos",
  "uri": "/todos?completed=false",
  "headers": {"accept": "application/json"},
  "body": "",
  "spec_info": {"spec_loaded": true, "spec_valid": true},
  "matched_route": {
    "operation_id": "listTodos",
    "path_template": "/todos",
    "path_params": {},
    "operation": {
      "operationId": "listTodos",
      "responses": {...}
    }
  }
}
```

**LLM Response**:

```json
{
  "actions": [
    {
      "type": "send_openapi_response",
      "status_code": 200,
      "headers": {"Content-Type": "application/json"},
      "body": "[{\"id\":1,\"title\":\"Buy milk\",\"done\":false}]"
    }
  ]
}
```

### Intentional Spec Violation

**Event to LLM** (spec says 200, but LLM violates intentionally):

```json
{
  "event_type": "openapi_request",
  "method": "GET",
  "path": "/todos",
  ...
}
```

**LLM Response** (returns 201 instead of 200 for testing):

```json
{
  "actions": [
    {
      "type": "send_openapi_response",
      "status_code": 201,
      "headers": {"Content-Type": "application/json"},
      "body": "[...]"
    }
  ]
}
```

This allows testing client error handling.

## References

- [OpenAPI 3.1 Specification](https://spec.openapis.org/oas/v3.1.0.html)
- [matchit Router](https://docs.rs/matchit/)
- [openapi-rs Parser](https://docs.rs/openapi-rs/)

## Key Design Principles

1. **Spec-Driven** - OpenAPI spec defines structure
2. **LLM Control** - LLM generates all responses
3. **Fast Validation** - Route matching without LLM (when spec loaded)
4. **Intentional Violations** - LLM can break spec for testing
5. **Dynamic Loading** - Spec can be loaded at startup or later
