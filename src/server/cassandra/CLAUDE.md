# Cassandra/CQL Protocol Implementation

## Overview

Cassandra/CQL server implementing the native Cassandra binary protocol (CQL v3) using the `cassandra-protocol` crate for frame parsing and manual response construction. Supports STARTUP, OPTIONS, QUERY, PREPARE, EXECUTE, and AUTH_RESPONSE operations with full LLM control over query responses.

**Port**: 9042 (default Cassandra port)
**Protocol Version**: Protocol v4 (Cassandra 3.x+)
**Stack Representation**: `ETH>IP>TCP>CASSANDRA`

## Library Choices

**cassandra-protocol** (v3.0):
- Chosen for comprehensive CQL binary protocol support
- Provides `Envelope` for frame parsing and serialization
- Handles frame headers, compression, opcodes
- Supports Protocol v4 features (prepared statements, paging, etc.)
- Manual response body construction required

**Manual Response Construction**:
- LLM controls all query/prepare/execute responses
- Response bodies manually built following CQL wire format
- Complex binary encoding for: metadata, column specs, row data
- Type mapping from JSON to CQL types (int, varchar, boolean, etc.)

## Architecture Decisions

### Frame-Based Protocol
- Each CQL frame has: version, direction, flags, stream_id, opcode, length, body
- Parse incoming frames with `Envelope::from_buffer()`
- Handle opcodes: STARTUP, OPTIONS, QUERY, PREPARE, EXECUTE, AUTH_RESPONSE
- Build response frames with appropriate opcode and body
- Encode frames with `Envelope::encode_with(Compression::None)`
- Write encoded bytes to TcpStream

### Connection State Machine
- Each connection maintains `CassandraConnectionState`:
  - `ready`: Connection ready after STARTUP
  - `protocol_version`: Negotiated protocol version (v4)
  - `prepared_statements`: HashMap of statement_id → (query, param_count)
  - `authenticated`: Auth status (Phase 3)
  - `username`: Authenticated user (Phase 3)

### Query Execution Flow
1. Client sends QUERY frame with CQL string
2. Parse query from frame body (long string format)
3. Create `CASSANDRA_QUERY_EVENT` with query string
4. Call LLM via `call_llm()` with event and protocol
5. Process action results:
   - `cassandra_result_rows`: Build RESULT frame with rows
   - `cassandra_error`: Send ERROR frame
   - `close_connection`: Close connection
6. If no action, send empty result set

### Prepared Statement Flow (Phase 2)
1. Client sends PREPARE frame with CQL query
2. Generate statement ID from query hash
3. Count parameters (count `?` occurrences)
4. Store in `prepared_statements` HashMap
5. Create `CASSANDRA_PREPARE_EVENT`
6. LLM returns metadata (columns, param types)
7. Send RESULT (Prepared) with statement ID and metadata
8. Client sends EXECUTE with statement ID and parameters
9. Look up query from statement ID
10. Create `CASSANDRA_EXECUTE_EVENT` with query and parameters
11. LLM generates result rows
12. Send RESULT (Rows)

### Authentication Flow (Phase 3)
1. During STARTUP, LLM can request authentication (AUTHENTICATE response)
2. Client sends AUTH_RESPONSE with SASL PLAIN credentials
3. Parse username/password from SASL format
4. Create `CASSANDRA_AUTH_EVENT`
5. LLM decides to accept/reject
6. Send AUTH_SUCCESS or ERROR
7. Mark connection as authenticated

### Response Body Formats

**READY** (after STARTUP):
- Empty body
- Signals connection ready for queries

**SUPPORTED** (after OPTIONS):
- String multimap: {option: [values]}
- Example: `{"CQL_VERSION": ["3.0.0"], "COMPRESSION": []}`

**RESULT (Rows)**:
- Kind: 0x0002
- Metadata: flags, column count, global keyspace/table
- Column specs: name, type code
- Row count: 4 bytes
- Row data: each cell as [bytes] or NULL (-1)

**RESULT (Prepared)**:
- Kind: 0x0004
- Statement ID: short bytes
- Result metadata: columns the query will return
- Parameters metadata: columns for bind variables

**ERROR**:
- Error code: 4 bytes (0x0000, 0x2200, etc.)
- Error message: string

## LLM Integration

### Action-Based Responses

**Sync Actions** (network event context required):
- `cassandra_ready`: Send READY after STARTUP
- `cassandra_supported`: Send SUPPORTED after OPTIONS
- `cassandra_result_rows`: Return result set with columns/rows
- `cassandra_prepared`: Return prepared statement metadata
- `cassandra_error`: Return error with code/message
- `cassandra_auth_success`: Authenticate user (Phase 3)

**Event Types**:
- `CASSANDRA_STARTUP_EVENT`: Connection startup
- `CASSANDRA_OPTIONS_EVENT`: Client requests supported options
- `CASSANDRA_QUERY_EVENT`: CQL query execution
- `CASSANDRA_PREPARE_EVENT`: Prepare statement
- `CASSANDRA_EXECUTE_EVENT`: Execute prepared statement
- `CASSANDRA_AUTH_EVENT`: Authentication (Phase 3)

### Example LLM Prompts

**STARTUP/OPTIONS**:
```
For STARTUP, send cassandra_ready
For OPTIONS, send cassandra_supported with options={CQL_VERSION:['3.0.0']}
```

**SELECT query**:
```
For SELECT * FROM users query, use cassandra_result_rows with:
columns=[{name:'id',type:'int'},{name:'name',type:'varchar'}]
rows=[[1,'Alice'],[2,'Bob']]
```

**Prepared statement**:
```
For PREPARE 'SELECT * FROM users WHERE id = ?', use cassandra_prepared with:
columns=[{name:'id',type:'int'},{name:'name',type:'varchar'}]
For EXECUTE with parameter '1', use cassandra_result_rows with rows=[[1,'Alice']]
```

**Error responses**:
```
For invalid query, use cassandra_error with error_code=0x2200 message='Table does not exist'
```

## Connection Management

### Connection Lifecycle
1. Server accepts TCP connection on port 9042
2. Create `CassandraConnectionState` (not ready, v4)
3. Add connection to `ServerInstance` with `ProtocolConnectionInfo::Cassandra`
4. Spawn task running `handle_connection()` loop
5. Read frames, dispatch to handlers
6. Handler processes frames and sends responses
7. Connection marked closed when stream ends

### State Tracking
- Connection state stored in `ServerInstance.connections` HashMap
- Tracks: remote_addr, local_addr, bytes_sent/received, packets_sent/received
- Protocol-specific: `ready`, `protocol_version`
- Status: Active or Closed
- Last activity timestamp

### Concurrency
- Multiple connections handled concurrently
- Each connection has independent state and prepared statements
- No shared state between connections
- LLM calls queued per connection

## Limitations

### Protocol Features (Phase 1 Implementation)
- **No authentication by default** - STARTUP → READY without auth (Phase 3 adds auth)
- **No compression** - NONE compression only
- **No paging** - full result sets only
- **No batching** - BATCH not implemented
- **No events** - server-side push events not supported
- **Limited types** - int, varchar, boolean only (more in Phase 2+)
- **No collections** - lists, sets, maps not implemented
- **No UDTs** - user-defined types not supported
- **No tracing** - query tracing not implemented

### Performance
- Each query/execute triggers LLM call
- No query caching or planning
- Full result sets in memory (no streaming)
- Binary encoding overhead

### Error Handling
- If LLM returns no action, empty result set sent
- Some error codes not fully implemented
- Protocol violations may cause connection close

## Known Issues

1. **Type system limited**: Only int, varchar, boolean supported in Phase 1
2. **No streaming**: Large result sets consume memory
3. **Prepared statement IDs**: Hash-based, may have collisions (rare)
4. **Parameter validation**: Basic validation only (count check)
5. **Keyspace/table names**: Hardcoded to "system"/"local" or "netget"/"data"

## Example Responses

### Query Response
```json
{
  "actions": [
    {
      "type": "cassandra_result_rows",
      "columns": [
        {"name": "id", "type": "int"},
        {"name": "name", "type": "varchar"}
      ],
      "rows": [
        [1, "Alice"],
        [2, "Bob"]
      ]
    }
  ]
}
```

### Prepared Statement
```json
{
  "actions": [
    {
      "type": "cassandra_prepared",
      "columns": [
        {"name": "id", "type": "int"},
        {"name": "name", "type": "varchar"}
      ]
    }
  ]
}
```

### Error Response
```json
{
  "actions": [
    {
      "type": "cassandra_error",
      "error_code": 8704,
      "message": "Table does not exist"
    }
  ]
}
```

## References

- [Cassandra Native Protocol Spec](https://github.com/apache/cassandra/blob/trunk/doc/native_protocol_v4.spec)
- [cassandra-protocol crate](https://docs.rs/cassandra-protocol/)
- [scylla client](https://docs.rs/scylla/) - used in tests
- [CQL3 Reference](https://cassandra.apache.org/doc/latest/cassandra/cql/)
