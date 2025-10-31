# gRPC Server Implementation

## Overview

gRPC server with dynamic protobuf schema support, where the LLM provides the schema definition and controls all RPC request/response handling through JSON. Implements a meta-protocol where the LLM becomes the RPC service implementation.

## Protocol Version

- **gRPC**: HTTP/2 with Protocol Buffers
- **Protobuf**: proto3 syntax
- **Transport**: HTTP/2 with binary protobuf encoding

## Library Choices

### Core Dependencies
- **prost-reflect** v0.14 - Dynamic protobuf message handling
  - Chosen for: Runtime schema loading, no code generation needed
  - Used for: Parsing/encoding protobuf without compilation
- **prost** v0.13 - Protocol Buffer implementation
  - Chosen for: FileDescriptorSet decoding, message encoding
  - Used for: Protobuf binary format handling
- **prost-types** - Standard protobuf types
  - Chosen for: FileDescriptorSet type definition
- **tonic-reflection** v0.12 - gRPC server reflection
  - Chosen for: gRPC reflection protocol support
  - Used for: Allow clients to discover service schema at runtime
- **hyper** v1 - HTTP/2 server
  - Chosen for: HTTP/2 support required for gRPC
  - Used for: Connection handling and HTTP/2 framing
- **base64** - Base64 encoding/decoding
  - Chosen for: Schema transmission in prompts
  - Used for: FileDescriptorSet encoding

### Why Dynamic Schema Loading?
**Flexibility over Performance**:
- Allows LLM to define arbitrary services without code generation
- Enables runtime schema changes and prototyping
- Simplifies testing (no protoc compilation step for users)
- Trade-off: Slower than compiled code, but sufficient for LLM-controlled services

## Architecture Decisions

### Schema Input Formats
**Three Methods Supported**:
1. **Base64-encoded FileDescriptorSet** (recommended)
   - No protoc dependency
   - LLM provides pre-compiled descriptor
   - Fastest startup
2. **.proto file path**
   - Requires protoc in PATH
   - Useful for development with existing schemas
3. **Inline .proto text**
   - Requires protoc in PATH
   - LLM provides raw proto definition
   - Most flexible for LLM generation

### LLM Control Points
**Complete RPC Control** - LLM handles all service logic:
1. **Startup**: LLM provides protobuf schema via `startup_params.proto_schema`
2. **Request**: Parse protobuf → convert to JSON → send to LLM
3. **Response**: LLM returns JSON → convert to protobuf → encode response

**Action-Based Responses**:
```json
{
  "actions": [
    {
      "type": "grpc_unary_response",
      "message": {"id": 123, "name": "Alice", "email": "alice@example.com"}
    }
  ]
}
```

### Dynamic Message Handling
**Runtime Type System**:
- Use `DescriptorPool` to store parsed schema
- Find service/method descriptors by name
- Create `DynamicMessage` instances for request/response
- Convert protobuf ↔ JSON using custom serialization

**JSON Conversion**:
- Protobuf Value → JSON: `proto_value_to_json()`
- JSON → Protobuf Value: `json_to_proto_value()`
- Handles all protobuf types: int32/64, string, bool, bytes (base64), enum, message, repeated, map

### Connection Management
- Each gRPC connection spawned as tokio task
- HTTP/2 handled by hyper's `http2::Builder`
- Connection tracked in `ProtocolConnectionInfo::Grpc` with service/method metadata
- gRPC framing: 5-byte header (1 byte compression + 4 bytes length) + payload

### Error Handling
**gRPC Status Codes**:
- HTTP 200 + `grpc-status: 0` for success
- HTTP 200 + `grpc-status: 13` for internal errors
- LLM can return `grpc_error` action with custom status codes

## State Management

### Per-Connection State
```rust
ProtocolConnectionInfo::Grpc {
    service_name: String,       // e.g., "test.UserService"
    method_name: String,        // e.g., "GetUser"
    metadata: HashMap<String, String>,  // gRPC metadata (headers)
}
```

### Server State
- **Descriptor Pool**: Cached schema for message parsing
- **Router**: Not needed (dynamic dispatch by service/method name)

## Limitations

### Not Implemented
- **Streaming RPCs** - Only unary (request/response) supported
  - No client streaming, server streaming, or bidirectional streaming
- **Compression** - gRPC compression flag ignored
- **Deadlines/Timeouts** - gRPC deadline metadata not enforced
- **Retry policies** - No automatic retry handling
- **Load balancing** - Single server instance only
- **Authentication** - No mTLS or token-based auth

### Schema Limitations
- **No reflection of LLM behavior** - Schema defines structure but not LLM logic
- **Protoc dependency** - .proto text/file formats require protoc in PATH
- **No schema validation** - LLM must provide valid protobuf schema
- **No proto3 optionals** - May not handle all proto3 features correctly

### Performance Considerations
- **Dynamic dispatch overhead** - Slower than compiled gRPC services
- **JSON serialization** - Extra conversion step vs. native protobuf
- **No connection pooling** - Each request creates new connection to LLM

## Example Prompts and Responses

### Startup (Inline Proto Text)
```
Start a gRPC server on port 50051. Here is the protobuf schema:

syntax = "proto3";
package calculator;

service Calculator {
  rpc Add(AddRequest) returns (AddResponse);
}

message AddRequest {
  int32 a = 1;
  int32 b = 2;
}

message AddResponse {
  int32 result = 1;
}

When you receive Add requests, return the sum of a and b.
```

### Startup (Base64 FileDescriptorSet)
```
Start a gRPC server on port 50051. The protobuf schema is provided as base64-encoded FileDescriptorSet: CpUCCg9jYWxjdWxhdG9yLnByb3RvEgpjYWxjdWxhdG9yIikKCkFkZFJlcXVlc3QSCwoDYQgBIAEoBVIBYRILCgNiCAIgASgFUgFiIiIKC0FkZFJlc3BvbnNlEhMKBnJlc3VsdBgBIAEoBVIGcmVzdWx0MkIKCkNhbGN1bGF0b3ISNAoDQWRkEhYuY2FsY3VsYXRvci5BZGRSZXF1ZXN0Gh0uY2FsY3VsYXRvci5BZGRSZXNwb25zZSIAYgZwcm90bzM=

When you receive Add requests, return the sum of a and b.
```

### Network Event (Unary RPC)
**Received**:
```json
{
  "event_type": "grpc_unary_request",
  "service": "calculator.Calculator",
  "method": "Add",
  "request": {"a": 5, "b": 3},
  "expected_response_schema": {
    "type": "object",
    "fields": {
      "result": {"type": "int32", "cardinality": "optional"}
    }
  }
}
```

**LLM Response**:
```json
{
  "actions": [
    {
      "type": "show_message",
      "message": "Calculating 5 + 3 = 8"
    },
    {
      "type": "grpc_unary_response",
      "message": {"result": 8}
    }
  ]
}
```

### Error Response
**LLM Response**:
```json
{
  "actions": [
    {
      "type": "grpc_error",
      "code": "INVALID_ARGUMENT",
      "message": "Both a and b must be positive integers"
    }
  ]
}
```

## References

- [gRPC Protocol Specification](https://github.com/grpc/grpc/blob/master/doc/PROTOCOL-HTTP2.md)
- [Protocol Buffers Language Guide](https://protobuf.dev/programming-guides/proto3/)
- [prost-reflect Documentation](https://docs.rs/prost-reflect/)
- [tonic gRPC Library](https://github.com/hyperium/tonic)

## Key Design Principles

1. **Dynamic Schema** - LLM provides schema, no code generation
2. **JSON Bridge** - LLM sees JSON, not binary protobuf
3. **Full Service Control** - LLM implements entire RPC service
4. **Reflection Support** - Clients can discover schema at runtime
5. **Action-Based** - Uses standard NetGet action system for responses
