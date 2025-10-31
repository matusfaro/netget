# HTTP Protocol Implementation

## Overview
HTTP server implementing RFC 7230-7235 (HTTP/1.1) using the hyper library. The LLM controls HTTP responses (status codes, headers, body) while hyper handles the protocol parsing, connection management, and request routing.

**Status**: Beta (Core Protocol)
**RFC**: RFC 7230-7235 (HTTP/1.1)

## Library Choices
- **hyper v1.0** - Modern async HTTP library with excellent performance
- **hyper-util** - Additional utilities for Tokio integration
- **http-body-util** - Body handling utilities (Full, Empty, Incoming)
- **Manual response construction** - LLM generates status, headers, body

**Rationale**: Hyper is the de-facto standard for HTTP in Rust. It handles:
- HTTP/1.1 protocol parsing and formatting
- Request header parsing and validation
- Connection keep-alive management
- Chunked transfer encoding
- Async I/O with Tokio integration

The LLM focuses on application logic (routing, business logic, response generation) rather than protocol implementation details.

## Architecture Decisions

### 1. Request-Response Model
HTTP is inherently synchronous request-response:
- Each request is handled independently by hyper
- LLM generates response based on request (method, URI, headers, body)
- No persistent connection state between requests (though TCP connection may persist)
- No async actions - HTTP is purely reactive to client requests

### 2. Hyper Service Pattern
Hyper uses a service pattern for request handling:
- `service_fn()` creates a closure that handles each request
- Each request gets a fresh LLM call to generate the response
- Service is cloned for each connection (cheap clone via Arc)
- Connection-scoped state persists across requests on same TCP connection

### 3. Connection Tracking
Unlike raw TCP, HTTP connections are tracked per-TCP-connection, not per-request:
- One `ConnectionId` per TCP connection (not per HTTP request)
- `ProtocolConnectionInfo::Http` tracks recent requests on the connection
- Multiple HTTP requests can occur on the same TCP connection (HTTP keep-alive)
- Connection closed when TCP socket closes

### 4. Body Handling
Request bodies are fully buffered before LLM processing:
- `req.into_body().collect().await` reads entire body into memory
- Bodies are converted to UTF-8 strings for LLM (or "binary" marker for non-UTF8)
- Response bodies are wrapped in `Full<Bytes>` for hyper
- No streaming body support (LLM sees complete request, generates complete response)

### 5. Header Parsing
Headers are extracted and provided to LLM as JSON object:
- All headers converted to lowercase keys (HTTP headers are case-insensitive)
- Non-UTF8 header values are skipped (rare edge case)
- LLM can specify response headers in same JSON format

### 6. Dual Logging
All HTTP operations use dual logging:
- **DEBUG**: Request summary (method, URI, body size)
- **TRACE**: Full request details (all headers, pretty-printed JSON body)
- Both go to `netget.log` (via tracing) and TUI Status panel (via status_tx)

### 7. Error Handling
LLM errors result in 500 Internal Server Error:
- If LLM call fails, return 500 with "Internal Server Error" body
- If LLM response is invalid, return 500
- Hyper connection errors (parsing failures) close connection automatically

## LLM Integration

### Action-Based Response Model
The LLM responds to HTTP events with actions:

**Events**:
- `http_request` - HTTP request received from client
  - Parameters: `method`, `uri`, `headers`, `body`

**Available Actions**:
- `send_http_response` - Send HTTP response (status, headers, body)
- Common actions: `show_message`, `update_instruction`, etc.
- **No async actions** - HTTP is purely request-response

### Example LLM Response
```json
{
  "actions": [
    {
      "type": "send_http_response",
      "status": 200,
      "headers": {
        "Content-Type": "text/html",
        "X-Custom-Header": "value"
      },
      "body": "<html><body>Hello World</body></html>"
    },
    {
      "type": "show_message",
      "message": "Served GET /"
    }
  ]
}
```

### Response Format
The `send_http_response` action returns structured data:
```json
{
  "status": 200,
  "headers": {"Content-Type": "text/html"},
  "body": "<html>...</html>"
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
2. **Register**: Connection added to `ServerInstance` with `ProtocolConnectionInfo::Http`
3. **Serve**: Hyper's `http1::Builder::new().serve_connection()` handles HTTP/1.1 protocol
4. **Request Loop**: Each request on the connection calls the service function
5. **LLM Call**: Service function calls LLM to generate response
6. **Close**: Connection removed when TCP socket closes (client disconnect or error)

### Connection Data Structure
```rust
ProtocolConnectionInfo::Http {
    recent_requests: Vec<String>, // Track recent URIs requested
}
```

Unlike TCP, no write_half or queued_data - hyper manages the socket internally.

### State Updates
- Connection state tracked in `ServerInstance.connections`
- Each request increments `packets_received` and `bytes_received`
- Each response increments `packets_sent` and `bytes_sent`
- `last_activity` updated on each request
- UI updates via `__UPDATE_UI__` message

## Known Limitations

### 1. HTTP/1.1 Only
- No HTTP/2 support (hyper supports it, but not implemented here)
- No HTTP/3 (QUIC) support
- Connection upgrade (WebSocket) not supported

### 2. No TLS Support
- Raw HTTP only (port 80, 8080, etc.)
- For HTTPS, would need to wrap listener with rustls or native-tls
- See future enhancement section for HTTPS implementation plan

### 3. No Streaming
- Request bodies fully buffered before LLM processing
- Response bodies fully generated before sending
- No support for chunked responses generated incrementally
- Large requests/responses may exhaust memory

### 4. No Connection Pooling Control
- Hyper manages keep-alive automatically
- No LLM control over connection persistence
- No way to force close after response (hyper decides)

### 5. Limited Header Control
- LLM can set custom headers, but hyper may add/modify:
  - `Content-Length` (calculated automatically)
  - `Date` (may be added by hyper)
  - `Connection` (keep-alive management)
- No way to prevent hyper's automatic headers

### 6. No Multipart Form Support
- Request body provided as raw string to LLM
- No automatic parsing of `multipart/form-data` or `application/x-www-form-urlencoded`
- LLM must parse these formats manually if needed

## Example Prompts

### Simple Web Server
```
listen on port 8080 via http
For GET /, return <h1>Welcome</h1>
For GET /about, return <h1>About Us</h1>
For other paths, return 404 with "Not Found"
```

### JSON API
```
listen on port 3000 via http
For POST /api/users, parse JSON body and return:
  Status: 201
  Content-Type: application/json
  Body: {"status": "created", "id": 123}
For GET /api/users/:id, return user data as JSON
```

### Static File Server (Simulated)
```
listen on port 8000 via http
For GET /index.html, return HTML content
For GET /style.css, return CSS with Content-Type: text/css
For GET /script.js, return JS with Content-Type: application/javascript
For other paths, return 404
```

### Custom Headers and Status Codes
```
listen on port 8080 via http
For GET /health, return 200 with body: OK
For GET /redirect, return 301 with Location header: /home
For GET /forbidden, return 403 with body: Access Denied
For POST /data, return 201 with X-Request-ID header
```

### REST API with Routing
```
listen on port 4000 via http
For GET /products, return list of products as JSON
For GET /products/:id, return single product as JSON
For POST /products, create new product and return 201
For PUT /products/:id, update product and return 200
For DELETE /products/:id, delete product and return 204 (no body)
```

## Performance Characteristics

### Latency
- One LLM call per HTTP request
- Typical latency: 2-5 seconds per request with qwen3-coder:30b
- Connection keep-alive reduces TCP handshake overhead for subsequent requests

### Throughput
- Limited by LLM response time (2-5s per request)
- Concurrent requests processed in parallel (each on separate tokio task)
- Hyper handles connection multiplexing efficiently

### Concurrency
- Unlimited concurrent connections (bounded by system resources)
- Each connection processed on separate tokio task
- Ollama lock serializes LLM API calls across all connections

### Memory Usage
- Each request/response buffered in memory
- Large requests/responses can be memory-intensive
- No streaming support means entire body must fit in RAM

## Comparison with Raw TCP

| Feature | TCP (Raw) | HTTP |
|---------|-----------|------|
| Protocol Parsing | LLM constructs protocol | Hyper handles HTTP parsing |
| Request Structure | Raw bytes | Parsed method, URI, headers, body |
| Response Structure | Raw bytes | Status, headers, body |
| Connection Model | Persistent, stateful | Request-response, less state |
| Use Case | Custom protocols, FTP, SMTP | Web APIs, REST, webhooks |
| Complexity | LLM implements protocol | LLM implements application logic |

## Future Enhancements

### HTTPS Support
Add TLS support using rustls:
```rust
use tokio_rustls::{TlsAcceptor, rustls::ServerConfig};

// Create TLS acceptor
let tls_acceptor = TlsAcceptor::from(tls_config);

// Accept TLS connection
let tls_stream = tls_acceptor.accept(tcp_stream).await?;

// Serve HTTP over TLS
http1::Builder::new().serve_connection(TokioIo::new(tls_stream), service).await?;
```

### HTTP/2 Support
Hyper supports HTTP/2, would need to:
- Use `hyper::server::conn::http2` instead of `http1`
- Handle ALPN negotiation for HTTP/2 over TLS
- Update LLM prompts to describe HTTP/2 features (server push, etc.)

### WebSocket Upgrade
Support WebSocket connections:
- Detect `Upgrade: websocket` header
- Perform WebSocket handshake
- Switch to WebSocket protocol implementation

### Streaming Responses
Support chunked responses generated incrementally:
- Use `Body` trait instead of `Full<Bytes>`
- Allow LLM to generate response chunks asynchronously
- Stream file downloads without buffering entire file

### Request Body Streaming
Process large request bodies in chunks:
- Don't buffer entire body before LLM call
- Stream body to LLM in chunks
- Useful for file uploads

## References
- [RFC 7230: HTTP/1.1 Message Syntax and Routing](https://datatracker.ietf.org/doc/html/rfc7230)
- [RFC 7231: HTTP/1.1 Semantics and Content](https://datatracker.ietf.org/doc/html/rfc7231)
- [RFC 7232: HTTP/1.1 Conditional Requests](https://datatracker.ietf.org/doc/html/rfc7232)
- [RFC 7233: HTTP/1.1 Range Requests](https://datatracker.ietf.org/doc/html/rfc7233)
- [RFC 7234: HTTP/1.1 Caching](https://datatracker.ietf.org/doc/html/rfc7234)
- [RFC 7235: HTTP/1.1 Authentication](https://datatracker.ietf.org/doc/html/rfc7235)
- [Hyper Documentation](https://docs.rs/hyper/latest/hyper/)
- [HTTP on Wikipedia](https://en.wikipedia.org/wiki/Hypertext_Transfer_Protocol)
