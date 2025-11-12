# ZooKeeper Client Implementation

## Overview

ZooKeeper client implementation that connects to ZooKeeper servers and allows LLM-controlled operations such as creating znodes, reading/writing data, and watching for changes.

## Implementation

### Library Choices

- **zookeeper-async v0.7**: Modern async Rust ZooKeeper client built on tokio
- **Binary Protocol**: Full ZooKeeper wire protocol support
- **Session Management**: Automatic session keepalive and reconnection

### Architecture

**Connection Flow**:
1. Client connects to ZooKeeper server via TCP (default port 2181)
2. Session established with timeout negotiation
3. LLM controls all operations via actions
4. Watchers can be set up for change notifications

**Operations**:
- `create_znode`: Create a new znode at specified path
- `get_data`: Read data from a znode
- `set_data`: Write data to a znode
- `delete_znode`: Delete a znode
- `get_children`: Get list of child znodes

### LLM Integration

**LLM Control Points**:
- ZNode creation (path, data, flags)
- Data reading and writing
- ZNode deletion
- Children listing
- Watch setup for notifications

**Actions**:
- Async: `create_znode`, `get_data`, `set_data`, `delete_znode`, `get_children`, `modify_instruction`
- Sync: `wait_for_more`, `disconnect`

**Events**:
- `zookeeper_connected`: Client connected successfully
- `zookeeper_data_received`: Data received from getData operation
- `zookeeper_children_received`: Children list received from getChildren

### Logging

**Dual logging** to both `netget.log` and TUI:
- INFO: Connection lifecycle
- DEBUG: Operation summaries (create, get, set, delete)
- TRACE: Full request/response details

## Limitations

1. **Simplified Implementation**: Basic operations only (no transactions, ACLs, or advanced features)
2. **No Watch Mechanism**: Change watches not fully implemented
3. **No Persistent Sessions**: Sessions not preserved across restarts
4. **Synchronous Operations**: Operations are synchronous (no pipelining)

## Example Usage

### Connect and Read Configuration

```
open_client zookeeper localhost:2181

Instruction: "Connect to ZooKeeper and read configuration from /myapp/config.
Log the configuration data."
```

### Service Discovery

```
open_client zookeeper localhost:2181

Instruction: "Monitor service registrations under /services.
List all services by getting children of /services.
For each service, read its data to get the endpoint."
```

### Dynamic Configuration Update

```
open_client zookeeper localhost:2181

Instruction: "Read current configuration from /myapp/timeout.
If timeout is less than 30 seconds, update it to 30.
Verify the update by reading it again."
```

### Hierarchical Data Management

```
open_client zookeeper localhost:2181

Instruction: "Create hierarchical structure:
/myapp (container)
/myapp/config (data: 'prod')
/myapp/services (container)

Then list all children under /myapp to verify."
```

## Testing Strategy

See `tests/client/zookeeper/CLAUDE.md` for E2E testing approach.
