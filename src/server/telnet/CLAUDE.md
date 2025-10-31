# Telnet Protocol Implementation

## Overview
Telnet server implementing basic terminal server functionality for remote shell access. The LLM controls terminal responses, command handling, and interactive prompt behavior. Focuses on line-based text interaction rather than full Telnet protocol negotiation.

**Status**: Alpha (Application Protocol)
**RFC**: RFC 854 (Telnet Protocol Specification), RFC 855 (Telnet Option Specifications)
**Port**: 23 (standard Telnet port)

## Library Choices
- **nectar** - Telnet protocol library (imported but not actively used)
  - Originally intended for Telnet option negotiation
  - Currently bypassed in favor of simple line-based reading
  - Kept as dependency for potential future enhancements
- **tokio** - Async runtime and I/O
  - `TcpListener` for accepting connections
  - `BufReader` for line-based reading
  - `AsyncWriteExt` for sending responses
- **No full Telnet implementation** - Simplified text-only approach

**Rationale**: Full Telnet protocol is complex (option negotiation, IAC commands, binary mode, etc.). Current implementation treats Telnet as simple line-based text protocol, similar to raw TCP but with terminal conventions. This gives LLM control over terminal interaction without Telnet complexity.

## Architecture Decisions

### 1. Simplified Telnet Protocol
Current implementation is **Telnet-lite**:
- Line-based reading (split on newlines)
- No Telnet option negotiation (IAC, WILL, WONT, DO, DONT)
- No special character handling (IAC escape sequences)
- No binary mode
- Just raw text lines (like raw TCP)

**Rationale**: LLM can control text-based terminal interaction without understanding Telnet binary protocol. Full Telnet support can be added later if needed.

### 2. Action-Based LLM Control
The LLM receives text lines and responds with actions:
- `send_telnet_message` - Send raw message (exact bytes, no modification)
- `send_telnet_line` - Send line with auto-added `\r\n`
- `send_telnet_prompt` - Send command prompt (e.g., "$ ")
- `wait_for_more` - Buffer more data before responding
- `close_connection` - Close client connection

### 3. Line-Based Message Processing
Telnet message flow:
1. Accept TCP connection
2. Read lines with `BufReader::read_line()` (splits on `\n`)
3. Send line to LLM as `telnet_message_received` event
4. LLM returns actions (e.g., `send_telnet_line`)
5. Execute actions (send responses)
6. Loop for next line

**Note**: Very similar to IRC implementation, both use line-based text protocol.

### 4. Automatic Line Termination
Terminal convention handling:
- Received messages: Preserved as-is from `read_line()` (includes `\n`)
- `send_telnet_line`: Auto-adds `\r\n` if not present
- `send_telnet_message`: Sends exact bytes (no modification)
- `send_telnet_prompt`: Sends prompt without newline (for inline prompts like "$ ")

### 5. Dual Logging
- **DEBUG**: Message summary with 100-char preview ("Telnet received 12 bytes: help")
- **TRACE**: Full text message ("Telnet data (text): \"help\\n\"")
- Both go to netget.log and TUI Status panel

### 6. Connection Management
Each Telnet client gets:
- Unique `ConnectionId`
- Entry in `ServerInstance.connections` with `ProtocolConnectionInfo::Telnet`
- Tracked bytes sent/received, packets sent/received
- State: Active until client disconnects or LLM closes

## LLM Integration

### Event Type
**`telnet_message_received`** - Triggered when Telnet client sends a message

Event parameters:
- `message` (string) - The Telnet message line received

### Available Actions

#### `send_telnet_message`
Send raw Telnet message (exact bytes, no modification).

Parameters:
- `message` (required) - Message to send (sent as-is)

Example:
```json
{
  "type": "send_telnet_message",
  "message": "Welcome to NetGet Telnet!\r\n$ "
}
```

Use case: When you need exact control over bytes (e.g., prompt without newline).

#### `send_telnet_line`
Send line of text (automatically adds `\r\n` if not present).

Parameters:
- `line` (required) - Line of text to send

Example:
```json
{
  "type": "send_telnet_line",
  "line": "Hello! You are connected to NetGet Telnet server."
}
```

Sends: `Hello! You are connected to NetGet Telnet server.\r\n`

Use case: Most common action for sending responses.

#### `send_telnet_prompt`
Send command prompt (e.g., "$ " or "> ").

Parameters:
- `prompt` (optional) - Prompt text (default: "> ")

Example:
```json
{
  "type": "send_telnet_prompt",
  "prompt": "netget> "
}
```

Sends: `netget> ` (no newline)

Use case: Interactive shell prompts.

#### `wait_for_more`
Wait for more data before responding.

Example:
```json
{
  "type": "wait_for_more"
}
```

Use case: Multi-line input accumulation.

#### `close_connection`
Close the Telnet connection.

Example:
```json
{
  "type": "close_connection"
}
```

## Connection Management

### Connection Lifecycle
1. **Accept**: TCP listener accepts connection
2. **Register**: Connection added to `ServerInstance` with `ProtocolConnectionInfo::Telnet`
3. **Split**: Stream split into ReadHalf and WriteHalf
4. **Track**: WriteHalf stored in `Arc<Mutex<WriteHalf>>` for sending
5. **Read Loop**: Continuous line reading until disconnect
6. **Close**: Connection removed when client closes or LLM sends `close_connection`

### State Management
- `ProtocolState`: Idle/Processing/Accumulating (prevents concurrent LLM calls)
- `queued_data`: Data buffered while LLM is processing
- Connection stays in ServerInstance until closed
- UI updates on every message (bytes sent/received, last activity)

## Known Limitations

### 1. No Telnet Option Negotiation
- No IAC (Interpret As Command) handling
- No WILL/WONT/DO/DONT negotiation
- No terminal type negotiation (TTYPE)
- No window size negotiation (NAWS)
- No echo control negotiation

**Impact**: Client terminal emulators may send option negotiation that's ignored. Basic text I/O works, but advanced terminal features don't.

### 2. No Special Character Handling
- No Ctrl+C (interrupt) detection
- No Ctrl+D (EOF) handling
- No escape sequence processing
- No color/formatting codes

**Workaround**: Use raw TCP if advanced terminal control is needed, or add Telnet protocol parsing.

### 3. No TLS/SSH Support
- Plain TCP only (port 23)
- No encryption
- Credentials sent in clear text

**Security Risk**: Telnet is insecure by design. Use SSH for production (not implemented).

### 4. Line-Based Only
- No character-by-character input (like raw terminal mode)
- No backspace editing on server side
- User's terminal handles line editing, server gets full lines

### 5. Feature Gate Required
Implementation is conditionally compiled:
```rust
#[cfg(feature = "telnet")]
impl TelnetServer { ... }
```

Must compile with `--features telnet` to enable protocol.

## Example Prompts

### Basic Echo Server
```
listen on port 23 via telnet
Send "Welcome to NetGet Telnet" when clients connect
Echo back any text you receive with "> " prefix
```

### Interactive Shell
```
listen on port 23 via telnet
Act as a simple shell
Send "$ " prompt after each response
Support commands:
  - help: show available commands
  - date: show current date/time
  - echo <text>: echo the text
  - exit: close connection
```

### Command Server
```
listen on port 23 via telnet
Respond to commands:
  - status: return "System OK"
  - info: return system information
  - quit: disconnect
Show "netget> " prompt after each command
```

### Multi-Line Input
```
listen on port 23 via telnet
Collect multi-line input until user sends "END"
Then process all lines and respond
Show "... " prompt for continuation lines
```

## Performance Characteristics

### Latency
- **Per Message (with scripting)**: Sub-millisecond
- **Per Message (without scripting)**: 2-5 seconds (LLM call)
- Line parsing: <1 microsecond
- Message formatting: <1 microsecond

### Throughput
- **With Scripting**: Thousands of messages per second
- **Without Scripting**: ~0.2-0.5 messages per second (LLM-limited)
- Concurrent connections: Unlimited (bounded by system resources)
- Each connection processes independently

### Scripting Compatibility
Good scripting candidate:
- Text-based protocol (easy to parse and generate)
- Repetitive command/response patterns
- State can be maintained in script
- Interactive prompts are deterministic

## Telnet vs SSH Comparison

| Feature | Telnet (NetGet) | SSH |
|---------|----------------|-----|
| Encryption | None | Yes (strong) |
| Authentication | None | Yes (keys/passwords) |
| Port | 23 | 22 |
| Security | Insecure | Secure |
| Complexity | Low | High |
| Use Case | Testing, LAN | Production |

**Recommendation**: Use Telnet for testing only. For production, use SSH protocol (see `src/server/ssh/CLAUDE.md`).

## Terminal Emulation Notes

### What Works
- Basic text I/O
- Line-based input
- Echo back responses
- Simple prompts

### What Doesn't Work
- ANSI color codes (sent as raw text)
- Cursor positioning
- Terminal size detection
- Character-by-character input
- Backspace/delete handling on server side
- Special key handling (arrow keys, function keys)

### Client Compatibility
Tested with:
- `telnet` command-line client (works)
- PuTTY (works in basic mode)
- Raw TCP with netcat (works, essentially same as Telnet-lite)

## Security Considerations

### Telnet Security Issues
- **No encryption**: All data sent in clear text
- **No authentication**: No password protection by default
- **Network sniffing**: Credentials easily captured
- **MITM attacks**: No integrity protection

### When to Use Telnet
- **Development/testing only**: Local network testing
- **Legacy systems**: Compatibility with old systems
- **Behind VPN**: When already on encrypted connection
- **Non-sensitive data**: Public information only

### When NOT to Use Telnet
- **Production systems**: Use SSH instead
- **Over internet**: Use SSH or HTTPS
- **With credentials**: Use SSH with key authentication
- **Sensitive data**: Use encrypted protocols

## References
- [RFC 854: Telnet Protocol Specification](https://datatracker.ietf.org/doc/html/rfc854)
- [RFC 855: Telnet Option Specifications](https://datatracker.ietf.org/doc/html/rfc855)
- [RFC 1123: Requirements for Internet Hosts (Telnet)](https://datatracker.ietf.org/doc/html/rfc1123#section-3)
- [Wikipedia: Telnet](https://en.wikipedia.org/wiki/Telnet)
- [Why Telnet is Insecure](https://www.ssh.com/academy/ssh/telnet)
