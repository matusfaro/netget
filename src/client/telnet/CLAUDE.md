# Telnet Client Implementation

## Overview

The Telnet client implementation provides LLM-controlled outbound Telnet connections. The LLM can connect to Telnet servers, send commands, handle option negotiations, and interpret server responses.

## Implementation Details

### Library Choice
- **tokio::net::TcpStream** - Async TCP connection
- **Custom Telnet Protocol Parser** - Handle IAC commands and option negotiation
- No external Telnet library needed (protocol is simple enough)

### Architecture

```
┌──────────────────────────────────────────────┐
│  TelnetClient::connect_with_llm_actions      │
│  - Connect to remote address                 │
│  - Split stream (read/write)                 │
│  - Spawn read loop                           │
└──────────────────────────────────────────────┘
         │
         ├─► Read Loop
         │   - Read raw TCP data
         │   - Parse Telnet protocol (IAC commands)
         │   - Handle option negotiation automatically
         │   - Extract actual text data
         │   - Call LLM with data_received event
         │   - Execute actions (send_command, send_text)
         │   - State machine (Idle/Processing/Accumulating)
         │
         └─► Write Half (Arc<Mutex<WriteHalf>>)
             - Shared for sending data
             - Used by action execution & negotiation
```

### Telnet Protocol Handling

**Protocol Constants:**
```
IAC (Interpret As Command) = 255 (0xFF)
WILL = 251 (0xFB) - Server offers to enable option
WONT = 252 (0xFC) - Server refuses option
DO = 253 (0xFD) - Server requests client enable option
DONT = 254 (0xFE) - Server requests client disable option
SB = 250 (0xFA) - Subnegotiation begin
SE = 240 (0xF0) - Subnegotiation end
```

**Negotiation Strategy:**
- Server sends WILL <option> → Client responds DONT (refuse)
- Server sends DO <option> → Client responds WONT (refuse)
- Simple "refuse all" strategy keeps implementation straightforward
- LLM doesn't need to understand option negotiation details

**Supported Options** (for logging only):
- ECHO (1)
- SUPPRESS_GO_AHEAD (3)
- TERMINAL_TYPE (24)
- WINDOW_SIZE (31)
- And others (see `get_option_name()`)

### Connection State Machine

**States:**
1. **Idle** - No LLM processing happening
2. **Processing** - LLM is being called, new data queued
3. **Accumulating** - LLM still processing, accumulating more data

**Transitions:**
- Idle → Processing: Data received, call LLM
- Processing → Accumulating: More data arrives during LLM call
- Accumulating → Accumulating: More data while LLM processing
- Processing/Accumulating → Idle: LLM returns, process queue

### LLM Control

**Async Actions** (user-triggered):
- `send_command` - Send command with newline appended (`cmd\r\n`)
- `send_text` - Send raw text without newline
- `disconnect` - Close connection

**Sync Actions** (in response to received data):
- `send_command` - Send command as response
- `send_text` - Send text as response
- `wait_for_more` - Don't respond yet, accumulate data

**Events:**
- `telnet_connected` - Fired when connection established
- `telnet_data_received` - Fired when text data received
  - `data` field: UTF-8 text (Telnet commands stripped)
  - `raw_hex` field: Raw bytes including IAC commands
- `telnet_option_negotiated` - Fired when option negotiation occurs (informational)

### Data Encoding

**Received Data:**
```json
{
  "data": "login: ",
  "raw_hex": "ff fb 01 ff fb 03 6c 6f 67 69 6e 3a 20"
}
```

**Sent Actions:**
```json
{
  "type": "send_command",
  "command": "whoami"
}
// Sends: "whoami\r\n"

{
  "type": "send_text",
  "text": "password"
}
// Sends: "password" (no newline)
```

### Dual Logging

```rust
info!("Telnet client {} connected", client_id);           // → netget.log
status_tx.send("[CLIENT] Telnet client connected");      // → TUI
debug!("Telnet client {} received WILL ECHO", client_id); // → netget.log
```

### Connection Lifecycle

1. **Connect**: `TcpStream::connect(remote_addr)`
2. **Connected**: Update ClientStatus::Connected
3. **Negotiation**: Automatically respond to option negotiations
4. **Data Flow**: Read loop processes incoming data, strips Telnet commands
5. **LLM Interaction**: LLM sees clean text, sends commands
6. **Disconnect**: ClientStatus::Disconnected or Error

### Error Handling

- **Connection Failed**: Return error, client stays in Error state
- **Read Error**: Log, update status to Error, break loop
- **Write Error**: Log, connection may close
- **LLM Error**: Log, continue accepting data
- **Invalid UTF-8**: Use lossy conversion (show � for invalid bytes)

### Telnet Protocol Edge Cases

**IAC Escaping:**
- IAC IAC (255 255) = Literal byte 255
- Handled in `parse_telnet_data()`

**Subnegotiation:**
- IAC SB ... IAC SE sequences
- Skipped/ignored (not relevant for basic shell usage)

**Incomplete Sequences:**
- If buffer ends mid-IAC sequence, may lose command
- Acceptable for LLM-controlled client (rare edge case)

## Limitations

- **No TLS Support** - Raw Telnet only (not Telnet over TLS)
- **No Reconnection** - Must manually reconnect via action
- **Simple Option Negotiation** - Refuses all options (good for most servers)
- **No Line Mode** - Character-at-a-time mode (some servers may expect line mode)
- **No Authentication Helpers** - LLM must handle login prompts manually
- **UTF-8 Only** - Non-UTF-8 encodings converted lossily

## Use Cases

**Typical LLM Flows:**

1. **Interactive Shell Session:**
   ```
   User: "Connect to telnet://localhost:23 and run 'ls'"
   LLM: Connects, waits for login prompt
   Server: "login: "
   LLM: Sends "user\r\n"
   Server: "Password: "
   LLM: Sends "password\r\n"
   Server: "$ "
   LLM: Sends "ls\r\n"
   Server: "file1\nfile2\n$ "
   LLM: Parses output, reports back to user
   ```

2. **Automated Command Execution:**
   ```
   User: "Check uptime on remote server"
   LLM: Sends "uptime\r\n" after login
   LLM: Extracts uptime from response
   ```

3. **Service Testing:**
   ```
   User: "Test if Telnet service is running"
   LLM: Connects, verifies banner
   LLM: Reports service status
   ```

## Testing Strategy

See `tests/client/telnet/CLAUDE.md` for E2E testing approach.

## References

- RFC 854: Telnet Protocol Specification
- RFC 855: Telnet Option Specifications
- RFC 1073: Telnet Window Size Option
- RFC 1091: Telnet Terminal Type Option
