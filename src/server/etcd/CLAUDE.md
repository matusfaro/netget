# etcd Protocol Implementation

## Overview
etcd v3 distributed key-value store server implementing the gRPC-based KV service. The LLM controls all key-value operations through actions.

**Status**: Alpha
**Protocol Version**: etcd v3 (gRPC)
**RFC**: etcd v3 API specification
**Port**: 2379 (default)

## Library Choices

### Core Dependencies
- **tonic** v0.12 - gRPC server framework
  - Used for HTTP/2 connection handling
  - Provides gRPC service infrastructure
- **prost** v0.13 - Protocol Buffers implementation
  - Used for encoding/decoding protobuf messages
  - Handles binary wire format
- **hyper** v1.5 - HTTP/2 server
  - Underlying transport for gRPC
  - Connection management and framing

### Official Protobuf Definitions
Unlike the dynamic gRPC server, etcd uses **pre-compiled** protobuf schemas from official etcd sources:
- `proto/etcd/rpc.proto` - KV service definition (simplified from official)
- `proto/etcd/kv.proto` - KeyValue message definitions (simplified from official)
- Generated at build time via `tonic-build` in `build.rs`

**Rationale**: Using official protobuf definitions ensures compatibility with real etcd clients (`etcd-client`, `etcdctl`, etc.) while maintaining LLM control over the actual key-value operations.

## Architecture Decisions

### 1. Static Schema, Dynamic Logic
**Design**: Pre-compiled protobuf schema + LLM-controlled responses
- Schema: Fixed at compile time (official etcd v3 KV service)
- Logic: LLM decides what keys/values to return for each operation
- Benefits: Client compatibility + LLM flexibility

### 2. In-Memory Store with Simplified MVCC
**EtcdStore Structure**:
```rust
struct EtcdStore {
    kvs: HashMap<Vec<u8>, KeyValue>,  // Key → KV metadata
    revision: i64,                     // Global revision counter
    cluster_id: u64,                   // Cluster identifier
    member_id: u64,                    // Member identifier
}
```

**MVCC-Like Behavior**:
- Each mutation (Put, Delete, Txn) increments global revision
- Each KeyValue tracks: create_revision, mod_revision, version, lease
- Simplified: No compaction, no snapshot, no multi-version history

### 3. gRPC Over HTTP/2
**Connection Handling**:
- TCP listener accepts connections
- Each connection spawned as tokio task
- HTTP/2 handled by hyper's `http2::Builder`
- gRPC framing: 5-byte header (compression flag + length) + protobuf payload

**RPC Routing**:
- Path-based routing: `/etcdserverpb.KV/Range`, `/etcdserverpb.KV/Put`, etc.
- Decode protobuf request → call LLM → encode protobuf response
- gRPC status codes in HTTP headers (grpc-status: 0 for success)

### 4. Action-Based LLM Control
The LLM doesn't manipulate protobuf directly. Instead, it returns semantic actions:

**Sync Actions** (network event triggered):
- `etcd_range_response` - Return key-value pairs
- `etcd_put_response` - Acknowledge Put operation
- `etcd_delete_range_response` - Acknowledge Delete operation
- `etcd_txn_response` - Return transaction result
- `etcd_compact_response` - Acknowledge compaction
- `etcd_error` - Return error with code and message

**Async Actions** (user-triggered, future):
- `etcd_list_keys` - Show all stored keys (debugging)
- `etcd_get_stats` - Display server statistics
- `etcd_set_key` - Manually set a key (admin operation)

### 5. Event Types
**Defined Events**:
- `etcd_range_request` - Client queries keys (Get operation)
- `etcd_put_request` - Client stores key-value pair
- `etcd_delete_request` - Client deletes keys
- `etcd_txn_request` - Client sends transaction

**Event Flow**:
1. gRPC request arrives (e.g., Range)
2. Decode protobuf request
3. Create Event with request parameters (key, range_end, limit)
4. Call LLM with event
5. LLM returns action (e.g., etcd_range_response)
6. Execute action → build protobuf response
7. Encode and send gRPC response

### 6. Dual Logging
- **DEBUG**: RPC summaries ("etcd Range request: key=foo")
- **TRACE**: Full protobuf message dumps (pretty-printed)
- Both go to netget.log and TUI Status panel
- Status messages use `status_tx` channel

## LLM Integration

### Event: `etcd_range_request`
Triggered when client sends Range (Get) request.

**Parameters**:
- `key` (string) - Key to query
- `range_end` (string, optional) - End of range for prefix queries
- `limit` (number, optional) - Maximum keys to return

**Available Actions**:
- `etcd_range_response` - Return key-value pairs
- `etcd_error` - Return error

### Action: `etcd_range_response`
Return key-value pairs for a Range request.

**Parameters**:
- `kvs` (array, required) - Array of key-value objects with fields:
  - `key` (string) - Key
  - `value` (string) - Value
  - `create_revision` (number) - Revision when created
  - `mod_revision` (number) - Revision when last modified
  - `version` (number) - Number of modifications
  - `lease` (number) - Lease ID (0 = no lease)
- `more` (boolean) - Whether there are more keys
- `count` (number) - Total count of matching keys

**Example**:
```json
{
  "actions": [
    {
      "type": "etcd_range_response",
      "kvs": [
        {
          "key": "foo",
          "value": "bar",
          "create_revision": 1,
          "mod_revision": 1,
          "version": 1,
          "lease": 0
        }
      ],
      "more": false,
      "count": 1
    }
  ]
}
```

### Action: `etcd_error`
Return an error response.

**Parameters**:
- `code` (string) - Error code (e.g., "KEY_NOT_FOUND", "INVALID_ARGUMENT")
- `message` (string) - Error message

**Example**:
```json
{
  "actions": [
    {
      "type": "etcd_error",
      "code": "KEY_NOT_FOUND",
      "message": "etcdserver: key not found"
    }
  ]
}
```

## Connection Management

### Connection Lifecycle
1. **TCP Accept**: Client connects to port 2379
2. **HTTP/2 Handshake**: Establish HTTP/2 connection
3. **gRPC Requests**: Client sends RPC calls (Range, Put, etc.)
4. **Connection Tracking**: Each connection tracked in ServerInstance
5. **Persistent**: Connection remains open for multiple RPCs (HTTP/2 keep-alive)

### Connection Info
```rust
ProtocolConnectionInfo::Etcd {
    cluster_name: String,       // Cluster identifier
    last_operation: String,     // Last RPC (e.g., "Range")
    operations_count: u64,      // Total RPC calls
}
```

## Known Limitations

### Phase 1 (Current) - KV Service Only
**Implemented**:
- ✅ Range (Get) - Query keys
- ✅ Put - Store key-value pairs
- ✅ DeleteRange - Delete keys
- ✅ Txn - Transactions
- ✅ Compact - Compaction (stub)

**Not Implemented**:
- ❌ Watch service - Real-time change notifications (requires streaming)
- ❌ Lease service - TTL-based expiration
- ❌ Auth service - Authentication and authorization
- ❌ Cluster service - Member management
- ❌ Maintenance service - System operations

### Simplified MVCC
- **No multi-version storage**: Only current version of each key
- **No compaction**: Revision counter increments but old data not removed
- **No snapshot**: No point-in-time queries
- **Simplified transactions**: Basic compare-and-swap only

### No Persistence
- **In-memory only**: All data lost on restart
- **No WAL**: No write-ahead log
- **No Raft**: No distributed consensus
- **Single node**: No replication

### Streaming RPCs Not Supported
- Watch requires bidirectional streaming (not yet implemented in NetGet gRPC)
- LeaseKeepAlive requires streaming (deferred to Phase 2)

## Example Prompts

### Simple KV Store
```
listen on port 2379 via etcd
Store configuration under /config/ prefix
When clients get /config/database, return "localhost:5432"
When clients get /config/timeout, return "30"
For unknown keys, return KEY_NOT_FOUND error
```

### Service Discovery
```
listen on port 2379 via etcd
Services register under /services/{service_name}/{instance_id}
When clients query /services/api, return all instances:
  - /services/api/instance-1 = "http://10.0.1.5:8080"
  - /services/api/instance-2 = "http://10.0.1.6:8080"
Use range queries to return multiple instances at once
```

### Distributed Lock (Transactions)
```
listen on port 2379 via etcd
Implement distributed locking with transactions
When client checks if /locks/database doesn't exist (create_revision = 0):
  - If true: create it with their client ID
  - If false: return error "lock already held"
```

### Configuration Store with Revisions
```
listen on port 2379 via etcd
Store application config under /app/config/
Track revision numbers for all changes
  - First put: create_revision=1, mod_revision=1, version=1
  - Update: create_revision=1, mod_revision=2, version=2
Log all changes with revision numbers
```

## Performance Characteristics

### Latency
- **With Scripting**: N/A (scripting not yet implemented for etcd)
- **Without Scripting**: 2-5 seconds per RPC (one LLM call per request)
- Protobuf encoding/decoding: ~10-100 microseconds
- gRPC framing overhead: minimal

### Throughput
- **Limited by LLM**: ~0.2-0.5 requests per second
- Concurrent requests processed in parallel (separate tokio tasks)
- Ollama lock serializes LLM API calls

### Scripting Compatibility
- **Future**: etcd could benefit from scripting for repetitive KV operations
- **Challenge**: Complex protobuf schema may be difficult to script
- **Alternative**: Cache common responses in LLM-generated script

## References
- [etcd v3 API Documentation](https://etcd.io/docs/v3.5/learning/api/)
- [etcd gRPC Protocol](https://github.com/etcd-io/etcd/tree/main/api/etcdserverpb)
- [etcd Client Protocol](https://etcd.io/docs/v3.5/learning/api_guarantees/)
- [tonic gRPC Framework](https://github.com/hyperium/tonic)
- [Official etcd Protobuf Definitions](https://github.com/etcd-io/etcd/tree/main/api)

## Key Design Principles

1. **Client Compatibility** - Use official protobuf schemas for real etcd client support
2. **LLM Control** - LLM decides all KV operations via semantic actions
3. **Simplified MVCC** - Track revisions without full multi-version storage
4. **Phase 1: KV Only** - Focus on core KV service, defer Watch/Lease/Auth
5. **In-Memory** - No persistence, suitable for testing/mocking use cases
