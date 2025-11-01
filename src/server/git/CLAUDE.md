# Git Smart HTTP Server Implementation

## Overview

Git Smart HTTP server implementing the Git Smart HTTP protocol (HTTP-based git clone/fetch). The LLM controls virtual repositories, reference advertisement, and pack file generation. This is a **read-only implementation** (clone/fetch only - no push support).

## Protocol Version

- **Git Smart HTTP**: Protocol used by `git clone http://...` and `git fetch`
- **Transport**: HTTP/1.1 GET and POST
- **Endpoints**:
  - `GET /info/refs?service=git-upload-pack` - Reference discovery
  - `POST /git-upload-pack` - Pack negotiation and transfer
- **Format**: Git pkt-line protocol (4-byte hex length + data)

## Library Choices

### Core Dependencies
- **hyper** (v1) - HTTP/1.1 server implementation
  - Chosen for: Existing NetGet infrastructure, async/await support
  - Used for: HTTP request/response processing
- **base64** - Base64 encoding/decoding
  - Used for: Encoding pack file data from LLM responses
- **No Git library** - Manual implementation for maximum LLM control
  - Rationale: Git protocol is simple enough to implement directly, provides full flexibility

### Why No git2-rs for Server?
- **git2-rs** (libgit2 bindings) is client-focused, no built-in server mode
- **gitoxide/gix** server components are incomplete
- **Manual implementation** provides:
  - Full LLM control over protocol responses
  - Virtual repositories without real .git directories
  - Flexibility in pack generation

## Architecture Decisions

### Virtual Repositories
**No Real .git Directories** - All repository content is LLM-generated:
1. **Repository metadata**: Stored in memory (not yet implemented - future enhancement)
2. **References (branches/tags)**: LLM provides list with fake/real commit SHAs
3. **Pack files**: LLM generates base64-encoded pack data (or simplified version)
4. **No persistence**: Repositories exist only during server lifetime

**Why Virtual**:
- Maximum flexibility - LLM can create any content
- No filesystem dependencies
- Perfect for honeypots (serve fake repositories)
- Ideal for testing/demonstrations

### LLM Control Points

**Complete Repository Control** - LLM implements all Git operations:

1. **Reference Discovery** (`GET /info/refs`):
   - Client: "What branches/tags exist?"
   - LLM: Generates list of refs with commit SHAs

2. **Pack Generation** (`POST /git-upload-pack`):
   - Client: "Send me objects for these SHAs"
   - LLM: Generates pack file (simplified or full)

3. **Error Handling**:
   - Repository not found (404)
   - Access denied (403)
   - Custom error messages

### Action-Based Responses

**Available Actions**:

```json
{
  "actions": [
    {
      "type": "git_advertise_refs",
      "refs": [
        {"name": "refs/heads/main", "sha": "abc123..."},
        {"name": "refs/tags/v1.0", "sha": "def456..."}
      ],
      "capabilities": ["multi_ack", "side-band-64k", "ofs-delta"]
    }
  ]
}
```

```json
{
  "actions": [
    {
      "type": "git_send_pack",
      "pack_data": "<base64 encoded pack file>"
    }
  ]
}
```

```json
{
  "actions": [
    {
      "type": "git_error",
      "message": "Repository not found",
      "code": 404
    }
  ]
}
```

### Pkt-Line Format

Git uses **pkt-line format**: 4-byte hex length (including the 4 bytes) + data

```
Example:
"001e# service=git-upload-pack\n"
 ^^^^ = 0x001e = 30 bytes total (4 + 26)
```

**Special packets**:
- `0000` - Flush packet (marks end of section)

**Implementation**:
```rust
fn format!("{:04x}{}", data.len() + 4, data)
```

### Connection Management
- Each HTTP request spawned as separate tokio task
- Connections tracked in `ProtocolConnectionInfo::Git` with `recent_repos: Vec<String>`
- HTTP/1.1 keep-alive handled by hyper
- No session state (each request is independent)

### Repository Parsing
**URL Path Formats**:
- `/repo-name/info/refs` - Repository named "repo-name"
- `/info/refs` - Default repository
- `/repo-name/git-upload-pack` - Upload pack for "repo-name"

## State Management

### Per-Connection State
```rust
ProtocolConnectionInfo::Git {
    recent_repos: Vec<String>,  // Track last 10 repository accesses
}
```

### No Repository Persistence
- Repositories defined in LLM prompts only
- No database or file storage
- Each server startup requires repository recreation

## Limitations

### Not Implemented
- **Push operations** (`git-receive-pack`) - Read-only server
- **Dumb HTTP protocol** - Only Smart HTTP supported
- **Real .git directories** - Virtual repositories only
- **Authentication** - No access control (all requests accepted)
- **Pack negotiation** - Simplified (LLM sees wants/haves but full negotiation not implemented)
- **Delta compression** - Pack files may be uncompressed
- **Protocol v2** - Only protocol v1 supported

### Simplified Pack Files
**Current approach**: LLM can provide:
1. **Base64-encoded minimal pack** - Just enough to satisfy `git clone`
2. **Empty pack** - For demonstration (clone will fail)
3. **Future**: Full pack generation with tree/blob/commit objects

**Why simplified**:
- Full pack generation requires understanding Git object format
- MVP focuses on protocol flow
- LLM can still generate realistic-looking responses

### LLM Interpretation Challenges
- **SHA generation** - LLM must provide 40-character hex SHAs (can be fake)
- **Pack format** - Complex binary format (simplified for MVP)
- **Capabilities** - Must match Git client expectations

## Example Prompts and Responses

### Startup
```
listen on port 9418 via git

Create virtual repository 'hello-world' with:
- main branch (SHA: 1234567890abcdef...)
- README.md containing "# Hello World\nWelcome to NetGet Git!"
- src/main.rs containing "fn main() { println!(\"Hello\"); }"

Allow public clones. When clients clone, provide minimal pack file.
```

### Network Event (GET /info/refs)
**Received**:
```
GET /hello-world/info/refs?service=git-upload-pack HTTP/1.1
Host: localhost:9418
```

**LLM Response**:
```json
{
  "actions": [
    {
      "type": "show_message",
      "message": "Client requesting refs for 'hello-world'"
    },
    {
      "type": "git_advertise_refs",
      "refs": [
        {"name": "refs/heads/main", "sha": "1234567890abcdef1234567890abcdef12345678"}
      ],
      "capabilities": ["multi_ack", "side-band-64k", "ofs-delta"]
    }
  ]
}
```

**Client Receives** (pkt-line format):
```
001e# service=git-upload-pack\n
0000
00541234567890abcdef1234567890abcdef12345678 refs/heads/main\0multi_ack side-band-64k ofs-delta\n
0000
```

### Network Event (POST /git-upload-pack)
**Received**:
```
POST /hello-world/git-upload-pack HTTP/1.1
Host: localhost:9418
Content-Length: 142

<pkt-line format pack negotiation>
```

**LLM Response**:
```json
{
  "actions": [
    {
      "type": "show_message",
      "message": "Client requesting pack for 'hello-world'"
    },
    {
      "type": "git_send_pack",
      "pack_data": "UEFDSwAAABo...=="
    }
  ]
}
```

**Client Receives**:
```
Content-Type: application/x-git-upload-pack-result

<base64 decoded pack file bytes>
```

### Error Response
**Received**:
```
GET /nonexistent/info/refs?service=git-upload-pack
```

**LLM Response**:
```json
{
  "actions": [
    {
      "type": "git_error",
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

### Fake Credential Trap
```
listen on port 9418 via git

Repository '.env-backup' contains:
- .env file with fake AWS credentials
- config.json with fake API keys

Log all clone attempts with source IP.
Track which files are accessed in clone requests.
```

### Enticing Repository Names
```
listen on port 9418 via git

Create repositories:
- 'production-secrets'
- 'admin-credentials'
- 'database-backups'
- 'customer-data'

All return 404 but log access attempts and source IPs.
```

### Dynamic Content Generation
```
listen on port 9418 via git

For any repository name requested:
1. Log the repository name and client IP
2. Return refs for 'main' branch
3. Generate pack with README.md: "This is a honeypot. Access logged."
```

## Testing Strategy

### Manual Testing
```bash
# Start NetGet with Git server
netget "listen on port 9418 via git. Create repo 'test' with main branch."

# In another terminal, clone the repository
git clone http://localhost:9418/test
```

### Expected Behavior
1. Git client requests `/test/info/refs?service=git-upload-pack`
2. Server sends pkt-line formatted refs
3. Git client requests `/test/git-upload-pack` with wants
4. Server sends pack file
5. Clone succeeds (or fails gracefully with informative error)

## References

- [Git Smart HTTP Protocol](https://git-scm.com/docs/http-protocol)
- [Git Pack Protocol](https://git-scm.com/docs/pack-protocol)
- [Pkt-Line Format](https://git-scm.com/docs/protocol-common#_pkt_line_format)
- [git-upload-pack](https://git-scm.com/docs/git-upload-pack)

## Key Design Principles

1. **Virtual First** - No real .git directories, all LLM-generated
2. **Read-Only MVP** - Focus on clone/fetch, defer push to future
3. **Simplified Packs** - Basic pack support, full compression later
4. **Honeypot-Ready** - Perfect for security research and logging
5. **Protocol Compliant** - Follows Git Smart HTTP specification

## Future Enhancements

- **Push support** (`git-receive-pack` endpoint)
- **Real repository mode** (integrate git2-rs for actual .git directories)
- **Full pack generation** (tree, blob, commit objects with delta compression)
- **Authentication** (HTTP Basic Auth)
- **Protocol v2** support
- **Repository persistence** (store virtual repo metadata in database/file)
