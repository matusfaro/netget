# IRC Protocol Implementation

## Overview

IRC (Internet Relay Chat) server implementing core IRC protocol for real-time text-based communication. The LLM controls
IRC responses, handles protocol commands (NICK, USER, JOIN, PRIVMSG, PING, etc.), and manages chat server logic.

**Status**: Alpha (Application Protocol)
**RFC**: RFC 1459 (Internet Relay Chat Protocol), RFC 2812 (IRC Client Protocol)
**Port**: 6667 (plain TCP), 6697 (with TLS, not implemented)

## Library Choices

- **No IRC library** - Manual protocol implementation
    - IRC is line-based text protocol (simple parsing)
    - Commands parsed from text lines
    - Responses constructed as formatted strings
    - No complex binary protocol
- **tokio** - Async runtime and I/O
    - `TcpListener` for accepting connections
    - `BufReader` for line-based reading
    - `AsyncWriteExt` for sending responses

**Rationale**: IRC protocol is simple enough that no dedicated library is needed. Messages are text lines ending with
`\r\n`, commands are space-separated tokens. Manual implementation gives LLM full control and avoids library
constraints.

## Architecture Decisions

### 1. Action-Based LLM Control

The LLM receives raw IRC messages and responds with structured actions:

- `send_irc_message` - Send raw IRC message
- `send_irc_welcome` - Send 001 RPL_WELCOME numeric
- `send_irc_pong` - Respond to PING with PONG
- `send_irc_join` - Send JOIN confirmation
- `send_irc_part` - Send PART confirmation
- `send_irc_privmsg` - Send PRIVMSG (chat message)
- `send_irc_notice` - Send NOTICE (notification)
- `send_irc_numeric` - Send numeric response (e.g., 332 for topic)
- `wait_for_more` - Buffer more data before responding
- `close_connection` - Close client connection

### 2. Line-Based Message Processing

IRC message flow:

1. Accept TCP connection
2. Read lines with `BufReader::read_line()` (splits on `\n`)
3. Parse IRC command from line (e.g., "NICK alice\r\n")
4. Send line to LLM as `irc_message_received` event
5. LLM returns actions (e.g., `send_irc_welcome`)
6. Execute actions (send responses)
7. Loop for next line

### 3. Automatic Line Termination

All IRC messages must end with `\r\n`:

- Received messages: Preserved as-is from `read_line()`
- Sent messages: Automatically add `\r\n` if not present
- `send_irc_message`: Formats to ensure `\r\n` termination
- Other actions: Format with `\r\n` suffix

### 4. Client State Tracking (Unused Currently)

`IrcProtocol` maintains per-connection state:

```rust
struct IrcClientState {
    nickname: Option<String>,
    username: Option<String>,
    realname: Option<String>,
    channels: Vec<String>,
}
```

**Note**: State tracking is implemented but not actively used by current LLM integration. LLM maintains state through
conversation context instead. State structure is available for future enhancements (e.g., server-side channel
management).

### 5. Dual Logging

- **DEBUG**: Message summary with 100-char preview ("IRC received 15 bytes: NICK alice")
- **TRACE**: Full text message ("IRC data (text): \"NICK alice\\r\\n\"")
- Both go to netget.log and TUI Status panel

### 6. Connection Management

Each IRC client gets:

- Unique `ConnectionId`
- Entry in `ServerInstance.connections` with `ProtocolConnectionInfo::Irc`
- Tracked bytes sent/received, packets sent/received
- State: Active until client disconnects or LLM closes

## LLM Integration

### Event Type

**`irc_message_received`** - Triggered when IRC client sends a message

Event parameters:

- `message` (string) - The IRC message line received (with `\r\n`)

### Available Actions

#### `send_irc_message`

Send raw IRC message (for custom responses).

Parameters:

- `message` (required) - IRC message to send (auto-adds `\r\n` if missing)

Example:

```json
{
  "type": "send_irc_message",
  "message": ":server NOTICE * :Looking up your hostname"
}
```

#### `send_irc_welcome`

Send IRC welcome message (numeric 001 - RPL_WELCOME).

Parameters:

- `nickname` (required) - Client nickname
- `server` (optional) - Server name (default: "irc.server")
- `message` (optional) - Welcome message (default: "Welcome to the IRC Network")

Example:

```json
{
  "type": "send_irc_welcome",
  "nickname": "alice",
  "server": "irc.example.com",
  "message": "Welcome to the IRC Network, alice!"
}
```

Sends: `:irc.example.com 001 alice :Welcome to the IRC Network, alice!\r\n`

#### `send_irc_pong`

Send IRC PONG response to PING.

Parameters:

- `token` (required) - Token from PING command

Example:

```json
{
  "type": "send_irc_pong",
  "token": "1234567890"
}
```

Sends: `PONG :1234567890\r\n`

#### `send_irc_join`

Send IRC JOIN confirmation.

Parameters:

- `nickname` (required) - Client nickname
- `channel` (required) - Channel name (e.g., "#general")
- `user` (optional) - Username (default: "user")
- `host` (optional) - Hostname (default: "localhost")

Example:

```json
{
  "type": "send_irc_join",
  "nickname": "alice",
  "channel": "#general"
}
```

Sends: `:alice!user@localhost JOIN #general\r\n`

#### `send_irc_privmsg`

Send IRC PRIVMSG (chat message).

Parameters:

- `source` (required) - Source (nickname or server)
- `target` (required) - Target (nickname or channel)
- `message` (required) - Message text

Example:

```json
{
  "type": "send_irc_privmsg",
  "source": "bot",
  "target": "alice",
  "message": "Hello, alice!"
}
```

Sends: `:bot PRIVMSG alice :Hello, alice!\r\n`

#### `send_irc_numeric`

Send IRC numeric response (e.g., 332 for topic, 353 for names).

Parameters:

- `code` (required) - Numeric code (e.g., 332, 353, 366)
- `target` (required) - Target nickname
- `message` (required) - Message text
- `server` (optional) - Server name (default: "irc.server")

Example:

```json
{
  "type": "send_irc_numeric",
  "code": 332,
  "target": "alice",
  "message": "#general Welcome to our channel!"
}
```

Sends: `:irc.server 332 alice :#general Welcome to our channel!\r\n`

See `actions.rs` for complete action list including `send_irc_part`, `send_irc_notice`, etc.

## Connection Management

### Connection Lifecycle

1. **Accept**: TCP listener accepts connection
2. **Register**: Connection added to `ServerInstance` with `ProtocolConnectionInfo::Irc`
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

### 1. No Channel State Management

- Server doesn't track which users are in which channels
- No channel topic storage
- No user lists per channel
- LLM must track channel state in conversation context

**Workaround**: LLM can maintain pseudo-state through prompts and memory.

### 2. No User Authentication

- No password checking
- No SASL support
- No NickServ integration
- All users accepted

### 3. Limited Numeric Responses

- Only generic `send_irc_numeric` action
- No dedicated actions for common numerics (353 NAMES, 366 ENDOFNAMES, etc.)
- LLM must know numeric codes

**Future Enhancement**: Add dedicated actions for common numerics.

### 4. No Server-to-Server Protocol

- Single-server only
- No IRC network support
- No server linking

### 5. No TLS Support

- Plain TCP only (port 6667)
- No SSL/TLS encryption (port 6697)
- See DoT/DoH for encrypted alternatives

**Workaround**: Use reverse proxy (e.g., nginx) for TLS termination.

### 6. No Operator Commands

- No /OPER command
- No /KILL, /KICK, /BAN commands
- No channel modes (+o, +v, etc.)

### 7. No CTCP Support

- No CTCP ACTION (/me)
- No CTCP VERSION, PING, TIME
- CTCP messages treated as regular PRIVMSG

## Example Prompts

### Basic IRC Server

```
listen on port 6667 via irc
When users send NICK and USER commands, send IRC welcome (001)
When users send PING, respond with PONG
When users send JOIN #channel, confirm with JOIN message
```

### Echo Bot

```
listen on port 6667 via irc
Respond to all PRIVMSG with an echo: "You said: <message>"
Ignore NICK, USER, and PING commands (send appropriate responses)
```

### Channel Server

```
listen on port 6667 via irc
Support channels: #general, #random, #dev
When users JOIN a channel, send JOIN confirmation and channel topic (332)
When users send PRIVMSG to channel, echo to all users (simulate broadcast)
```

### Interactive Bot

```
listen on port 6667 via irc
Act as a helpful chatbot named "NetBot"
Respond to PRIVMSG with relevant information
Support commands: !help, !time, !joke
Send welcome message when users connect
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
- High message volume typical for chat

## IRC Protocol References

### Common Commands

- **NICK** - Set nickname
- **USER** - Set username and realname
- **JOIN** - Join a channel
- **PART** - Leave a channel
- **PRIVMSG** - Send message to user or channel
- **NOTICE** - Send notice (non-reply message)
- **PING** - Keepalive ping
- **PONG** - Keepalive response
- **QUIT** - Disconnect from server

### Common Numerics

- **001** - RPL_WELCOME (welcome message)
- **332** - RPL_TOPIC (channel topic)
- **353** - RPL_NAMREPLY (channel user list)
- **366** - RPL_ENDOFNAMES (end of NAMES list)
- **433** - ERR_NICKNAMEINUSE (nickname in use)

### Message Format

```
:<prefix> <command> <params> :<trailing>
```

Examples:

- `:alice!user@host PRIVMSG #general :Hello everyone`
- `:server 001 alice :Welcome to the IRC Network`
- `PING :1234567890`
- `PONG :1234567890`

## References

- [RFC 1459: Internet Relay Chat Protocol](https://datatracker.ietf.org/doc/html/rfc1459)
- [RFC 2812: Internet Relay Chat: Client Protocol](https://datatracker.ietf.org/doc/html/rfc2812)
- [IRC Numeric List](https://www.alien.net.au/irc/irc2numerics.html)
- [Modern IRC Documentation](https://modern.ircdocs.horse/)
