# HTTP/2 Protocol Implementation

## Overview
HTTP/2 server implementing RFC 7540 (HTTP/2) using the hyper library. The LLM controls HTTP/2 responses (status codes, headers, body) while hyper handles the protocol parsing, connection management, multiplexing, and header compression.

**Status**: Experimental (Core Protocol)
**RFC**: RFC 7540 (HTTP/2)

## Library Choices
- **hyper v1.0** - Modern async HTTP library with HTTP/2 support
- **hyper-util** - Additional utilities for Tokio integration
- **http-body-util** - Body handling utilities (Full, Empty, Incoming)
- **http_common** - Shared internal module with HTTP/HTTP2 common logic (request extraction, response building)
- **Manual response construction** - LLM generates status, headers, body

**Rationale**: Hyper is the de-facto standard for HTTP in Rust. For HTTP/2, it handles:
- HTTP/2 protocol framing and binary encoding
- Request header decompression (HPACK)
- Connection multiplexing (multiple concurrent streams)
- Flow control and stream prioritization
- Server connection preface and settings negotiation
- Async I/O with Tokio integration

The LLM focuses on application logic (routing, business logic, response generation) rather than protocol implementation details.

## Architecture Decisions

### 1. Request-Response Model
HTTP/2 follows the same request-response pattern as HTTP/1.1:
- Each request is handled independently by hyper
- LLM generates response based on request (method, URI, headers, body)
- Multiple requests can be processed concurrently over a single TCP connection (multiplexing)
- No async actions - HTTP/2 is purely reactive to client requests

### 2. Hyper Service Pattern with HTTP/2
Hyper uses the same service pattern for HTTP/2:
- `service_fn()` creates a closure that handles each request
- Each request gets a fresh LLM call to generate the response
- Service is cloned for each connection (cheap clone via Arc)
- HTTP/2 uses `http2::Builder` instead of `http1::Builder`
- Requires `TokioExecutor` for async runtime integration

### 3. Connection Tracking
HTTP/2 connections are tracked per-TCP-connection:
- One `ConnectionId` per TCP connection (not per HTTP/2 stream)
- `ProtocolConnectionInfo::Http2` tracks recent requests on the connection
- Multiple HTTP/2 streams can occur on the same TCP connection (standard HTTP/2 behavior)
- Connection closed when TCP socket closes

### 4. Body Handling
Request bodies are fully buffered before LLM processing:
- `req.into_body().collect().await` reads entire body into memory
- Bodies are converted to UTF-8 strings for LLM (or "binary" marker for non-UTF8)
- Response bodies are wrapped in `Full<Bytes>` for hyper
- No streaming body support (LLM sees complete request, generates complete response)

### 5. Header Compression
HTTP/2 uses HPACK header compression (automatic):
- Hyper handles HPACK compression/decompression transparently
- Headers extracted and provided to LLM as JSON object (same as HTTP/1.1)
- All headers converted to lowercase keys
- Non-UTF8 header values are skipped (rare edge case)
- LLM can specify response headers in same JSON format

### 6. Dual Logging
All HTTP/2 operations use dual logging:
- **DEBUG**: Request summary (method, URI, version, body size)
- **TRACE**: Full request details (all headers, pretty-printed JSON body)
- Both go to `netget.log` (via tracing) and TUI Status panel (via status_tx)

### 7. Error Handling
LLM errors result in 500 Internal Server Error:
- If LLM call fails, return 500 with "Internal Server Error" body
- If LLM response is invalid, return 500
- Hyper connection errors (protocol violations) close connection automatically

### 8. Shared http_common Module
HTTP and HTTP/2 share common implementation logic via `src/server/http_common/`:
- **handler.rs** - Request extraction (headers, body, logging) and response building
- **actions.rs** - Shared action execution (`execute_http_response_action`)
- Reduces code duplication (~60% of logic shared between HTTP/HTTP2)
- Maintains protocol-specific boundaries (separate modules, keywords, events)
- Future HTTP/2-specific features (server push) remain in HTTP/2 module

**Benefits**:
- Single source of truth for request/response logic
- Consistent logging and error handling
- Easier maintenance (bug fixes apply to both protocols)
- Clear separation: shared logic in `http_common`, protocol-specific in `http`/`http2`

## LLM Integration

### Action-Based Response Model
The LLM responds to HTTP/2 events with actions:

**Events**:
- `http2_request` - HTTP/2 request received from client
  - Parameters: `method`, `uri`, `version`, `headers`, `body`

**Available Actions**:
- `send_http2_response` - Send HTTP/2 response (status, headers, body)
- `push_resource` - Push resource to client proactively (server push - limited implementation)
- Common actions: `show_message`, `update_instruction`, etc.
- **No async actions** - HTTP/2 is purely request-response

### Example LLM Response
```json
{
  "actions": [
    {
      "type": "send_http2_response",
      "status": 200,
      "headers": {
        "Content-Type": "application/json",
        "X-Custom-Header": "value"
      },
      "body": "{\"message\": \"Hello from HTTP/2!\"}"
    },
    {
      "type": "show_message",
      "message": "Served GET / via HTTP/2"
    }
  ]
}
```

### Response Format
The `send_http2_response` action returns structured data:
```json
{
  "status": 200,
  "headers": {"Content-Type": "application/json"},
  "body": "{\"data\": \"value\"}"
}
```

This is serialized to `ActionResult::Output` and parsed by the request handler to construct the actual hyper `Response`.

### Default Response
If LLM doesn't provide a response or response parsing fails:
- Status: 200 OK
- Headers: empty
- Body: empty string

This ensures the server always responds (no hanging connections).

## Connection Management

### Connection Lifecycle
1. **Accept**: `TcpListener::accept()` creates new TCP connection
2. **Register**: Connection added to `ServerInstance` with `ProtocolConnectionInfo::Http2`
3. **Serve**: Hyper's `http2::Builder::new().serve_connection()` handles HTTP/2 protocol
4. **Request Loop**: Multiple concurrent requests on the connection (HTTP/2 multiplexing)
5. **LLM Call**: Service function calls LLM to generate response for each request
6. **Close**: Connection removed when TCP socket closes (client disconnect or error)

### Connection Data Structure
```rust
ProtocolConnectionInfo::Http2 {
    recent_requests: Vec<(String, String, Instant)>, // method, path, time
}
```

Unlike TCP, no write_half or queued_data - hyper manages the socket internally.

### State Updates
- Connection state tracked in `ServerInstance.connections`
- Each request increments `packets_received` and `bytes_received`
- Each response increments `packets_sent` and `bytes_sent`
- `last_activity` updated on each request
- UI updates via `__UPDATE_UI__` message

## HTTP/2 Features

### 1. Multiplexing
- Multiple concurrent requests over single TCP connection
- Hyper handles stream management automatically
- Each request processed independently in separate async task
- No head-of-line blocking (unlike HTTP/1.1 pipelining)

### 2. Header Compression
- HPACK compression reduces header overhead
- Dynamic table maintained per connection
- Hyper handles compression/decompression transparently
- LLM sees decompressed headers as JSON

### 3. Binary Framing
- HTTP/2 uses binary framing layer (not text-based like HTTP/1.1)
- Hyper handles frame encoding/decoding
- LLM interacts with high-level request/response objects

### 4. Flow Control
- HTTP/2 uses per-stream and per-connection flow control
- Hyper manages flow control windows automatically
- Prevents fast sender from overwhelming slow receiver

### 5. Stream Prioritization
- HTTP/2 supports request prioritization
- Hyper handles priority frames
- Current implementation doesn't expose prioritization to LLM

### 6. Server Push (Limited Implementation)
- HTTP/2 supports server-initiated push of resources
- `push_resource` action defined but not fully functional
- Requires connection-level access (not available in service pattern)
- Hyper supports server push via extended connection API
- Future: Restructure to use connection-level handler for full push support

## Known Limitations

### 1. No TLS Support (Cleartext HTTP/2)
- Raw HTTP/2 only (h2c - HTTP/2 over cleartext TCP)
- Most browsers require HTTP/2 over TLS (h2)
- For production, would need to wrap listener with rustls
- See future enhancement section for HTTPS implementation plan

### 2. Server Push Partially Implemented
- `push_resource` action defined but doesn't execute actual pushes
- Service pattern doesn't provide connection-level access needed for pushes
- Logs warning when push is requested
- Full implementation requires:
  - Restructure HTTP/2 handler to use connection API instead of service pattern
  - Manage push streams manually
  - Store pending pushes and send via connection object

### 3. No Streaming
- Request bodies fully buffered before LLM processing
- Response bodies fully generated before sending
- No support for chunked responses generated incrementally
- Large requests/responses may exhaust memory

### 4. No Connection Pooling Control
- Hyper manages connection persistence automatically
- No LLM control over connection lifecycle
- No way to force close after response (hyper decides)

### 5. Limited Header Control
- LLM can set custom headers, but hyper may add/modify:
  - `:status` pseudo-header (HTTP/2 status)
  - `:path`, `:method`, `:scheme`, `:authority` (HTTP/2 pseudo-headers)
  - `content-length` (calculated automatically)
- No way to prevent hyper's automatic headers

### 6. No Stream Prioritization Control
- HTTP/2 stream prioritization handled by hyper
- No LLM control over stream priorities
- No way to influence response ordering

### 7. No Upgrade from HTTP/1.1
- Server only speaks HTTP/2 directly (h2c)
- No support for HTTP/1.1 Upgrade to HTTP/2
- Clients must connect with HTTP/2 directly

## Example Prompts

### Simple JSON API
```
listen on port 8080 via http2
For GET /, return JSON: {"message": "Hello HTTP/2"}
For GET /api/users, return JSON array of users
For POST /api/users, parse JSON body and return 201
```

### REST API with Multiplexing
```
listen on port 3000 via http/2
For GET /products, return list of products as JSON
For GET /products/:id, return single product as JSON
For POST /products, create new product and return 201
For PUT /products/:id, update product and return 200
For DELETE /products/:id, delete product and return 204
```

### Custom Headers and Status Codes
```
listen on port 8080 via http2
For GET /health, return 200 with body: OK
For GET /metrics, return 200 with Content-Type: text/plain
For POST /data, return 201 with X-Request-ID header
For any 404, return JSON error message
```

### gRPC-like Binary API
```
listen on port 50051 via http/2
For POST /service.Method, parse protobuf-like JSON and return response
Use Content-Type: application/grpc+json
Return appropriate gRPC status codes
```

## Performance Characteristics

### Latency
- One LLM call per HTTP/2 request (same as HTTP/1.1)
- Typical latency: 2-5 seconds per request with qwen3-coder:30b
- Multiplexing allows concurrent requests without additional TCP handshakes

### Throughput
- Limited by LLM response time (2-5s per request)
- Concurrent requests processed in parallel (each on separate hyper stream)
- Single TCP connection can handle many concurrent requests (multiplexing benefit)

### Concurrency
- Unlimited concurrent connections (bounded by system resources)
- Each connection supports unlimited concurrent streams (HTTP/2 multiplexing)
- Ollama lock serializes LLM API calls across all connections/streams

### Memory Usage
- Each request/response buffered in memory
- Large requests/responses can be memory-intensive
- No streaming support means entire body must fit in RAM
- Multiplexing can increase memory usage (more concurrent requests)

## Comparison with HTTP/1.1

| Feature | HTTP/1.1 | HTTP/2 |
|---------|----------|--------|
| Protocol Encoding | Text-based | Binary framing |
| Header Compression | None | HPACK |
| Multiplexing | No (one request per connection) | Yes (many streams per connection) |
| Connection Overhead | High (new TCP for each concurrent request) | Low (reuse single TCP connection) |
| Prioritization | None | Stream priorities |
| Server Push | No | Yes (action defined, needs connection API) |
| Use Case | Simple APIs, legacy clients | Modern APIs, high-concurrency |

## Future Enhancements

### TLS Support (h2 over TLS)
Add TLS support using rustls for browser compatibility:
```rust
use tokio_rustls::{TlsAcceptor, rustls::ServerConfig};

// Create TLS acceptor with ALPN for HTTP/2
let tls_acceptor = TlsAcceptor::from(tls_config);

// Accept TLS connection
let tls_stream = tls_acceptor.accept(tcp_stream).await?;

// Serve HTTP/2 over TLS
http2::Builder::new(TokioExecutor::new())
    .serve_connection(TokioIo::new(tls_stream), service)
    .await?;
```

### Server Push Full Implementation
Complete HTTP/2 server push implementation (currently partially implemented):

**Current Status**: Action defined but doesn't execute pushes (service pattern limitation)

**Full Implementation Approach**:
```rust
// Replace service pattern with connection-level handling
let conn = http2::Builder::new(TokioExecutor::new())
    .serve_connection(io, service);

// Access connection object to send push promises
let mut sender = conn.send_request(
    Request::builder()
        .method("GET")
        .uri("/style.css")
        .body(())
        .unwrap()
)?;

// Send push promise headers and body
sender.send_data(Bytes::from("body { margin: 0; }"), true)?;
```

**Requirements**:
- Restructure handler to use `Connection` API instead of `service_fn`
- Store pending pushes in connection state
- Match incoming requests to push promises
- Handle client rejection of pushes

### Stream Prioritization Control
Allow LLM to influence stream priorities:
```json
{
  "type": "send_http2_response",
  "status": 200,
  "priority": "high",
  "body": "..."
}
```

### Streaming Responses
Support chunked responses generated incrementally:
- Use `Body` trait instead of `Full<Bytes>`
- Allow LLM to generate response chunks asynchronously
- Stream large files without buffering entire file

### HTTP/1.1 to HTTP/2 Upgrade
Support upgrade from HTTP/1.1:
- Detect `Upgrade: h2c` header
- Perform upgrade handshake
- Switch to HTTP/2 on same connection

### ALPN Negotiation
Support ALPN for protocol negotiation over TLS:
- Advertise `h2` in ALPN
- Fall back to HTTP/1.1 if client doesn't support HTTP/2

## References
- [RFC 7540: HTTP/2](https://datatracker.ietf.org/doc/html/rfc7540)
- [RFC 7541: HPACK Header Compression](https://datatracker.ietf.org/doc/html/rfc7541)
- [Hyper HTTP/2 Documentation](https://docs.rs/hyper/latest/hyper/server/conn/http2/index.html)
- [HTTP/2 on Wikipedia](https://en.wikipedia.org/wiki/HTTP/2)
- [HTTP/2 Frequently Asked Questions](https://http2.github.io/faq/)
