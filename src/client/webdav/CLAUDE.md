# WebDAV Client Implementation

## Overview

The WebDAV client extends HTTP to support Web-based Distributed Authoring and Versioning (WebDAV) operations. It
provides LLM-controlled access to remote file systems via WebDAV protocol.

## Library Choices

### Core Libraries

- **reqwest** (v0.12+) - HTTP client with custom method support
    - Supports custom HTTP methods via `Method::from_bytes()`
    - Built-in TLS support for HTTPS
    - Timeout configuration
    - No native WebDAV support, so we implement WebDAV on top

### Why reqwest?

- **Mature**: Battle-tested HTTP client library
- **Async**: Native tokio support
- **Custom Methods**: Supports WebDAV methods (PROPFIND, MKCOL, COPY, MOVE, etc.)
- **Simple**: Easy to construct requests with headers and bodies

## Architecture

### Connection Model

WebDAV is **connectionless** like HTTP - each operation is a separate HTTP request:

1. **Client Initialization**: Creates reqwest client, stores base URL
2. **On-Demand Requests**: LLM triggers WebDAV methods via actions
3. **Response Processing**: LLM receives XML responses and decides next action

### State Management

State stored in `AppState::protocol_data`:

- `base_url`: Base URL for all WebDAV requests
- `http_client`: Reqwest client instance marker

### WebDAV Methods Supported

| Method        | Purpose                            | Body Required                |
|---------------|------------------------------------|------------------------------|
| **PROPFIND**  | List properties/directory contents | XML (properties to fetch)    |
| **MKCOL**     | Create collection (directory)      | No                           |
| **COPY**      | Copy resource                      | No (uses Destination header) |
| **MOVE**      | Move/rename resource               | No (uses Destination header) |
| **DELETE**    | Delete resource                    | No                           |
| **PUT**       | Upload file                        | Yes (file content)           |
| **GET**       | Download file                      | No                           |
| **PROPPATCH** | Modify properties                  | XML (properties to update)   |
| **LOCK**      | Lock resource                      | XML (lock info)              |
| **UNLOCK**    | Unlock resource                    | No                           |

## LLM Integration

### Action Flow

1. **User Request** → LLM instruction (e.g., "List files in /dav/documents/")
2. **LLM Decision** → Generates WebDAV action (e.g., PROPFIND with depth:1)
3. **Action Execution** → `execute_action()` returns `ClientActionResult::Custom`
4. **Request Processing** → `make_request()` constructs HTTP request with WebDAV method
5. **Response** → LLM receives XML response via `webdav_response_received` event
6. **Next Action** → LLM decides follow-up (e.g., download a file, create folder)

### Event Triggers

- **webdav_connected**: Fired when client initializes
- **webdav_response_received**: Fired after each WebDAV response
    - Contains: status_code, headers, body (XML), method used

### XML Body Construction

#### PROPFIND Example

**All properties:**

```xml
<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:">
  <D:allprop/>
</D:propfind>
```

**Specific properties:**

```xml
<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:">
  <D:prop>
    <D:getcontentlength/>
    <D:getlastmodified/>
    <D:resourcetype/>
  </D:prop>
</D:propfind>
```

The LLM specifies which properties to request (or `null` for all), and we construct the XML automatically.

### Headers

WebDAV uses special HTTP headers:

- **Depth**: Controls recursion level for PROPFIND/COPY
    - `0`: Resource only
    - `1`: Resource + immediate children
    - `infinity`: All descendants
- **Destination**: Target path for COPY/MOVE
- **Overwrite**: T (true) or F (false) for COPY/MOVE
- **Lock-Token**: Token for LOCK/UNLOCK operations

## Implementation Details

### Custom HTTP Methods

reqwest doesn't natively support WebDAV methods, so we use:

```rust
let method = reqwest::Method::from_bytes(b"PROPFIND")?;
let request = http_client.request(method, url);
```

### XML Response Parsing

We **don't** parse XML in the client implementation. Instead:

1. Return raw XML body to LLM
2. LLM extracts information from XML (file names, sizes, types)
3. LLM decides next action based on XML content

This keeps the implementation simple and lets the LLM handle XML understanding.

### Authentication

WebDAV typically uses HTTP Basic Auth or Digest Auth:

- **Basic Auth**: Send `Authorization: Basic <base64(username:password)>` header
- **Digest Auth**: reqwest handles automatically if credentials provided

Future enhancement: Add auth parameter to startup config.

## Limitations

1. **No XML Parsing**: We return raw XML to LLM, relying on LLM's XML understanding
2. **No Lock Management**: LOCK/UNLOCK implemented but no lock token tracking
3. **No Versioning**: WebDAV versioning extensions (DeltaV) not supported
4. **No Access Control**: ACL methods not implemented
5. **No Quota Support**: QUOTA extension not implemented
6. **Authentication**: Currently no built-in auth (add headers manually)

## Example Prompts

### List Directory

```
Connect to http://webdav.example.com/dav and list all files in the /documents/ folder
```

LLM generates:

```json
{
  "type": "propfind",
  "path": "/documents/",
  "depth": "1"
}
```

### Upload File

```
Upload a file named hello.txt with content "Hello, WebDAV!" to /dav/files/
```

LLM generates:

```json
{
  "type": "put",
  "path": "/dav/files/hello.txt",
  "content": "Hello, WebDAV!",
  "content_type": "text/plain"
}
```

### Create Folder

```
Create a new folder named "projects" in /dav/
```

LLM generates:

```json
{
  "type": "mkcol",
  "path": "/dav/projects/"
}
```

### Copy File

```
Copy /dav/report.pdf to /dav/backup/report.pdf
```

LLM generates:

```json
{
  "type": "copy",
  "source": "/dav/report.pdf",
  "destination": "/dav/backup/report.pdf",
  "overwrite": true
}
```

## Testing Strategy

E2E tests will use a local WebDAV server (e.g., `wsgidav` Python package):

1. Start WebDAV server on `127.0.0.1:8080`
2. Connect WebDAV client with instruction
3. LLM performs operations (PROPFIND, PUT, MKCOL, etc.)
4. Verify results via assertions

Target: **< 10 LLM calls** per test suite.

## Known Issues

1. **URL Encoding**: Paths with special characters may need encoding
2. **Namespaces**: XML namespaces assumed to be `DAV:` - custom namespaces not handled
3. **Error Handling**: HTTP errors returned as events, but no special WebDAV error parsing
4. **Chunked Uploads**: Large files uploaded in single request (no chunking)

## Future Enhancements

- Add authentication support (Basic/Digest) via startup parameters
- Parse common XML responses (multistatus) to provide structured data
- Support WebDAV extensions (CalDAV, CardDAV)
- Implement chunked uploads for large files
- Add lock token management for exclusive access
