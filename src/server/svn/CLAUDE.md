# SVN Protocol Implementation

## Overview
SVN (Subversion) server implementing the svn:// protocol for version control repository access. The LLM controls repository responses including directory listings, file contents, revisions, and metadata.

**Status**: Experimental (Infrastructure Protocol)
**RFC**: N/A (Custom protocol documented at https://svn.apache.org/repos/asf/subversion/trunk/subversion/libsvn_ra_svn/protocol)
**Port**: 3690 (TCP)

## Library Choices
- **Manual SVN protocol implementation** - No external library
  - No pure Rust SVN server library exists
  - Available libraries (subversion-rs) are client-only bindings to C libraries
  - SVN protocol is complex but manageable for basic commands
  - Custom parser for S-expression-like syntax: `( command args... )`
  - Manual response formatting for protocol messages

**Rationale**: Unlike HTTP or DNS, there's no mature Rust library for SVN server implementation. The official Apache Subversion is written in C. We implement a simplified version of the protocol that supports basic commands, sufficient for testing and honeypot scenarios.

**Note**: This is not a full SVN server implementation. It handles protocol basics but doesn't implement repository storage, delta algorithms, or advanced features.

## Architecture Decisions

### 1. Action-Based LLM Control
The LLM responds with actions:
- `send_svn_greeting` - Send protocol greeting with version and capabilities
- `send_svn_success` - Send success response with optional data
- `send_svn_failure` - Send error response with code and message
- `send_svn_list` - Send directory listing (items with name, kind, size, revision)
- `send_svn_response` - Send custom protocol response
- `close_connection` - Close connection

### 2. Two-Phase Protocol
SVN protocol has two phases:
1. **Greeting Phase**: Server sends greeting with protocol version (2), capabilities, and auth mechanisms
2. **Command Phase**: Client sends commands, server responds

Implementation:
- First, LLM handles `svn_greeting` event → sends greeting
- Then, loop handles `svn_command` events → LLM responds to each command

### 3. S-Expression Format
SVN protocol uses S-expression-like syntax:
- Commands: `( command-name arg1 arg2 ... )`
- Responses: `( success ( data... ) )` or `( failure ( error-info ) )`
- Lists: `( item1 item2 item3 )`
- Strings: `"quoted"` or unquoted words

**Implementation**: Simple parser splits S-expressions into command and args. Response builder constructs proper format.

### 4. Simplified Command Set
We implement basic commands:
- `get-latest-rev` - Get latest revision number
- `get-dir` - List directory contents
- `get-file` - Retrieve file contents
- `update` - Update working copy
- `stat` - Get file/dir metadata
- `log` - Get commit history

LLM can respond with fake data for any command. This is sufficient for testing SVN clients and simulating repositories.

### 5. Dual Logging
- **DEBUG**: Command summary ("SVN received ( get-latest-rev )")
- **TRACE**: Full protocol messages (both commands and responses)
- Both go to netget.log and TUI Status panel

### 6. Connection Tracking
Each SVN connection creates a connection entry:
- Connection ID: Unique per client
- Protocol info: JSON with authenticated status, repository_url, commands_processed
- Status: Active during processing, Closed after disconnect
- Tracks bytes/packets sent/received

## LLM Integration

### Event Types

#### `svn_greeting`
Triggered when SVN client first connects. Server must send protocol greeting.

Event parameters: (none)

Available actions:
- `send_svn_greeting` - Send greeting with version and capabilities

#### `svn_command`
Triggered when SVN client sends a protocol command.

Event parameters:
- `command_line` (string) - Full command line received
- `command` (string) - Parsed command name
- `args` (array) - Command arguments

Available actions:
- `send_svn_success` - Success response
- `send_svn_failure` - Error response
- `send_svn_list` - Directory listing
- `send_svn_response` - Custom response
- `close_connection` - Close connection

### Available Actions

#### `send_svn_greeting`
Send SVN protocol greeting with version and capabilities.

Parameters (all optional):
- `min_version` (number) - Minimum protocol version (default: 2)
- `max_version` (number) - Maximum protocol version (default: 2)
- `mechanisms` (array) - Auth mechanisms (default: ["ANONYMOUS"])
- `realm` (string) - Auth realm (default: "svn")

Example:
```json
{
  "type": "send_svn_greeting",
  "min_version": 2,
  "max_version": 2,
  "mechanisms": ["ANONYMOUS"],
  "realm": "svn"
}
```

Response format: `( success ( 2 2 ( ANONYMOUS ) ( edit-pipeline svndiff1 ) ) )`

#### `send_svn_success`
Send SVN success response.

Parameters:
- `message` (string, optional) - Success message (default: "success")
- `data` (string/array, optional) - Data to include in response

Example:
```json
{
  "type": "send_svn_success",
  "data": "42"
}
```

Response format: `( success ( 42 ) )`

#### `send_svn_failure`
Send SVN error response.

Parameters:
- `error_code` (number, optional) - SVN error code (default: 210000)
- `message` (string, optional) - Error message

Example:
```json
{
  "type": "send_svn_failure",
  "error_code": 210000,
  "message": "Repository not found"
}
```

Response format: `( failure ( ( 210000 0 0 0 "Repository not found" 0 0 ) ) )`

#### `send_svn_list`
Send SVN directory listing.

Parameters:
- `items` (array, required) - Array of items with fields:
  - `name` (string) - Item name
  - `kind` (string) - "file" or "dir"
  - `size` (number, optional) - File size in bytes
  - `revision` (number, optional) - Last changed revision

Example:
```json
{
  "type": "send_svn_list",
  "items": [
    {"name": "trunk", "kind": "dir", "revision": 1},
    {"name": "branches", "kind": "dir", "revision": 1},
    {"name": "README.txt", "kind": "file", "size": 1234, "revision": 5}
  ]
}
```

#### `send_svn_response`
Send custom SVN protocol response.

Parameters:
- `response` (string, required) - Raw SVN protocol response

Example:
```json
{
  "type": "send_svn_response",
  "response": "( success ( 123 ) )"
}
```

### Example LLM Responses

#### Basic Repository
```json
{
  "actions": [
    {
      "type": "send_svn_list",
      "items": [
        {"name": "trunk", "kind": "dir", "revision": 1},
        {"name": "branches", "kind": "dir", "revision": 1},
        {"name": "tags", "kind": "dir", "revision": 1}
      ]
    },
    {
      "type": "show_message",
      "message": "Sent standard SVN repository layout"
    }
  ]
}
```

#### Error Response
```json
{
  "actions": [
    {
      "type": "send_svn_failure",
      "error_code": 210005,
      "message": "Path not found"
    },
    {
      "type": "show_message",
      "message": "Sent path not found error"
    }
  ]
}
```

## Connection Management

### Connection Lifecycle
1. **Accept**: TCP connection accepted
2. **Greeting**: LLM sends protocol greeting via `svn_greeting` event
3. **Commands**: Loop reads commands, calls LLM with `svn_command` events
4. **Disconnect**: Client closes or LLM returns `close_connection`
5. **Cleanup**: Update connection status to Closed

### SVN Protocol Format
SVN uses S-expression-like syntax:
- Greeting: `( success ( min-ver max-ver ( mechanisms... ) ( capabilities... ) ) )`
- Command: `( command-name arg1 arg2 ... )`
- Success: `( success ( data... ) )`
- Failure: `( failure ( ( error-code ... "message" ... ) ) )`

## Known Limitations

### 1. Simplified Protocol Implementation
- Only implements basic command parsing (S-expressions)
- No support for binary data transfer (svndiff format)
- No delta compression
- No REPORT commands for updates
- No editor commands for commits
- Just enough to handle basic repository browsing

### 2. No Authentication
- Only ANONYMOUS mechanism supported
- No SASL authentication
- No username/password validation
- Client authentication requests are accepted but not enforced

### 3. No Repository Storage
- Responses are generated by LLM, not from real repository
- No persistent storage of files or revisions
- Can't handle actual commits (would need to store data)
- Useful for honeypots and testing, not production use

### 4. No Advanced Features
- No merge tracking
- No lock management
- No hooks or triggers
- No repository administration commands
- No repository format upgrades

### 5. Protocol Version 2 Only
- Only implements protocol version 2
- No support for older versions (1)
- No support for newer experimental versions

### 6. Text-Based Commands Only
- Reads commands line-by-line
- Assumes text protocol format
- Binary data would require different parsing
- Sufficient for most command interactions

## Example Prompts

### Basic SVN Server
```
listen on port 3690 via svn
Respond to SVN commands with a fake repository
Repository has standard layout: trunk/, branches/, tags/
Latest revision is 42
```

### Specific Repository
```
listen on port 3690 via svn
Act as SVN repository for myproject
Repository structure:
  trunk/
    src/
    docs/
    README.txt
  branches/
    feature-x/
  tags/
    v1.0/
Latest revision: 123
```

### Error Responses
```
listen on port 3690 via svn
For any path request, respond with "Path not found" error
Use error code 210005
```

### Realistic Repository
```
listen on port 3690 via svn
Simulate a software project repository
trunk/ contains: src/, tests/, docs/, README.md, LICENSE
Each directory has 3-5 files
Latest revision: 456
Author: dev@example.com
```

## Performance Characteristics

### Latency
- **With Scripting**: Sub-second (script handles commands)
- **Without Scripting**: 2-5 seconds per command (one LLM call each)
- Command parsing: ~50-100 microseconds
- Response formatting: ~50-100 microseconds

### Throughput
- **With Scripting**: Hundreds of commands per second
- **Without Scripting**: Limited by LLM (~0.2-0.5 commands/sec)
- SVN clients typically send bursts of commands during checkout/update

### Scripting Compatibility
SVN is a good candidate for scripting:
- Deterministic command-response pattern
- No complex state machine
- Predictable data structures
- Can pre-generate fake repository data

When scripting enabled:
- Server startup generates script (1 LLM call)
- All commands handled by script (0 LLM calls per command)
- Script can simulate repository state and generate responses

### Connection Duration
- Short-lived: Single command then disconnect
- Medium: Checkout/update operations (multiple commands)
- Long-lived: Rare (clients don't maintain persistent connections)

## References
- [SVN Protocol Specification](https://svn.apache.org/repos/asf/subversion/trunk/subversion/libsvn_ra_svn/protocol)
- [Apache Subversion](https://subversion.apache.org/)
- [subversion-rs (Rust client bindings)](https://github.com/jelmer/subversion-rs)
- [SVN Book](https://svnbook.red-bean.com/)
- [SVN Protocol Analysis](https://cwiki.apache.org/confluence/display/SVN/ProtocolAnalysis)
