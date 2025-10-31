# Tor Directory Protocol Implementation

## Overview

Tor Directory implements an HTTP-based directory server that serves Tor network consensus documents, microdescriptors, and relay information. This is an Alpha-status implementation focused on serving static directory content controlled by the LLM.

**Protocol Compliance**: Tor Directory Protocol (dir-spec.txt)
**Transport**: HTTP/1.1 over TCP
**Status**: Alpha - Basic directory serving functionality

## Library Choices

### HTTP Implementation
- **Manual HTTP parsing** - Simple HTTP/1.1 request line parsing
- **tokio::io** - AsyncBufReadExt for line-by-line request reading
- **tokio::net::TcpStream** - Direct TCP connection handling

**Rationale**: Tor Directory protocol is simple HTTP GET requests to specific paths. Full HTTP server framework (hyper, actix) would be overkill. Manual parsing allows LLM to control all aspects of response generation.

### No External Libraries for Directory Format
- **Manual consensus generation** - LLM generates network-status-version 3 format
- **Manual microdescriptor generation** - LLM generates onion-key and ntor-onion-key fields

**Rationale**: No comprehensive Rust library for Tor directory document generation exists. LLM is well-suited to generating text-based directory documents following the spec.

## Architecture Decisions

### 1. LLM-Controlled Content Generation

**Design Philosophy**: Directory content (consensus, microdescriptors, relay descriptors) is entirely LLM-generated based on user instructions.

**Control Points**:
- Consensus document structure
- Relay entries and flags
- Microdescriptor content
- Error responses (404, 500)

**Benefits**:
- Flexible testing (can generate any network topology)
- Honeypot mode (can serve fake relays)
- Educational demonstrations
- Protocol research

### 2. Simple HTTP Handler

**Request Flow**:
1. Read HTTP request line: `GET /path HTTP/1.1`
2. Skip headers (read until blank line)
3. Create `TOR_DIRECTORY_REQUEST_EVENT` with path and client_ip
4. Call LLM with event
5. Execute action result (send HTTP response)

**Supported Methods**: GET only (Tor Directory is read-only)

**Error Handling**:
- Malformed requests → 400 Bad Request
- LLM failure → 500 Internal Server Error
- Unknown paths → 404 Not Found (LLM decision)

### 3. Action-Based Response System

**Sync Actions** (requires network context):
- `serve_consensus` - Serve network consensus document
- `serve_microdescriptors` - Serve relay microdescriptors
- `serve_server_descriptors` - Serve full server descriptors
- `serve_not_found` - Return 404 error

**Async Actions** (user-triggered):
- None currently (directory is stateless, responds to requests only)

**Action Execution**:
- LLM returns action with consensus_data or microdescriptors payload
- Action handler builds HTTP response with Content-Type and Content-Length
- Response sent to client

### 4. Stateless Design

**No Persistent State**:
- No consensus storage (generated per request)
- No caching (LLM generates fresh each time)
- No relay database

**Benefits**:
- Simple implementation
- Flexible content (LLM can change responses)
- No synchronization issues

**Tradeoffs**:
- Slower than real directory (LLM call per request)
- Not suitable for high-traffic scenarios
- Scripting mode could cache responses for performance

## LLM Integration

**Event Type**: `TOR_DIRECTORY_REQUEST_EVENT`

**Event Data**:
```json
{
  "path": "/tor/status-vote/current/consensus",
  "client_ip": "127.0.0.1",
  "method": "GET"
}
```

**LLM Prompt Context**:
- Available actions: serve_consensus, serve_microdescriptors, serve_server_descriptors, serve_not_found
- Path information (which document is requested)
- Client IP (for logging/filtering)

**Response Actions**:
```json
{
  "actions": [
    {
      "type": "serve_consensus",
      "consensus_data": "network-status-version 3\n..."
    }
  ]
}
```

**Scripting**: Possible but not yet implemented. Would cache LLM-generated responses and serve without LLM calls.

## Connection Management

**Connection Lifecycle**:
1. Accept TCP connection
2. Read HTTP request (single request per connection)
3. Send response
4. Close connection

**HTTP/1.1 without Keep-Alive**: Each request uses new TCP connection (simpler implementation).

**Connection Tracking**:
- Connections tracked in AppState with connection_id
- Bytes sent/received tracked (HTTP response size)
- No persistent connections (single request-response)

## Directory Protocol Paths

### Consensus Documents
- `/tor/status-vote/current/consensus` - Network consensus (all relays)
- `/tor/status-vote/current/consensus-microdesc` - Microdescriptor consensus

### Microdescriptors
- `/tor/micro/d/<digest>` - Single microdescriptor by digest
- `/tor/micro/d/<digest1>-<digest2>` - Multiple microdescriptors

### Server Descriptors
- `/tor/server/d/<digest>` - Single server descriptor
- `/tor/server/all` - All server descriptors

### Authority Keys
- `/tor/keys/authority` - Directory authority keys
- `/tor/keys/fp/<fingerprint>` - Keys for specific relay

**LLM Responsibility**: Generate appropriate content based on path

## Limitations

### Not Implemented
1. **Consensus signing** - No cryptographic signatures (authority_keys.rs, consensus_signer.rs exist but unused)
2. **Compression** - No gzip/deflate support (Tor clients prefer compressed)
3. **Caching headers** - No Cache-Control, ETag, Last-Modified
4. **Partial responses** - No HTTP Range support
5. **POST requests** - No relay descriptor uploads (authority function)
6. **Authentication** - No HTTP auth (not required for mirrors)
7. **HTTPS** - No TLS support (mirrors typically use HTTP)

### Current Capabilities
- Serve consensus documents via LLM
- Serve microdescriptors via LLM
- Return 404 for unknown paths
- Basic HTTP/1.1 response formatting
- Action-based content generation

### Known Issues
- Slow response time (LLM call per request)
- No compression (large consensus documents)
- No caching (generates fresh each time)
- Single request per connection (no keep-alive)

## Example Prompts

### Start a directory mirror
```
open_server port 9030 base_stack tor_directory.
This is a Tor directory mirror.
When clients request /tor/status-vote/current/consensus, return a simple test consensus with network-status-version 3 and a few fake relays.
When clients request any other path, return a 404 error.
```

### Serve microdescriptors
```
open_server port 9030 base_stack tor_directory.
When clients request /tor/micro/d/test, return a microdescriptor with onion-key and ntor-onion-key fields.
```

### Custom consensus content
```
open_server port 9030 base_stack tor_directory.
Serve a consensus document with 5 fake relays: 3 exits, 2 guards.
Include relay fingerprints and flags.
```

## References

- [Tor Directory Protocol](https://spec.torproject.org/dir-spec/) (dir-spec.txt)
- [Network Consensus Format](https://spec.torproject.org/dir-spec/formats.html)
- [Microdescriptors](https://spec.torproject.org/dir-spec/microdescriptors.html)
- [Server Descriptors](https://spec.torproject.org/dir-spec/server-descriptors.html)

## Implementation Statistics

| Module | Lines of Code | Purpose |
|--------|--------------|---------|
| `mod.rs` | 203 | HTTP session handling, request parsing |
| `actions.rs` | ~150 | Action definitions, response generation |
| `authority_keys.rs` | N/A | Placeholder for future signing |
| `consensus_signer.rs` | N/A | Placeholder for future signing |
| **Total** | **~350** | Basic directory server implementation |

This is an Alpha implementation focused on serving LLM-generated directory content for testing and research purposes. Future work includes consensus signing, compression, and caching for production use.
