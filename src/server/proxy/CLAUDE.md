# HTTP/HTTPS Proxy Protocol Implementation

## Overview

HTTP/HTTPS proxy server implementing RFC 7230 (HTTP/1.1) with CONNECT method support (RFC 7231) for HTTPS tunneling.
Provides sophisticated Man-in-the-Middle (MITM) capabilities with certificate generation, pass-through mode, and
LLM-controlled filtering.

**Compliance**: HTTP/1.1 (RFC 7230-7235), CONNECT method (RFC 7231 Section 4.3.6)

## Library Choices

- **`http-mitm-proxy`** (conceptual) - Protocol framework for MITM operations
- **`rcgen`** v0.13 - On-the-fly certificate generation for MITM TLS interception
- **`rustls`** v0.23 + **`tokio-rustls`** v0.26 - TLS stack for MITM (both server and client)
- **`webpki-roots`** v0.26 - Root certificates for validating upstream TLS connections
- **`regex`** - Pattern matching for selective request/response filtering
- **Manual implementation** - Request/response parsing and modification for maximum LLM control
- **`axum`** / **`reqwest`** - Not used; raw TCP/TLS handling for full flexibility

**Why manual implementation**: Provides complete control over every aspect of request/response modification, allowing
the LLM to inject headers, rewrite URLs, modify bodies, and make granular filtering decisions without library
constraints.

## Architecture Decisions

### Three Operating Modes

1. **MITM Mode with Certificate Generation** (default)
    - Generates self-signed CA certificate at startup
    - Intercepts HTTPS by performing TLS handshake with client using generated cert
    - Decrypts, inspects, and forwards traffic
    - Status: Certificate generation implemented, full TLS MITM pending

2. **MITM Mode with Loaded Certificate**
    - Loads existing CA certificate from file (cert_path, key_path)
    - Uses production CA for enterprise deployments
    - Allows transparent HTTPS inspection with trusted certificates

3. **Pass-Through Mode** (certificate_mode = "none")
    - No certificate, no decryption
    - HTTPS CONNECT requests create tunnels without inspection
    - LLM controls allow/block decisions based on destination host/port/SNI only
    - HTTP requests fully inspected and modifiable

### Filter Configuration System

**`ProxyFilterConfig`** allows granular control of LLM involvement:

```rust
pub struct ProxyFilterConfig {
    certificate_mode: CertificateMode,    // Generate, LoadFromFile, or None
    request_filter_mode: FilterMode,      // AllRequests, Selective, None
    response_filter_mode: FilterMode,     // AllResponses, Selective, None
    https_connection_filter_mode: FilterMode, // AllConnections, Selective, None
    request_patterns: Vec<String>,        // Regex patterns for selective filtering
    response_patterns: Vec<String>,
    https_host_patterns: Vec<String>,
}
```

**FilterMode**:

- `AllRequests` - LLM consults on every request
- `Selective` - LLM consults only when patterns match
- `None` - Pass through without LLM (fast path)

This prevents performance bottlenecks when LLM involvement is unnecessary.

### Request/Response Lifecycle

**HTTP Request Flow**:

1. Client sends HTTP request → Parse method, URI, headers, body
2. Check `request_filter_mode` + patterns → Decide if LLM consultation needed
3. If consulting LLM → Create `PROXY_HTTP_REQUEST_EVENT` with full request info
4. LLM returns `RequestAction`: Pass, Block, or Modify
5. Apply modifications (headers, path, query params, body, regex replacements)
6. Forward to upstream server
7. Parse response → Check `response_filter_mode`
8. Return response to client (with optional modifications)

**HTTPS CONNECT Flow** (Pass-Through):

1. Client sends `CONNECT host:port` → Parse destination
2. Check `https_connection_filter_mode` + patterns
3. If consulting LLM → Create `PROXY_HTTPS_CONNECT_EVENT` with destination info
4. LLM returns `HttpsConnectionAction`: Allow or Block
5. If allowed → Send `200 Connection Established`, create bidirectional tunnel
6. If blocked → Send `403 Forbidden` with reason

**Access Logging**:

- **DEBUG level**: `[ACCESS] {client_ip} {method} {url} -> {status} {bytes} in {duration}`
- Common Log Format compatible for integration with log analyzers
- Pass-through HTTPS: `[ACCESS] {client_ip} CONNECT {host}:{port} -> TUNNEL {bytes}`

## LLM Integration

### Action-Based Control

**LLM returns structured actions for granular control**:

**HTTP Request Actions**:

```json
{
  "actions": [
    {
      "type": "pass_http_request",
      "message": "Allowing request to example.com"
    },
    {
      "type": "block_http_request",
      "status": 403,
      "body": "Access denied by policy"
    },
    {
      "type": "modify_http_request",
      "headers": {"X-Proxy": "NetGet"},
      "remove_headers": ["User-Agent"],
      "new_path": "/api/v2",
      "query_params": {"key": "value"},
      "new_body": "modified content",
      "body_replacements": [
        {"pattern": "old", "replacement": "new"}
      ]
    }
  ]
}
```

**HTTPS Connection Actions** (Pass-Through Mode):

```json
{
  "actions": [
    {
      "type": "allow_https_connect",
      "message": "Allowing connection to example.com:443"
    },
    {
      "type": "block_https_connect",
      "reason": "Domain blocked by policy"
    }
  ]
}
```

### Event Types

**`PROXY_HTTP_REQUEST_EVENT`**:

- Triggered: When HTTP request matches filter configuration
- Context: method, url, host, path, headers, body, client_addr
- LLM decides: Pass, Block (status + body), Modify (granular changes)

**`PROXY_HTTPS_CONNECT_EVENT`**:

- Triggered: When HTTPS CONNECT request matches filter configuration
- Context: destination_host, destination_port, sni (from TLS handshake if available), client_addr
- LLM decides: Allow (create tunnel), Block (send 403)

## Connection and State Management

**Per-Connection State** (`ProtocolConnectionInfo::Proxy`):

```rust
Proxy {
    recent_requests: Vec<(String, u16, Duration)>, // (url, status, duration)
}
```

Tracks recent proxy activity for monitoring and debugging.

**Connection Lifecycle**:

1. Accept TCP connection → Add to server connections
2. Read initial HTTP request line
3. If `CONNECT` → Route to `handle_https_connect()`
4. If other method → Route to `handle_http_request()`
5. Process through LLM filtering pipeline
6. Mark connection closed after completion

**Concurrent Connections**: Each connection handled in separate tokio task. No connection limit enforced by default (
production deployments should add rate limiting).

## Certificate Management

**Self-Signed CA Generation**:

```rust
let mut params = CertificateParams::default();
params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
params.distinguished_name.push(DnType::CommonName, "NetGet MITM Proxy CA");
let key_pair = KeyPair::generate()?;
let cert = params.self_signed(&key_pair)?;
```

**Per-Host Certificate Generation** (for MITM):

- Implemented in `cert_cache.rs` using `CertificateCache`
- Generates leaf certificates signed by CA for specific domains
- Caches certificates per domain (24-hour TTL) to avoid regeneration overhead
- Supports domain normalization (case-insensitive, trimmed)
- Automatic addition of wildcard and www variants in SAN

**Certificate Installation**:

- Users must install CA certificate in system trust store for MITM mode
- Without installation: browsers show security warnings
- Enterprise deployments: Distribute CA via group policy
- Use `export_ca_certificate` action to save CA cert to file
- Installation locations:
  - **macOS**: System Keychain (`/Library/Keychains/System.keychain`)
  - **Windows**: Trusted Root Certification Authorities
  - **Linux**: `/usr/local/share/ca-certificates/` then `update-ca-certificates`

## Implementation Details

### Module Structure

- **`mod.rs`**: Main proxy server, connection handling, pass-through mode
- **`actions.rs`**: LLM action definitions and execution
- **`filter.rs`**: Request/response/HTTPS filtering logic
- **`cert_cache.rs`**: Per-domain certificate caching for MITM
- **`tls_mitm.rs`**: Full TLS MITM orchestration (client accept + upstream connect)

### MITM Flow (tls_mitm.rs)

1. **Send 200 to client**: `HTTP/1.1 200 Connection Established\r\n\r\n`
2. **Generate leaf cert**: `CertificateCache::get_or_generate(domain)` → signed by CA
3. **TLS accept from client**: `TlsAcceptor::accept()` using generated cert
4. **TLS connect to upstream**: `TlsConnector::connect()` with certificate validation
5. **Read HTTP request**: Parse from decrypted client TLS stream
6. **Consult LLM for request**: Create `PROXY_HTTP_REQUEST_EVENT`, get `RequestAction`
7. **Forward to upstream**: Apply request modifications, send via upstream TLS stream
8. **Read HTTP response**: Receive from upstream TLS stream, parse status/headers/body
9. **Consult LLM for response**: Create `PROXY_HTTP_RESPONSE_EVENT`, get `ResponseAction`
10. **Return to client**: Apply response modifications (status, headers, body), send via client TLS stream
11. **Bidirectional copy**: Switch to `tokio::io::copy` for keep-alive connections

### Certificate Cache Design

- **Thread-safe**: `Arc<RwLock<HashMap<String, CachedCert>>>`
- **TTL**: 24 hours (configurable via `cert_ttl_secs`)
- **Normalization**: Domains lowercased and trimmed
- **SAN generation**: Automatically adds wildcard (`*.example.com`) and www (`www.example.com`)
- **Automatic cleanup**: Background task runs hourly to remove expired certificates
- **Manual cleanup**: `cleanup_expired()` method available for on-demand maintenance
- **Stats**: `get_stats()` returns `CacheStats` (total, expired, valid)
- **Logging**: Cache stats logged hourly to INFO level

## Current Status

1. **HTTPS MITM Fully Implemented** ✅
    - Certificate generation works (self-signed CA)
    - Per-domain leaf certificate caching implemented (`cert_cache.rs`)
    - Automatic cache cleanup task (runs hourly)
    - TLS interception implemented (`tls_mitm.rs`):
      - Client-side TLS accept with dynamically generated certificates
      - Upstream-side TLS connect with certificate validation
      - HTTP request/response proxying through LLM filtering
      - **Response modification**: Full HTTP response parsing and modification
        - Parse status code, headers, and body from upstream responses
        - LLM consultation via `PROXY_HTTP_RESPONSE_EVENT`
        - Support for status changes, header modifications, body replacement
        - Configurable via `response_filter_mode` (All/Selective/None)
    - Certificate export action for user installation
    - Comprehensive E2E tests for MITM mode (7 test cases)

## Limitations

1. **HTTP/2 and HTTP/3 Not Supported**
    - Only HTTP/1.1 implemented
    - HTTPS CONNECT tunnels may carry HTTP/2 (transparent to proxy in pass-through)

3. **WebSocket Upgrade Not Handled**
    - WebSocket handshake passes through
    - No LLM inspection of WebSocket frames

4. **Chunked Transfer Encoding**
    - Basic support for reading responses
    - Complex chunked request bodies may not be fully parsed

5. **No Authentication**
    - Proxy doesn't require authentication (anyone can use it)
    - Should add Basic/Digest auth for production

### Protocol Compliance Gaps

- Missing: Range requests (partial content)
- Missing: 100-Continue handling
- Missing: Upgrade header (WebSocket, HTTP/2)
- Missing: Trailer headers in chunked encoding

## Performance Considerations

**Fast Path**: When `FilterMode::None`, requests pass through without LLM consultation (< 1ms overhead).

**Selective Filtering**: Regex pattern matching on URL/headers adds ~10-100μs overhead.

**LLM Consultation**: Adds 500ms-5s latency per request (depends on model and prompt complexity). Use selective
filtering to minimize impact.

**Concurrent Request Handling**: Each request spawns async task, allowing parallel LLM consultations.

## Example Prompts

### Basic HTTP Proxy

```
Listen on port 8080 using proxy stack. Pass all HTTP requests through unchanged.
```

### Selective Blocking

```
Listen on port 8080 using proxy stack. Block all requests to *.ads.* domains with
status 403 and body "Ads blocked". Pass all other requests through.
```

### Header Injection

```
Listen on port 8080 using proxy stack. For all requests to api.example.com, add
header "Authorization: Bearer TOKEN123" before forwarding.
```

### HTTPS Allow/Block (Pass-Through)

```
Listen on port 8080 using proxy stack with no certificate (pass-through mode).
Block HTTPS connections to facebook.com and twitter.com. Allow all others.
```

### URL Rewriting

```
Listen on port 8080 using proxy stack. Rewrite all requests from /old-api/* to
/new-api/* before forwarding.
```

### Request Body Modification

```
Listen on port 8080 using proxy stack. For POST requests to /api/login, replace
any occurrence of "password" in the body with "hashed_password" before forwarding.
```

### MITM Mode (When Fully Implemented)

```
Listen on port 8080 using proxy stack with certificate generation (MITM mode).
Inspect all HTTPS traffic and log any requests containing credit card patterns.
```

## References

- RFC 7230: HTTP/1.1 Message Syntax and Routing
- RFC 7231: HTTP/1.1 Semantics and Content (CONNECT method)
- RFC 7235: HTTP/1.1 Authentication
- RFC 5246: TLS 1.2 (for HTTPS interception)
- Common Log Format: https://en.wikipedia.org/wiki/Common_Log_Format
- rcgen documentation: https://docs.rs/rcgen/
