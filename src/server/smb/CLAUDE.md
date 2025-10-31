# SMB Protocol Implementation

## Overview

SMB2 (Server Message Block version 2) file server implementing a subset of MS-SMB2 protocol. Provides Windows-compatible file sharing where the LLM controls the virtual filesystem, authentication, and file operations.

**Protocol**: SMB 2.1 (dialect 0x0210)
**Transport**: Direct TCP (port 445) or NetBIOS over TCP (port 139)
**Port**: 445 (standard), configurable
**Status**: Alpha

## Library Choices

- **Manual SMB2 implementation** - No library used
  - SMB2 binary protocol parsing and response generation
  - Custom packet builders for Negotiate, Session Setup, Tree Connect, etc.
  - Direct control over all protocol aspects
- **tokio::net::TcpListener** - TCP connection management

**Why manual implementation?**
- No suitable Rust SMB2 server library exists
- Full control needed for LLM integration at protocol level
- SMB2 protocol is complex but manageable for core operations
- Allows honeypot behavior (accept invalid requests, log probes)

## Architecture Decisions

### Simplified SMB2 Dialect
Implements minimal SMB 2.1 subset:
- **Negotiate Protocol** - Offer SMB 2.1 dialect (0x0210)
- **Session Setup** - Guest authentication only
- **Tree Connect** - Accept all share connections
- **Create** - Open/create files and directories
- **Read/Write** - File content operations
- **Close** - Close file handles
- **Query Info** - File attributes
- **Query Directory** - Directory listings

**Not implemented**:
- SMB 3.x features (encryption, multichannel, etc.)
- NTLM authentication (only guest)
- Opportunistic locks (oplocks)
- Durable handles
- Compound requests

### LLM-Controlled Filesystem
Similar to NFS, LLM controls entire filesystem:
- **Authentication** - LLM decides who can connect
- **File operations** - LLM provides file content, attributes
- **Directory structure** - LLM defines folders and files

### Guest-Only Authentication
Current implementation uses guest authentication:
- No password verification
- LLM can accept or deny based on username
- Session IDs allocated per connection

### File Handle Management
Server maintains file handle state:
- 16-byte GUID per file handle (generated with timestamp)
- HashMap of handles → file paths
- Handles tracked per connection

### Binary Protocol Handling
Manual SMB2 packet parsing:
- 64-byte SMB2 header parsing
- Command extraction (offset 12-13, little-endian u16)
- Response builders for each command type

## Connection Management

### TCP Connection Lifecycle
1. Client connects to TCP port
2. SMB2 Negotiate exchange
3. Session Setup (authentication)
4. Tree Connect (share connection)
5. File operations (Create, Read, Write, Close)
6. Disconnect

### Connection Tracking
Connections tracked in ServerInstance state:
- Connection ID per TCP connection
- Protocol-specific info: `ProtocolConnectionInfo::Smb { authenticated, username, session_id, open_files }`
- Stats: bytes_sent, bytes_received, packets_sent, packets_received
- Status updated on connection close

### Per-Connection State
`SmbConnectionState` maintains:
- **Sessions**: HashMap<session_id, SmbSession>
- **Trees**: HashMap<tree_id, SmbTreeConnect>
- **Files**: HashMap<file_handle, SmbFileHandle>
- Next session ID, next tree ID generators

### Concurrency
Multiple concurrent connections supported:
- Each connection handled in separate tokio task
- Connection state isolated (no shared mutable state)
- LLM calls serialized per operation

## State Management

### Server State
Minimal global state:
- Server ID for LLM context
- Connection tracking for UI

### Connection State
Per-connection state in `Arc<Mutex<SmbConnectionState>>`:
- Sessions: Maps session_id → username, authenticated flag
- Trees: Maps tree_id → share name
- Files: Maps file_id (GUID) → path, is_directory

### Filesystem State
LLM maintains filesystem via instructions:
- File paths stored in file handles
- LLM consulted for file content on demand
- No persistent storage

## Limitations

### Simplified SMB2 Implementation
- **SMB 2.1 only** - No SMB 3.x features
- **Guest auth only** - No NTLM, Kerberos, or secure authentication
- **No encryption** - Plain text protocol (no SMB3 encryption)
- **No signing** - Packets not cryptographically signed
- **No oplocks** - No opportunistic locking for performance

### Protocol Simplifications
- Fixed tree IDs and session IDs (not from request)
- Minimal header fields populated
- Timestamps often zero
- File attributes simplified

### LLM Performance
- **CRITICAL**: Every file operation calls LLM (slow)
- High latency (seconds per operation)
- Not suitable for real file sharing workloads
- Scripting mode not yet implemented for SMB

### Testing Limitations
- Real SMB clients (Windows, smbclient) have strict requirements
- Clients expect full SMB2 compliance
- Some clients probe for SMB1 (not supported)
- Testing uses raw TCP sockets, not real SMB clients

## LLM Integration

### Event-Based Processing
SMB operations trigger `SMB_OPERATION_EVENT`:
```json
{
  "operation": "read",
  "params": {
    "path": "/documents/readme.txt",
    "offset": 0,
    "length": 4096
  }
}
```

LLM receives:
- Operation name (session_setup, create, read, write, etc.)
- Structured parameters (paths, offsets, sizes)

### Action Response Format

**smb_auth_success** / **allow_auth** - Allow authentication:
```json
{
  "type": "smb_auth_success"
}
```

**smb_auth_deny** - Deny authentication:
```json
{
  "type": "smb_auth_deny",
  "message": "Access denied"
}
```

**smb_read_file** - File content:
```json
{
  "type": "smb_read_file",
  "content": "File content here"
}
```

**smb_get_file_info** - File attributes:
```json
{
  "type": "smb_get_file_info",
  "size": 4096,
  "is_directory": false,
  "created": "2024-01-15T10:30:00Z"
}
```

**smb_list_directory** - Directory listing:
```json
{
  "type": "smb_list_directory",
  "files": [
    {"name": "readme.txt", "size": 1024, "is_directory": false},
    {"name": "subdir", "size": 0, "is_directory": true}
  ]
}
```

### Error Handling
No explicit error field - LLM just omits expected action type.
Server returns default error response if action not found.

## Example Prompts and Responses

### Example 1: Basic File Server

**Prompt:**
```
Start an SMB file server on port 445. Accept all guest connections.
Provide /documents directory with readme.txt (content: "Welcome to NetGet SMB").
```

**LLM Response (session_setup):**
```json
{
  "actions": [
    {
      "type": "smb_auth_success"
    }
  ]
}
```

**LLM Response (read):**
```json
{
  "actions": [
    {
      "type": "smb_read_file",
      "content": "Welcome to NetGet SMB"
    }
  ]
}
```

### Example 2: Authentication Control

**Prompt:**
```
Start an SMB file server on port 445. Only allow user "alice" to authenticate.
Deny all other users.
```

**LLM Response (alice):**
```json
{
  "actions": [
    {
      "type": "show_message",
      "message": "Allowing alice to connect"
    },
    {
      "type": "smb_auth_success"
    }
  ]
}
```

**LLM Response (bob):**
```json
{
  "actions": [
    {
      "type": "show_message",
      "message": "Denying bob - not authorized"
    },
    {
      "type": "smb_auth_deny"
    }
  ]
}
```

### Example 3: Directory Listings

**Prompt:**
```
Start an SMB file server on port 445. /documents contains: report.pdf (1024 bytes),
presentation.pptx (4096 bytes), archive folder.
```

**LLM Response (query_directory):**
```json
{
  "actions": [
    {
      "type": "smb_list_directory",
      "files": [
        {"name": "report.pdf", "size": 1024, "is_directory": false},
        {"name": "presentation.pptx", "size": 4096, "is_directory": false},
        {"name": "archive", "size": 0, "is_directory": true}
      ]
    }
  ]
}
```

### Example 4: Write Operations

**Prompt:**
```
Start an SMB file server on port 445. Accept file writes, log the content.
```

**LLM Response (write):**
```json
{
  "actions": [
    {
      "type": "show_message",
      "message": "Client wrote 256 bytes to /documents/newfile.txt"
    }
  ]
}
```

## References

- [MS-SMB2: Server Message Block (SMB) Protocol Versions 2 and 3](https://docs.microsoft.com/en-us/openspecs/windows_protocols/ms-smb2)
- [SMB2 Wikipedia](https://en.wikipedia.org/wiki/Server_Message_Block#SMB_2.0)
- [Samba SMB Implementation](https://www.samba.org/)
- [SMB Packet Structure](https://wiki.wireshark.org/SMB2)

## Logging

### Structured Logging Levels

**TRACE** - Full SMB2 packet details:
- Hex dump of request/response packets
- Detailed header parsing
- File handle mappings

**DEBUG** - SMB2 command summaries:
- Command type and parameters
- "SMB2 CREATE /documents/readme.txt"
- "SMB2 READ fileid=0x123... offset=0 len=4096"

**INFO** - High-level events:
- Connection open/close
- Authentication attempts
- "SMB connection from 192.168.1.100"
- "SMB auth attempt: guest"
- "SMB connection closed"

**WARN** - Non-fatal issues:
- Invalid SMB2 signature
- Unknown command codes
- Malformed requests

**ERROR** - Critical failures:
- LLM communication errors
- Connection read/write failures
- Invalid packet structure

All logs use dual logging pattern (tracing macros + status_tx).
