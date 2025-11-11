# gRPC Client Implementation

## Overview

gRPC client with dynamic protobuf schema support, where the LLM controls RPC method calls and interprets responses
through JSON. The client uses tonic for gRPC transport and prost-reflect for dynamic message handling.

## Protocol Version

- **gRPC**: HTTP/2 with Protocol Buffers
- **Protobuf**: proto3 syntax
- **Transport**: HTTP/2 with binary protobuf encoding

## Library Choices

### Core Dependencies

- **tonic** v0.12 - gRPC client library
    - Chosen for: Industry-standard Rust gRPC implementation
    - Used for: Channel management, request/response handling
- **prost-reflect** v0.14 - Dynamic protobuf message handling
    - Chosen for: Runtime schema loading, no code generation needed
    - Used for: Parsing/encoding protobuf without compilation
- **prost** v0.13 - Protocol Buffer implementation
    - Chosen for: FileDescriptorSet decoding, message encoding
    - Used for: Protobuf binary format handling
- **prost-types** v0.13 - Standard protobuf types
    - Chosen for: FileDescriptorSet type definition
- **base64** v0.22 - Base64 encoding/decoding
    - Chosen for: Schema transmission and bytes field encoding
    - Used for: FileDescriptorSet encoding and protobuf bytes type
- **tempfile** v3.13 - Temporary file management
    - Chosen for: Protoc compilation of inline .proto text
    - Used for: Creating temp files for protoc input

### Why Dynamic Schema Loading?

**Flexibility over Performance**:

- Allows LLM to connect to any gRPC service without code generation
- Enables runtime schema changes and prototyping
- Simplifies client usage (no protoc compilation step for users)
- Trade-off: Slower than compiled code, but sufficient for LLM-controlled clients

## Architecture Decisions

### Schema Input Formats

**Three Methods Supported** (same as server):

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

**Complete RPC Control** - LLM decides what methods to call:

1. **Startup**: LLM provides protobuf schema via `startup_params.proto_schema`
2. **Connected**: LLM sees list of available services
3. **Call**: LLM chooses service, method, and provides request as JSON
4. **Response**: LLM receives response as JSON, decides next action

**Action-Based Calls**:

```json
{
  "actions": [
    {
      "type": "call_grpc_method",
      "service": "calculator.Calculator",
      "method": "Add",
      "request": {"a": 5, "b": 3},
      "metadata": {"auth-token": "secret"}
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

- JSON → Protobuf: `json_to_dynamic_message()`, `json_to_proto_value()`
- Protobuf → JSON: `dynamic_message_to_json()`, `proto_value_to_json()`
- Handles all protobuf types: int32/64, string, bool, bytes (base64), enum, message, repeated, map

### Connection Management

- Single `Channel` created per client
- HTTP/2 connection pooling handled by tonic
- Connection tracked in client state with service metadata
- Persistent connection allows multiple RPC calls

### Error Handling

**gRPC Status Codes**:

- Success: Response converted to JSON, sent to LLM
- Error: Status code and message sent to LLM via `grpc_error` event
- LLM can decide how to handle errors (retry, log, etc.)

## State Management

### Client State

- **Connection State**: Idle/Processing/Accumulating (standard client pattern)
- **Protocol Data**:
    - `grpc_client`: "initialized" marker
    - `server_addr`: Server address for reference

### No Per-Call State

- Each RPC call is independent
- State machine prevents concurrent calls (Processing state)
- LLM memory tracks conversation across calls

## Limitations

### Not Implemented

- **Streaming RPCs** - Only unary (request/response) supported
    - No client streaming, server streaming, or bidirectional streaming
- **Compression** - gRPC compression not explicitly configured
- **Deadlines/Timeouts** - Uses tonic defaults, no custom timeout support
- **Retry policies** - No automatic retry handling (LLM must explicitly retry)
- **Load balancing** - Single endpoint, no client-side load balancing
- **Authentication** - No mTLS or advanced auth (metadata only)

### Schema Limitations

- **Protoc dependency** - .proto text/file formats require protoc in PATH
- **No schema validation** - LLM must provide valid protobuf schema
- **No reflection** - Client doesn't use gRPC reflection protocol to discover schema

### Performance Considerations

- **Dynamic dispatch overhead** - Slower than compiled gRPC clients
- **JSON serialization** - Extra conversion step vs. native protobuf
- **State machine** - Prevents concurrent calls, serializes all RPCs

## Example Prompts and Responses

### Startup (Base64 FileDescriptorSet)

```
Connect to gRPC server at localhost:50051. The protobuf schema is: CpUCCg9jYWxjdWxhdG9yLnByb3RvEgpjYWxjdWxhdG9yIikKCkFkZFJlcXVlc3QSCwoDYQgBIAEoBVIBYRILCgNiCAIgASgFUgFiIiIKC0FkZFJlc3BvbnNlEhMKBnJlc3VsdBgBIAEoBVIGcmVzdWx0MkIKCkNhbGN1bGF0b3ISNAoDQWRkEhYuY2FsY3VsYXRvci5BZGRSZXF1ZXN0Gh0uY2FsY3VsYXRvci5BZGRSZXNwb25zZSIAYgZwcm90bzM=

When connected, call the Add method with a=5 and b=3.
```

### Startup (Inline Proto Text)

```
Connect to gRPC server at localhost:50051 with this schema:

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

Call Add with a=10, b=20.
```

### Connected Event

**Received**:

```json
{
  "event_type": "grpc_connected",
  "server_addr": "localhost:50051",
  "services": ["calculator.Calculator"]
}
```

**LLM Response**:

```json
{
  "actions": [
    {
      "type": "call_grpc_method",
      "service": "calculator.Calculator",
      "method": "Add",
      "request": {"a": 5, "b": 3}
    }
  ]
}
```

### Response Event

**Received**:

```json
{
  "event_type": "grpc_response_received",
  "service": "calculator.Calculator",
  "method": "Add",
  "response": {"result": 8}
}
```

**LLM Response**:

```json
{
  "actions": [
    {
      "type": "show_message",
      "message": "Addition result: 5 + 3 = 8"
    },
    {
      "type": "disconnect"
    }
  ]
}
```

### Error Event

**Received**:

```json
{
  "event_type": "grpc_error",
  "service": "calculator.Calculator",
  "method": "Add",
  "code": "INVALID_ARGUMENT",
  "message": "Both a and b must be positive integers"
}
```

**LLM Response**:

```json
{
  "actions": [
    {
      "type": "show_message",
      "message": "Server returned error: INVALID_ARGUMENT - Both a and b must be positive integers"
    },
    {
      "type": "call_grpc_method",
      "service": "calculator.Calculator",
      "method": "Add",
      "request": {"a": 10, "b": 20}
    }
  ]
}
```

## System Dependencies

### macOS Setup

**gRPC client on macOS**: Mostly pure Rust, but has **optional protoc dependency** for inline .proto text format.

**Minimum Setup (No dependencies)**:

```bash
# Build with base64-encoded FileDescriptorSet (recommended, no deps)
./cargo-isolated.sh build --no-default-features --features grpc

# This works if you provide schema as base64:
netget> Connect to gRPC server at localhost:50051
# Then provide proto schema as base64-encoded FileDescriptorSet
```

**Optional: Install protoc for .proto text support**:

```bash
# If you want to use inline .proto text format:
brew install protobuf

# Verify installation
protoc --version
```

**Why three schema formats?**

1. **Base64 FileDescriptorSet** (recommended) - No dependencies, fastest
2. **.proto file path** - Requires protoc, useful for development
3. **Inline .proto text** - Requires protoc, most flexible for LLM generation

### Linux Setup

**Optional protoc installation**:

```bash
# Debian/Ubuntu
sudo apt-get install protobuf-compiler

# Fedora/RHEL
sudo dnf install protobuf-compiler

# Alpine
apk add protobuf-dev

# Arch
sudo pacman -S protobuf
```

### Troubleshooting

**"protoc not found" error**:

- Use base64-encoded FileDescriptorSet format instead (no protoc needed)
- Or install protoc: `brew install protobuf`

**"Invalid protobuf schema" error**:

- Ensure proto definition is valid proto3 syntax
- Use online protoc compiler to test: https://protoc-web.appspot.com/

**"Cannot connect to gRPC server"**:

- Verify server address is correct and server is running
- Check firewall settings allow connection to gRPC port
- Ensure proto schema matches server implementation

## References

- [gRPC Protocol Specification](https://github.com/grpc/grpc/blob/master/doc/PROTOCOL-HTTP2.md)
- [Protocol Buffers Language Guide](https://protobuf.dev/programming-guides/proto3/)
- [tonic gRPC Library](https://github.com/hyperium/tonic)
- [prost-reflect Documentation](https://docs.rs/prost-reflect/)

## Key Design Principles

1. **Dynamic Schema** - LLM provides schema, no code generation
2. **JSON Bridge** - LLM sees JSON, not binary protobuf
3. **Full Call Control** - LLM decides which methods to call
4. **Metadata Support** - LLM can set gRPC metadata (headers)
5. **Action-Based** - Uses standard NetGet action system for calls
6. **State Machine** - Prevents concurrent calls, serializes RPCs for LLM

## Comparison with Server

| Aspect             | Server                            | Client                               |
|--------------------|-----------------------------------|--------------------------------------|
| **Schema**         | LLM provides (implements service) | LLM provides (calls service)         |
| **Role**           | Responds to RPC calls             | Initiates RPC calls                  |
| **Connection**     | Accepts incoming                  | Connects outbound                    |
| **Control Flow**   | Reactive (waits for calls)        | Proactive (LLM decides when to call) |
| **Metadata**       | Receives from client              | Sends to server                      |
| **Error Handling** | LLM returns error codes           | LLM receives error codes             |
