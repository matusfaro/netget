# ZooKeeper Server Implementation

## Overview

ZooKeeper is a distributed coordination service that provides a hierarchical namespace (similar to a file system) for configuration management, synchronization, and group services. This implementation provides a simplified ZooKeeper server that uses LLM to handle client requests.

## Implementation

### Library Choices

- **Protocol Parsing**: Manual binary protocol parsing
- **No external library**: Implemented from scratch to allow LLM control over responses
- **Binary Format**: ZooKeeper uses Jute serialization with length-prefixed messages

### Architecture

**Message Format**:
```
[4 bytes length][payload]

Payload for request:
[4 bytes xid][4 bytes op_type][variable data]

Payload for response:
[4 bytes xid][8 bytes zxid][4 bytes error_code][variable data]
```

**Operation Types**:
- 1: create
- 2: delete
- 3: exists
- 4: getData
- 5: setData
- 6: getACL
- 7: setACL
- 8: getChildren
- 9: sync
- 11: ping
- 12: getChildren2
- 13: check
- 14: multi

### Protocol State Machine

1. **Connection**: Client connects via TCP
2. **Request**: Client sends length-prefixed request
3. **Parse**: Server parses operation type and path
4. **LLM**: Server calls LLM with request event
5. **Response**: LLM generates response action
6. **Send**: Server sends length-prefixed response

### LLM Integration

**LLM Control Points**:
- ZNode data (what to return for getData)
- ZNode children (what to return for getChildren)
- ZNode existence (exists check)
- ZNode creation/deletion acknowledgment
- Error codes (permission denied, no node, etc.)

**Actions**:
- `zookeeper_response`: Send response with xid, zxid, error_code, and data

**Events**:
- `zookeeper_request`: Triggered when client sends a request

### Logging

**Dual logging** to both `netget.log` and TUI:
- INFO: Connection lifecycle
- DEBUG: Request summaries (operation, path)
- TRACE: Full payloads (hex-encoded)

## Limitations

1. **No Persistent Storage**: ZNodes are not stored. LLM provides all data on-demand per CLAUDE.md protocol memory policy.
2. **Simplified Protocol**: Only basic operations supported (create, delete, getData, setData, getChildren)
3. **No Watches**: Watch mechanism not implemented
4. **No Sessions**: Session management simplified (no session timeout, keepalive)
5. **No ACLs**: Access control not enforced
6. **No Multi**: Multi-operation transactions not supported
7. **Simplified Parsing**: Request parsing extracts operation type and path only

## Example Prompts

### Basic ZooKeeper Server

```
Open ZooKeeper server on port 2181

Instruction: "Act as a ZooKeeper server. Store configuration data for a distributed application.
When clients read /config/database, return connection string 'postgres://localhost:5432'.
When clients read /config/cache, return 'redis://localhost:6379'.
For any other path, return empty data."
```

### ZooKeeper with Hierarchy

```
Open ZooKeeper server on port 2181

Instruction: "Act as a ZooKeeper server with the following hierarchy:
/services (container)
/services/web (data: 'http://10.0.0.1:8080')
/services/api (data: 'http://10.0.0.2:8000')
/config (container)
/config/timeout (data: '30')
/config/retries (data: '3')

Respond to getData with the appropriate data. Respond to getChildren with child names."
```

### ZooKeeper with Dynamic Updates

```
Open ZooKeeper server on port 2181

Instruction: "Act as a ZooKeeper server. Track service registrations.
When clients create znodes under /services, store their data in memory.
When clients read /services, list all registered services.
When clients delete a service znode, remove it from the list."
```

## Testing Strategy

See `tests/server/zookeeper/CLAUDE.md` for E2E testing approach.
