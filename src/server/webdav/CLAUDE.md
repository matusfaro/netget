# WebDAV Protocol Implementation

## Overview

WebDAV (Web Distributed Authoring and Versioning) server implementing RFC 4918. Provides file server functionality over
HTTP with support for file operations, directory browsing, and locking.

**Protocol**: WebDAV (RFC 4918)
**Transport**: HTTP/1.1 over TCP
**Port**: 80, 8080, or configurable
**Status**: Alpha

## Library Choices

- **dav-server** v0.5 - WebDAV protocol implementation
    - Handles WebDAV method parsing (PROPFIND, MKCOL, COPY, MOVE, etc.)
    - Provides DavHandler for HTTP service integration
    - Manages WebDAV XML response generation
- **dav-server::memfs::MemFs** - In-memory filesystem
    - Virtual filesystem for testing and LLM control
    - No persistent storage (intentional for security)
- **dav-server::fakels::FakeLs** - Fake locking system
    - WebDAV lock/unlock support without actual locking
    - Satisfies protocol requirements for WebDAV clients
- **hyper** v1.x - HTTP server foundation

**Why dav-server?**

- Complete WebDAV protocol implementation
- Abstracts away complex XML parsing/generation
- Focus LLM on filesystem operations, not protocol details

## Architecture Decisions

### Library-Driven Protocol

Unlike most NetGet protocols, WebDAV uses a library (dav-server) to handle protocol details:

- DavHandler processes all WebDAV methods
- MemFs provides virtual filesystem
- LLM controls filesystem content (future enhancement)

### Current Implementation: Library-Only

**IMPORTANT**: Current implementation does NOT integrate LLM:

- Server spawns with DavHandler directly
- All filesystem operations handled by MemFs library
- No LLM consultation for file operations

**Future LLM Integration**: To add LLM control:

1. Implement custom filesystem trait (NFSFileSystem-like)
2. Consult LLM for read/write/lookup/mkdir operations
3. Return LLM-generated file content and directory listings

### In-Memory Filesystem

MemFs provides ephemeral storage:

- Files stored in RAM only
- No persistence across restarts
- Suitable for honeypot/testing scenarios

### HTTP Service Integration

DavHandler wraps as hyper service:

```rust
let service = service_fn(move |req| {
    let dav = dav_clone.clone();
    async move {
        Ok::<_, std::convert::Infallible>(dav.handle(req).await)
    }
});
```

## Connection Management

### HTTP Persistent Connections

WebDAV over HTTP/1.1 with persistent connections:

- Multiple requests per TCP connection
- Connection pooling by clients
- Each connection handled in separate tokio task

### Connection Tracking

Connections tracked in ServerInstance state:

- Connection ID per TCP connection
- Protocol-specific info: `ProtocolConnectionInfo::WebDav { recent_operations }`
- Stats: bytes_sent, bytes_received, packets_sent, packets_received
- Status updated on connection close

### Concurrency

Multiple concurrent connections supported:

- Each connection handled by hyper HTTP/1 server
- No shared mutable state (MemFs internally synchronized)
- Operations are atomic per request

## State Management

### Server State

Minimal state required:

- Connection tracking for UI display
- Recent operations list per connection

### Filesystem State

Managed by MemFs library:

- Directory tree in memory
- File content as byte arrays
- No explicit state management needed

### No Session State

WebDAV is stateless over HTTP:

- Each operation independent
- Authentication could be added via HTTP headers
- Locking state managed by FakeLs (no real locks)

## Limitations

### No LLM Integration (Yet)

Current implementation is library-driven:

- No LLM consultation for file operations
- All operations handled by MemFs
- **To add LLM**: Implement custom filesystem trait

### Limited WebDAV Features

- **No authentication** - All operations allowed
- **No encryption** - Plain HTTP only (no HTTPS)
- **Fake locking** - LOCK/UNLOCK accepted but not enforced
- **No versioning** - DAV versioning extensions not implemented

### In-Memory Only

- No persistent storage
- Files lost on server restart
- Suitable for testing/honeypot, not production file sharing

### Testing Limitations

- WebDAV clients (Windows, macOS Finder) may have specific requirements
- Some clients require authentication headers
- Testing uses reqwest with custom HTTP methods

## LLM Integration (Future Enhancement)

### Planned Action-Based Responses

**webdav_file_content** - Return file content for GET/PROPFIND:

```json
{
  "type": "webdav_file_content",
  "path": "/documents/readme.txt",
  "content": "Welcome to NetGet WebDAV server",
  "size": 33,
  "modified": "2024-01-15T10:30:00Z"
}
```

**webdav_directory_listing** - Return directory contents:

```json
{
  "type": "webdav_directory_listing",
  "path": "/documents",
  "entries": [
    {"name": "readme.txt", "type": "file", "size": 33},
    {"name": "subdir", "type": "directory"}
  ]
}
```

**webdav_create_file** - Confirm file creation:

```json
{
  "type": "webdav_create_file",
  "path": "/documents/newfile.txt",
  "status": "created"
}
```

### Event-Based Processing (Future)

WebDAV operations would trigger `WEBDAV_OPERATION_EVENT`:

```json
{
  "method": "PROPFIND",
  "path": "/documents",
  "depth": 1
}
```

### Implementation Pattern

Follow NFS implementation pattern:

1. Create `LlmWebDavFilesystem` implementing dav-server filesystem trait
2. Consult LLM in filesystem methods (read, write, lookup, etc.)
3. Parse LLM action responses and return to DavHandler

## Example Usage (Current)

### Example 1: Basic WebDAV Server

**Prompt:**

```
Listen on port 8080 using webdav stack. Provide a virtual filesystem
with directory /documents.
```

**Behavior:**

- Server starts with MemFs filesystem
- Root directory "/" created automatically
- Clients can PROPFIND, PUT, GET, MKCOL operations
- No LLM involvement (handled by dav-server)

### Example 2: File Operations

**Client Operations:**

```
PROPFIND / (list root directory)
MKCOL /documents (create directory)
PUT /documents/test.txt (upload file)
GET /documents/test.txt (download file)
```

**Server Behavior:**

- All operations handled by MemFs
- Files stored in memory
- Standard WebDAV responses (207 Multi-Status, 201 Created, etc.)

## Example Prompts (Future with LLM)

### Example 1: LLM-Controlled Files

**Prompt:**

```
Listen on port 8080 via WebDAV. When clients request /documents/readme.txt,
return content "Welcome to NetGet WebDAV server".
```

**Expected LLM Response:**

```json
{
  "actions": [
    {
      "type": "webdav_file_content",
      "path": "/documents/readme.txt",
      "content": "Welcome to NetGet WebDAV server"
    }
  ]
}
```

### Example 2: Dynamic Directory Listings

**Prompt:**

```
Listen on port 8080 via WebDAV. For PROPFIND on /documents, show files:
readme.txt (33 bytes), report.pdf (1024 bytes).
```

**Expected LLM Response:**

```json
{
  "actions": [
    {
      "type": "webdav_directory_listing",
      "path": "/documents",
      "entries": [
        {"name": "readme.txt", "type": "file", "size": 33},
        {"name": "report.pdf", "type": "file", "size": 1024}
      ]
    }
  ]
}
```

## References

- [RFC 4918: WebDAV](https://tools.ietf.org/html/rfc4918)
- [dav-server Rust crate](https://docs.rs/dav-server)
- [hyper HTTP library](https://docs.rs/hyper)
- [WebDAV Resources](http://www.webdav.org/)

## Logging

### Structured Logging Levels

**TRACE** - Full WebDAV request/response details:

- Complete XML bodies for PROPFIND responses
- File content for PUT/GET operations
- Lock token details

**DEBUG** - WebDAV operation summaries:

- Method and path
- "WebDAV PROPFIND /documents depth=1"
- "WebDAV PUT /documents/file.txt (256 bytes)"

**INFO** - High-level events:

- Connection open/close
- "WebDAV connection from 192.168.1.100"
- "WebDAV connection closed"

**WARN** - Non-fatal issues:

- Malformed WebDAV requests
- Invalid XML in PROPFIND

**ERROR** - Critical failures:

- DavHandler errors
- Filesystem operation failures

All logs use dual logging pattern (tracing macros + status_tx).
