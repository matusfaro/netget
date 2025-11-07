# IRC Client Implementation

## Overview

The IRC client connects to IRC servers using a custom line-based protocol implementation. Unlike the server which uses the `irc` crate, the client implements IRC manually for fine-grained LLM control.

## Library Choices

**NO external IRC library** - Custom implementation using:
- `tokio::net::TcpStream` - Raw TCP connection
- Line-based protocol parsing (CRLF-delimited)
- Manual IRC command construction

**Rationale**: The `irc` crate is designed for full-featured IRC clients with automatic message handling, which would limit LLM control. Our custom implementation gives the LLM direct control over all IRC commands and responses.

## Architecture

### Connection Flow

1. **TCP Connect** - Connect to IRC server (typically port 6667 or 6697 for TLS)
2. **Registration** - Automatically send `NICK` and `USER` commands
3. **PING/PONG** - Automatically respond to PING to maintain connection
4. **Message Loop** - Read lines, parse IRC messages, call LLM for decisions

### IRC Message Format

```
:[source] COMMAND [params] :[trailing]
```

Examples:
```
PING :server.example.com
:nick!user@host PRIVMSG #channel :Hello world
:server 001 nick :Welcome to the IRC Network
```

### State Machine

**ConnectionState**:
- `Idle` - Ready to process messages
- `Processing` - LLM call in progress
- `Accumulating` - Queuing messages during LLM processing

This prevents concurrent LLM calls on the same client.

## LLM Integration

### Events

**Connected Event** (`irc_connected`):
- Triggered when registration completes (001 welcome message)
- Contains: `remote_addr`, `nickname`
- LLM decides: Join channels, set modes, etc.

**Message Received Event** (`irc_message_received`):
- Triggered for every IRC message (except PING)
- Contains: `source`, `command`, `target`, `message`, `raw_message`
- LLM decides: Respond with PRIVMSG, join/part channels, change nick

### Actions

**Async Actions** (user-triggered):
- `join_channel` - Join a channel
- `part_channel` - Leave a channel
- `change_nick` - Change nickname
- `disconnect` - Quit the server

**Sync Actions** (response to messages):
- `send_privmsg` - Send message to channel/user
- `send_notice` - Send notice to channel/user
- `send_raw` - Send raw IRC command
- `wait_for_more` - Don't respond yet

### Startup Parameters

- `nickname` - IRC nickname (default: `netget_user`)
- `username` - IRC username (default: `netget`)
- `realname` - IRC real name (default: `NetGet IRC Client`)

## Implementation Details

### PING/PONG Handling

PING messages are handled automatically without LLM involvement:
```rust
if line.starts_with("PING ") {
    let pong = line.replace("PING", "PONG");
    write_all(format!("{}\r\n", pong).as_bytes()).await?;
    continue;
}
```

This ensures the connection stays alive without requiring LLM responses.

### Message Parsing

The `parse_irc_message` function extracts:
- **Source** - Who sent the message (nick!user@host or server)
- **Command** - IRC command (PRIVMSG, JOIN, 001, etc.)
- **Target** - Channel or user (for PRIVMSG/NOTICE)
- **Message** - The actual message text

### Registration Flow

1. Connect to TCP socket
2. Send `NICK <nickname>`
3. Send `USER <username> 0 * :<realname>`
4. Wait for 001 (welcome) message
5. Fire `irc_connected` event to LLM

### Error Handling

- **Connection errors** - Return error immediately
- **Read errors** - Set client status to Error, disconnect
- **PING timeout** - Server disconnects (handled by server)
- **Nick collision** - LLM receives 433 message, can choose new nick

## Limitations

1. **No TLS** - Currently only plaintext IRC (port 6667)
   - Future: Add TLS support for port 6697 (IRC over TLS)

2. **No SASL** - No SASL authentication support
   - Future: Add SASL PLAIN mechanism for authenticated connections

3. **No DCC** - No Direct Client-to-Client protocol support
   - Rationale: DCC is rarely used, complex to implement

4. **No CTCP** - No Client-to-Client Protocol (VERSION, TIME, etc.)
   - Future: Add basic CTCP response actions

5. **Single Encoding** - Assumes UTF-8, doesn't handle legacy encodings
   - Rationale: Modern IRC servers use UTF-8

6. **No Message Splitting** - Long messages may be truncated by server
   - Future: Auto-split messages longer than 512 bytes

## Testing Strategy

See `tests/client/irc/CLAUDE.md` for testing details.

## Example Prompts

```
Connect to IRC at irc.libera.chat:6667 with nick testbot, join #test and say hello
```

```
Connect to IRC server localhost:6667, join #bots, respond to any message mentioning 'help'
```

```
Connect to IRC at irc.example.org:6667, join #monitoring and report any errors you see
```

## Future Enhancements

1. **TLS Support** - Use `tokio-rustls` for encrypted connections
2. **SASL Authentication** - Support SASL PLAIN and EXTERNAL
3. **CTCP Responses** - Handle VERSION, PING, TIME requests
4. **Message Splitting** - Auto-split long messages
5. **Channel State Tracking** - Track joined channels, modes, users
6. **Rate Limiting** - Prevent flooding servers with too many commands

## References

- [RFC 1459](https://tools.ietf.org/html/rfc1459) - Original IRC protocol
- [RFC 2812](https://tools.ietf.org/html/rfc2812) - IRC client protocol
- [Modern IRC Specs](https://modern.ircdocs.horse/) - Modern IRC documentation
