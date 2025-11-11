# Mercurial HTTP Server Implementation

## Overview

Mercurial HTTP server implementing the Mercurial wire protocol over HTTP (used by `hg clone http://...` and `hg pull`).
The LLM controls virtual repositories, capabilities advertisement, branch information, and bundle generation. This is a
**read-only implementation** (clone/pull only - no push support).

## Protocol Version

- **Mercurial HTTP wire protocol**: Protocol used by `hg clone http://...` and `hg pull`
- **Transport**: HTTP/1.1 GET and POST
- **Commands** (via `?cmd=<command>` query parameter):
    - `GET /?cmd=capabilities` - Server capabilities
    - `GET /?cmd=heads` - Repository heads (tip changesets)
    - `GET /?cmd=branchmap` - Branch to node ID mappings
    - `GET /?cmd=listkeys&namespace=<ns>` - List keys (bookmarks, tags, etc.)
    - `POST /?cmd=getbundle` - Bundle retrieval (clone/pull)
    - `POST /?cmd=unbundle` - Bundle upload (push - not implemented)
- **Format**: Text-based responses, some binary bundles

## Library Choices

### Core Dependencies

- **hyper** (v1) - HTTP/1.1 server implementation
    - Chosen for: Existing NetGet infrastructure, async/await support
    - Used for: HTTP request/response processing
- **urlencoding** - URL decoding for query parameters
    - Used for: Parsing command parameters
- **No Mercurial library** - Manual implementation for maximum LLM control
    - Rationale: Mercurial protocol is simple enough to implement directly, provides full flexibility

### Why No hg Libraries for Server?

- **hglib** - Client library only (command server interface)
- **No Rust server libraries** - Mercurial server components don't exist in Rust ecosystem
- **Manual implementation** provides:
    - Full LLM control over protocol responses
    - Virtual repositories without real .hg directories
    - Flexibility in bundle generation

## Architecture Decisions

### Virtual Repositories

**No Real .hg Directories** - All repository content is LLM-generated:

1. **Repository metadata**: Stored in memory (not yet implemented - future enhancement)
2. **Heads/branches**: LLM provides list with fake/real node IDs
3. **Bundles**: LLM generates bundle data (or empty bundle for MVP)
4. **No persistence**: Repositories exist only during server lifetime

**Why Virtual**:

- Maximum flexibility - LLM can create any content
- No filesystem dependencies
- Perfect for honeypots (serve fake repositories)
- Ideal for testing/demonstrations

### LLM Control Points

**Complete Repository Control** - LLM implements all Mercurial operations:

1. **Capabilities** (`GET /?cmd=capabilities`):
    - Client: "What can this server do?"
    - LLM: Generates list of supported capabilities

2. **Heads** (`GET /?cmd=heads`):
    - Client: "What are the repository heads?"
    - LLM: Generates list of head node IDs

3. **Branch Map** (`GET /?cmd=branchmap`):
    - Client: "What branches exist?"
    - LLM: Generates branch name to node ID mappings

4. **List Keys** (`GET /?cmd=listkeys&namespace=...`):
    - Client: "List bookmarks/tags/phases"
    - LLM: Generates key-value mappings for namespace

5. **Get Bundle** (`POST /?cmd=getbundle`):
    - Client: "Send me changesets"
    - LLM: Generates bundle (simplified or full)

6. **Error Handling**:
    - Repository not found (404)
    - Access denied (403)
    - Custom error messages

### Action-Based Responses

**Available Actions**:

```json
{
  "actions": [
    {
      "type": "hg_capabilities",
      "capabilities": ["batch", "branchmap", "getbundle", "httpheader=1024", "known", "lookup", "pushkey", "unbundle=HG10GZ,HG10BZ,HG10UN"]
    }
  ]
}
```

```json
{
  "actions": [
    {
      "type": "hg_heads",
      "heads": ["abc123...", "def456..."]
    }
  ]
}
```

```json
{
  "actions": [
    {
      "type": "hg_branchmap",
      "branches": {
        "default": ["abc123..."],
        "stable": ["def456..."]
      }
    }
  ]
}
```

```json
{
  "actions": [
    {
      "type": "hg_listkeys",
      "keys": {
        "master": "abc123...",
        "develop": "def456..."
      }
    }
  ]
}
```

```json
{
  "actions": [
    {
      "type": "hg_send_bundle",
      "bundle_type": "HG10UN",
      "bundle_data": ""
    }
  ]
}
```

```json
{
  "actions": [
    {
      "type": "hg_error",
      "message": "Repository not found",
      "code": 404
    }
  ]
}
```

### Response Formats

**Capabilities**: Newline-separated capability strings

```
batch
branchmap
getbundle
httpheader=1024
known
lookup
```

**Heads**: Newline-separated node IDs (40-char hex)

```
a1b2c3d4e5f6789012345678901234567890abcd
1234567890abcdef1234567890abcdef12345678
```

**Branchmap**: Format: `<branch> <node1> <node2>...`

```
default abc123...
stable def456... 789abc...
```

**Listkeys**: Tab-separated key-value pairs

```
master\tabc123...
develop\tdef456...
```

**Bundles**: Binary changegroup format or empty

### Connection Management

- Each HTTP request spawned as separate tokio task
- Connections tracked in `ProtocolConnectionInfo::Mercurial` with `recent_repos: Vec<String>`
- HTTP/1.1 keep-alive handled by hyper
- No session state (each request is independent)

### Repository Parsing

**URL Path Formats**:

- `/repo-name?cmd=...` - Repository named "repo-name"
- `/?cmd=...` - Default repository
- Query parameters parsed for command and namespace

## State Management

### Per-Connection State

```rust
ProtocolConnectionInfo::Mercurial {
    recent_repos: Vec<String>,  // Track last 10 repository accesses
}
```

### No Repository Persistence

- Repositories defined in LLM prompts only
- No database or file storage
- Each server startup requires repository recreation

## Limitations

### Not Implemented

- **Push operations** (`unbundle`) - Read-only server
- **Real .hg directories** - Virtual repositories only
- **Authentication** - No access control (all requests accepted)
- **Bundle compression** - Simplified bundle handling
- **Full changegroup format** - MVP uses empty or minimal bundles
- **Advanced features**: largefiles, phases, obsolete markers, etc.

### Simplified Bundles

**Current approach**: LLM can provide:

1. **Empty bundle** - For demonstration (clone will fail gracefully)
2. **Minimal bundle** - Just enough to satisfy basic operations
3. **Future**: Full bundle generation with changeset/manifest/file data

**Why simplified**:

- Full bundle generation requires understanding Mercurial's changegroup format
- MVP focuses on protocol flow
- LLM can still generate realistic-looking responses

### LLM Interpretation Challenges

- **Node ID generation** - LLM must provide 40-character hex node IDs (can be fake)
- **Bundle format** - Complex binary format (simplified for MVP)
- **Capabilities** - Must match Mercurial client expectations

## Example Prompts and Responses

### Startup

```
listen on port 8000 via mercurial

Create virtual repository 'hello-world' with:
- default branch (node: abc123...)
- README.md containing "# Hello World\nWelcome to NetGet Mercurial!"

Allow public clones. When clients clone, provide minimal bundle.
```

### Network Event (GET /?cmd=capabilities)

**Received**:

```
GET /?cmd=capabilities HTTP/1.1
Host: localhost:8000
```

**LLM Response**:

```json
{
  "actions": [
    {
      "type": "show_message",
      "message": "Client requesting capabilities"
    },
    {
      "type": "hg_capabilities",
      "capabilities": ["batch", "branchmap", "getbundle", "httpheader=1024", "known", "lookup", "pushkey", "unbundle=HG10GZ,HG10BZ,HG10UN"]
    }
  ]
}
```

**Client Receives**:

```
batch
branchmap
getbundle
httpheader=1024
known
lookup
pushkey
unbundle=HG10GZ,HG10BZ,HG10UN
```

### Network Event (GET /?cmd=heads)

**Received**:

```
GET /?cmd=heads HTTP/1.1
Host: localhost:8000
```

**LLM Response**:

```json
{
  "actions": [
    {
      "type": "show_message",
      "message": "Client requesting heads"
    },
    {
      "type": "hg_heads",
      "heads": ["1234567890abcdef1234567890abcdef12345678"]
    }
  ]
}
```

**Client Receives**:

```
1234567890abcdef1234567890abcdef12345678
```

### Network Event (POST /?cmd=getbundle)

**Received**:

```
POST /?cmd=getbundle HTTP/1.1
Host: localhost:8000
Content-Length: 142

<bundle request parameters>
```

**LLM Response**:

```json
{
  "actions": [
    {
      "type": "show_message",
      "message": "Client requesting bundle"
    },
    {
      "type": "hg_send_bundle",
      "bundle_type": "HG10UN",
      "bundle_data": ""
    }
  ]
}
```

**Client Receives**:

```
Content-Type: application/mercurial-0.1

<empty or minimal bundle data>
```

### Error Response

**Received**:

```
GET /nonexistent?cmd=capabilities
```

**LLM Response**:

```json
{
  "actions": [
    {
      "type": "hg_error",
      "message": "Repository 'nonexistent' not found",
      "code": 404
    }
  ]
}
```

**Client Receives**:

```
HTTP/1.1 404 Not Found
Content-Type: text/plain

Error: Repository 'nonexistent' not found
```

## Honeypot Use Cases

### Fake Secret Repository

```
listen on port 8000 via mercurial

Repository '.env-backup' contains:
- .env file with fake AWS credentials
- config.ini with fake API keys

Log all clone attempts with source IP.
Track which commands are accessed.
```

### Enticing Repository Names

```
listen on port 8000 via mercurial

Create repositories:
- 'production-secrets'
- 'admin-passwords'
- 'database-backups'
- 'customer-data'

All return 404 but log access attempts and source IPs.
```

### Dynamic Content Generation

```
listen on port 8000 via mercurial

For any repository name requested:
1. Log the repository name and client IP
2. Return heads for 'default' branch
3. Generate empty bundle with message: "This is a honeypot. Access logged."
```

## Testing Strategy

### Manual Testing

```bash
# Start NetGet with Mercurial server
netget "listen on port 8000 via mercurial. Create repo 'test' with default branch."

# In another terminal, clone the repository
hg clone http://localhost:8000/test
```

### Expected Behavior

1. Mercurial client requests `/?cmd=capabilities`
2. Server sends newline-separated capabilities
3. Client requests `/?cmd=heads`
4. Server sends head node IDs
5. Client requests `/?cmd=getbundle`
6. Server sends bundle data
7. Clone succeeds (or fails gracefully with informative error)

## References

- [Mercurial Wire Protocol](https://www.mercurial-scm.org/wiki/WireProtocol)
- [HttpCommandProtocol](https://wiki.mercurial-scm.org/HttpCommandProtocol)
- [Mercurial Internals](https://hg.schlittermann.de/hg/once/help/internals.wireprotocol)
- [Changegroup Format](https://www.mercurial-scm.org/wiki/ChangeGroupFormat)

## Key Design Principles

1. **Virtual First** - No real .hg directories, all LLM-generated
2. **Read-Only MVP** - Focus on clone/pull, defer push to future
3. **Simplified Bundles** - Basic bundle support, full format later
4. **Honeypot-Ready** - Perfect for security research and logging
5. **Protocol Compliant** - Follows Mercurial HTTP wire protocol specification

## Future Enhancements

- **Push support** (`unbundle` command)
- **Real repository mode** (integrate with actual .hg directories)
- **Full bundle generation** (changeset, manifest, file data)
- **Authentication** (HTTP Basic Auth)
- **Repository persistence** (store virtual repo metadata)
- **Advanced features**: largefiles, phases, obsolete markers
- **SSH transport** (hg serve over SSH)
