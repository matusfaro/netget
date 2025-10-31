# Redis Protocol Implementation

## Overview

Redis server implementing RESP2 (REdis Serialization Protocol v2) using the `redis-protocol` crate for parsing and manual encoding for responses. The server handles all Redis command types with full LLM control over command responses.

**Port**: 6379 (default Redis port)
**Protocol Version**: RESP2 (Redis 2.0+)
**Stack Representation**: `ETH>IP>TCP>REDIS`

## Library Choices

**redis-protocol** (v5.2):
- Chosen for robust RESP2 frame parsing
- Provides `decode()` function for parsing RESP frames
- `OwnedFrame` enum represents all RESP data types
- Does NOT provide response encoding (we implement manually)
- Limitation: No RESP3 support (Redis 6.0+)

**Manual Response Encoding**:
- LLM controls all command responses through action system
- Responses manually encoded to RESP2 format
- Helper functions: `encode_simple_string`, `encode_bulk_string`, `encode_integer`, `encode_array`, `encode_error`, `encode_null`
- Follows RESP2 specification exactly

## Architecture Decisions

### Frame-Based Processing
- Read data into buffer from TcpStream
- Parse RESP frames using `redis_protocol::decode()`
- Extract command from frame (typically Array of BulkStrings)
- Send command to LLM via event system
- Encode LLM response actions to RESP2 format
- Write encoded bytes directly to TcpStream
- Continue processing remaining frames in buffer

### Connection Handler Design
- Each connection spawns a `RedisHandler` task
- Handler owns: connection_id, llm_client, app_state, protocol
- Single async loop processes frames sequentially
- No state persistence between commands (stateless)
- Buffer management: accumulate partial frames, drain processed bytes

### Command Execution Flow
1. Client sends RESP2 frame (e.g., `*2\r\n$3\r\nGET\r\n$3\r\nkey\r\n`)
2. Decode frame to `OwnedFrame::Array`
3. Convert frame to command string (e.g., "GET key")
4. Create `REDIS_COMMAND_EVENT` with command string
5. Call LLM via `call_llm()` with event and protocol
6. Process action results:
   - `redis_simple_string`: Encode and send `+OK\r\n`
   - `redis_bulk_string`: Encode and send `$5\r\nhello\r\n`
   - `redis_integer`: Encode and send `:42\r\n`
   - `redis_array`: Encode and send `*3\r\n$5\r\nvalue1\r\n...`
   - `redis_error`: Encode and send `-ERR message\r\n`
   - `redis_null`: Encode and send `$-1\r\n`
   - `close_connection`: Close connection and exit
7. If no action found, no response sent (client hangs)

### RESP2 Encoding
- **Simple Strings**: `+{value}\r\n` (status replies like OK, PONG)
- **Bulk Strings**: `${length}\r\n{data}\r\n` (binary-safe strings)
- **Integers**: `:{value}\r\n` (numeric responses)
- **Arrays**: `*{count}\r\n{element1}{element2}...` (lists, sets, nested structures)
- **Errors**: `-{message}\r\n` (error responses, start with ERR, WRONGTYPE, etc.)
- **Null**: `$-1\r\n` (nil/null value)

### Type Handling
- JSON values from LLM converted to appropriate RESP types
- String → Bulk String
- Number → Integer or Bulk String
- Boolean → Bulk String ("1" or "0")
- Null → Null Bulk String
- Array → RESP Array (recursive encoding)
- Object → JSON-encoded Bulk String

## LLM Integration

### Action-Based Responses

**Sync Actions** (network event context required):
- `redis_simple_string`: Return status reply (OK, PONG)
- `redis_bulk_string`: Return string value (GET, SET)
- `redis_integer`: Return numeric value (INCR, DEL count)
- `redis_array`: Return array of values (MGET, KEYS)
- `redis_error`: Return error message (ERR, WRONGTYPE)
- `redis_null`: Return nil value (nonexistent keys)
- `close_connection`: Close connection (QUIT command)

**Event Types**:
- `REDIS_COMMAND_EVENT`: Fired for every command
  - Data: `{ "command": "GET mykey" }`

### Example LLM Prompts

**PING command**:
```
For PING commands, use redis_simple_string with value='PONG'
```

**GET/SET commands**:
```
For SET commands, use redis_simple_string value='OK'
For GET commands, use redis_bulk_string value='myvalue'
```

**Numeric commands**:
```
For INCR commands, use redis_integer value=42
For DEL commands, use redis_integer value=1
```

**Array commands**:
```
For KEYS * commands, use redis_array values=['key1','key2','key3']
For MGET commands, use redis_array values=['value1','value2']
```

**Nonexistent keys**:
```
For GET nonexistent commands, use redis_null
```

**Error responses**:
```
For INVALID commands, use redis_error message='ERR unknown command'
```

## Connection Management

### Connection Lifecycle
1. Server accepts TCP connection on port 6379
2. Create `RedisHandler` with unique `ConnectionId`
3. Add connection to `ServerInstance` with `ProtocolConnectionInfo::Redis`
4. Spawn async task running handler loop
5. Handler processes commands until disconnect or `close_connection` action
6. Connection marked closed in ServerInstance

### State Tracking
- Connection state stored in `ServerInstance.connections` HashMap
- Tracks: remote_addr, local_addr, bytes_sent/received, packets_sent/received
- Status: Active or Closed
- Last activity timestamp updated per command

### Concurrency
- Multiple connections handled concurrently
- Each connection has independent handler and buffer
- No shared state between connections
- LLM calls queued per connection via `ProtocolConnectionInfo`

## Limitations

### Protocol Features
- **RESP2 only** - no RESP3 support (no push events, doubles, sets)
- **No authentication** - AUTH command not implemented
- **No database selection** - SELECT command ignored
- **No transactions** - MULTI/EXEC not implemented
- **No pub/sub** - PUBLISH/SUBSCRIBE not implemented
- **No pipelining optimization** - commands processed sequentially
- **No Lua scripting** - EVAL/EVALSHA not supported
- **No persistence** - no RDB/AOF, data not saved
- **No clustering** - single-node only
- **No replication** - no master/slave

### Performance
- Each command triggers LLM call (unless scripting is used)
- No command batching or optimization
- Synchronous command processing per connection
- Buffer allocation per connection (4KB chunks)

### Error Handling
- If LLM returns no action, client hangs (no response sent)
- Unknown actions silently ignored
- Malformed RESP frames cause connection close

## Known Issues

1. **No response fallback**: If LLM fails to return a valid action, no response sent to client (client hangs)
2. **Blocking commands**: BLPOP, BRPOP would block entire connection (not suitable for single-threaded handler)
3. **Large values**: Very large bulk strings may cause memory issues (no streaming)
4. **Client commands**: CLIENT SETNAME, CLIENT LIST not implemented
5. **INFO command**: Would require hardcoded server stats

## Example Responses

### Simple String (PING → PONG)
```json
{
  "actions": [
    {
      "type": "redis_simple_string",
      "value": "PONG"
    }
  ]
}
```

### Bulk String (GET mykey)
```json
{
  "actions": [
    {
      "type": "redis_bulk_string",
      "value": "myvalue"
    }
  ]
}
```

### Integer (INCR counter)
```json
{
  "actions": [
    {
      "type": "redis_integer",
      "value": 42
    }
  ]
}
```

### Array (KEYS *)
```json
{
  "actions": [
    {
      "type": "redis_array",
      "values": ["key1", "key2", "key3"]
    }
  ]
}
```

### Null (GET nonexistent)
```json
{
  "actions": [
    {
      "type": "redis_null"
    }
  ]
}
```

### Error (INVALID command)
```json
{
  "actions": [
    {
      "type": "redis_error",
      "message": "ERR unknown command"
    }
  ]
}
```

## References

- [RESP2 Protocol Specification](https://redis.io/docs/reference/protocol-spec/)
- [redis-protocol crate](https://docs.rs/redis-protocol/)
- [redis-rs client](https://docs.rs/redis/) - used in tests
- [Redis Commands Reference](https://redis.io/commands/)
