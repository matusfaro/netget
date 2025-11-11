# MySQL Protocol Implementation

## Overview

MySQL server implementing MySQL 5.7+ wire protocol using the `opensrv-mysql` crate. The server handles prepared
statements, query execution, and DDL/DML operations with full LLM control over query responses.

**Port**: 3306 (default MySQL port)
**Protocol Version**: MySQL 5.7+ compatible
**Stack Representation**: `ETH>IP>TCP>MYSQL`

## Library Choices

**opensrv-mysql** (v0.8):

- Chosen for comprehensive MySQL wire protocol support
- Provides `AsyncMysqlShim` trait for async server implementation
- Handles connection handshake, authentication, and command parsing
- Supports both simple queries and prepared statements (PREPARE/EXECUTE/CLOSE)
- Limitation: Does not support authentication (NoOp authentication used)

**Manual Response Construction**:

- LLM controls all query responses through action system
- Result sets built from LLM-provided columns and row data
- Type mapping from JSON to MySQL column types (INT, VARCHAR, BLOB, etc.)

## Architecture Decisions

### Connection Handler Design

- Each connection spawns a `MysqlHandler` implementing `AsyncMysqlShim`
- Handler owns connection-specific state: prepared statements, protocol instance
- TcpStream split with `tokio::io::split()` for async read/write
- `opensrv-mysql` manages protocol framing and command parsing

### Prepared Statement Management

- Statements stored in `Arc<Mutex<HashMap<u32, String>>>` per connection
- Statement IDs auto-incremented starting from 1
- PREPARE stores query string with generated ID
- EXECUTE retrieves stored query and treats as regular query
- CLOSE removes statement from map

### Query Execution Flow

1. Client sends QUERY or EXECUTE command
2. Handler extracts SQL text
3. Create `MYSQL_QUERY_EVENT` with query string
4. Call LLM via `call_llm()` with event and protocol
5. Process action results:
    - `mysql_query_response`: Send result set with columns/rows
    - `mysql_ok`: Send OK response with affected_rows/last_insert_id
    - `mysql_error`: Send OK response (opensrv limitation: errors sent as OK)
6. If no action found, send empty OK response

### Type System

- LLM specifies column types as strings: "INT", "VARCHAR", "BLOB", etc.
- `json_to_mysql_string()` converts JSON values to MySQL wire format
- All values serialized as strings (simplified for LLM interaction)
- NULL represented as "NULL" string

## LLM Integration

### Action-Based Responses

**Sync Actions** (network event context required):

- `mysql_query_response`: Return result set with columns and rows
- `mysql_ok`: Return OK response (for DDL/DML)
- `mysql_error`: Return error response

**Event Types**:

- `MYSQL_QUERY_EVENT`: Fired for every query/execute operation
    - Data: `{ "query": "SELECT * FROM users" }`

### Example LLM Prompts

**Basic SELECT query**:

```
For SELECT 1 query, use mysql_query_response with:
columns=[{name:'result',type:'INT'}]
rows=[[1]]
```

**Multi-row result**:

```
For SELECT * FROM users, use mysql_query_response with:
columns=[{name:'id',type:'INT'},{name:'name',type:'VARCHAR'}]
rows=[[1,'Alice'],[2,'Bob']]
```

**DDL/DML operations**:

```
For CREATE/INSERT/UPDATE queries, use mysql_ok with affected_rows=1
```

## Connection Management

### Connection Lifecycle

1. Server accepts TCP connection on port 3306
2. Create `MysqlHandler` with unique `ConnectionId`
3. Add connection to `ServerInstance` with `ProtocolConnectionInfo::Mysql`
4. Spawn task running `AsyncMysqlIntermediary::run_on(handler, reader, writer)`
5. Handler processes commands until disconnect
6. Connection marked closed in ServerInstance

### State Tracking

- Connection state stored in `ServerInstance.connections` HashMap
- Tracks: remote_addr, local_addr, bytes_sent/received, packets_sent/received
- Status: Active or Closed
- Last activity timestamp updated per query

### Concurrency

- Multiple connections handled concurrently
- Each connection has independent LLM call queue (via `ProtocolConnectionInfo`)
- Prepared statements isolated per connection
- No shared state between connections

## Limitations

### Authentication

- **No authentication implemented** - all connections accepted
- Uses `NoopStartupHandler` (opensrv-mysql limitation)
- Username/password ignored
- Production use would require custom auth handler

### Error Handling

- opensrv uses `completed()` for both success and error responses
- Error responses sent as OK (no way to send actual ERROR packet)
- LLM-specified error codes/messages logged but not sent to client

### Protocol Features

- **No SSL/TLS support** - plain TCP only
- **No binary protocol** (prepared statements use text protocol)
- **Limited type support** - all values serialized as strings
- **No transactions** - BEGIN/COMMIT/ROLLBACK handled as regular queries
- **No stored procedures/functions**

### Performance

- Each query triggers LLM call (unless scripting is used)
- No query caching or optimization
- No connection pooling
- Synchronous query processing per connection

## Known Issues

1. **Error responses**: Cannot send proper MySQL ERROR packets due to opensrv API limitations
2. **Type precision**: All numeric values sent as strings, may lose precision
3. **Binary data**: BLOB data not properly handled (treated as UTF-8 strings)
4. **Variable queries**: `SELECT @@version` and similar system variable queries require explicit prompting

## Example Responses

### Successful Query

```json
{
  "actions": [
    {
      "type": "mysql_query_response",
      "columns": [
        {"name": "id", "type": "INT"},
        {"name": "email", "type": "VARCHAR"}
      ],
      "rows": [
        [1, "alice@example.com"],
        [2, "bob@example.com"]
      ]
    }
  ]
}
```

### DDL Operation

```json
{
  "actions": [
    {
      "type": "mysql_ok",
      "affected_rows": 0,
      "last_insert_id": 0
    }
  ]
}
```

### Error Response

```json
{
  "actions": [
    {
      "type": "mysql_error",
      "error_code": 1064,
      "message": "You have an error in your SQL syntax"
    }
  ]
}
```

## References

- [MySQL Wire Protocol](https://dev.mysql.com/doc/dev/mysql-server/latest/page_protocol_connection_phase.html)
- [opensrv-mysql crate](https://docs.rs/opensrv-mysql/)
- [mysql_async client](https://docs.rs/mysql-async/) - used in tests
