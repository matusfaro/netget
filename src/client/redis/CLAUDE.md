# Redis Client Implementation

## Overview

The Redis client implementation provides LLM-controlled access to Redis servers. The LLM can execute Redis commands and interpret responses.

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
