# Redis Client Implementation

## Overview

The Redis client implementation provides LLM-controlled access to Redis servers. The LLM can execute Redis commands and
interpret responses.

## Implementation Details

### Library Choice

- **Direct TCP connection** with simplified RESP parsing
- Line-based response reading
- No external Redis library (simplified implementation)

### Architecture

```
┌────────────────────────────────────────┐
│  RedisClient::connect_with_llm_actions │
│  - Connect to Redis via TCP            │
│  - Split stream (read/write)           │
│  - Spawn read loop                     │
└────────────────────────────────────────┘
         │
         ├─► Read Loop
         │   - Read line-by-line responses
         │   - Call LLM with response
         │   - Execute follow-up commands
         │
         └─► Write Half (Arc<Mutex<WriteHalf>>)
             - Send Redis commands
             - Format: "COMMAND args\r\n"
```

### LLM Control

**Async Actions** (user-triggered):

- `execute_redis_command` - Execute Redis command
    - Parameter: command (string)
    - Examples: "GET key", "SET key value", "HGETALL hash"
- `disconnect` - Close connection

**Sync Actions** (in response to Redis responses):

- `execute_redis_command` - Execute follow-up command based on response
- `wait_for_more` - Take no action and wait for more data (use when the reply is incomplete)

**Events:**

- `redis_connected` - Fired when connection established
- `redis_response_received` - Fired when response received
    - Data includes: response (string)

### Command Format

Redis commands are sent as simple strings:

```
GET mykey\r\n
SET mykey myvalue\r\n
HGETALL user:123\r\n
```

### Structured Actions

```json
// Command action
{
  "type": "execute_redis_command",
  "command": "GET user:123:name"
}

// Response event
{
  "event_type": "redis_response_received",
  "data": {
    "response": "+OK\r\n"
  }
}
```

### Dashboard injection (`[ execute_redis_command ]`, `[ disconnect ]`)

`connect_with_llm_actions` registers a command channel
(`client::command_support::register_command_channel`) *before* the `redis_connected` LLM
call, which a manual rule can park. Because `read_line` is not cancellation-safe, commands
are drained by a separate `command_loop` task (registered with `register_client_task`)
that shares the write half, not by a `select!` arm. `execute_redis_command` yields
`ClientActionResult::Custom { name: "redis_command" }`, which the generic
`handle_stream_client_command` cannot write, so `command_loop` routes the result through
`apply_action` — the one function the connected-event path and the read loop also use to
encode commands — then records an `injected_action` access-log entry and replies with
`ClientSendOutcome::Sent { bytes_sent }`. An injected `disconnect` half-closes; the read
loop sees EOF. The handle is removed (`remove_client_handle`) on every read-loop exit and
on the connect-time early return, so the rail stops offering `[ send ]` on a dead client.
Test: `tests/client/redis/command_channel_test.rs` (zero LLM calls).

### Dual Logging

```rust
info!("Redis client {} connected", client_id);           // → netget.log
status_tx.send("[CLIENT] Redis client connected");      // → TUI
```

## Limitations

- **Simplified RESP Parsing** - Line-based, not full RESP protocol
- **No Connection Pooling** - Single connection per client
- **No Pub/Sub** - Subscribe commands not supported
- **No Pipelining** - Commands sent one at a time
- **No Authentication** - AUTH command can be sent manually
- **No Cluster Support** - Single server only

## Usage Examples

### GET Command

**User**: "Connect to Redis and get the value of user:123"

**LLM Action**:

```json
{
  "type": "execute_redis_command",
  "command": "GET user:123"
}
```

### SET Command

**User**: "Set the key 'status' to 'active'"

**LLM Action**:

```json
{
  "type": "execute_redis_command",
  "command": "SET status active"
}
```

### HGETALL Command

**User**: "Get all fields from hash user:123"

**LLM Action**:

```json
{
  "type": "execute_redis_command",
  "command": "HGETALL user:123"
}
```

## Testing Strategy

See `tests/client/redis/CLAUDE.md` for E2E testing approach.

## Future Enhancements

- **Full RESP Protocol** - Parse all RESP data types
- **Pub/Sub Support** - SUBSCRIBE, PUBLISH commands
- **Pipelining** - Batch commands for performance
- **Authentication** - Built-in AUTH handling
- **Cluster Support** - Redis Cluster client
- **Connection Pooling** - Multiple connections
