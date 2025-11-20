# MSSQL Server Protocol Implementation

## Overview

MSSQL server implementing Microsoft SQL Server TDS (Tabular Data Stream) protocol version 7.x. The server handles SQL queries with full LLM control over query responses, implementing a simplified TDS protocol manually due to the absence of Rust TDS server libraries.

**Port**: 1433 (default MSSQL port)
**Protocol Version**: TDS 7.4 compatible
**Stack Representation**: `ETH>IP>TCP>TDS>MSSQL`

## Library Choices

**Manual TDS Implementation**:
- **Rationale**: No Rust TDS server libraries exist (only client libraries like `tiberius`)
- **Approach**: Manual protocol parsing and packet construction
- **Scope**: Simplified subset of TDS protocol sufficient for basic query execution
- **Reference**: Microsoft [MS-TDS] Tabular Data Stream Protocol specification

**No Database Engine**:
- LLM controls all query responses through action system
- No actual data storage - responses generated on demand
- Similar pattern to MySQL/PostgreSQL implementations in NetGet

## Architecture Decisions

### TDS Protocol Implementation

**Packet Structure**:
- 8-byte header: type (1), status (1), length (2), SPID (2), packet_id (1), window (1)
- Variable-length data payload
- Big-endian length in header, little-endian data in payload

**Supported Packet Types**:
- `0x12`: Pre-Login - Version negotiation
- `0x10`: TDS7 Login - Authentication (no-op, accepts all)
- `0x01`: SQL Batch - Query execution (LLM-controlled)
- `0x04`: Tabular Result - Response packet type
- `0x03`: RPC Request - Not implemented (returns error)
- `0x0E`: Bulk Load - Not implemented (returns error)
- `0x07`: Attention - Connection cancellation

### Connection Handler Design

- Each connection spawns independent `MssqlHandler`
- Handler owns connection-specific protocol instance
- No connection pooling or multiplexing
- Single-threaded query processing per connection

### Pre-Login/Login Flow

1. **Pre-Login**: Client sends version and encryption preferences
   - Server responds with SQL Server 16.0.0.0 (SQL Server 2022)
   - Encryption: NOT_SUP (no TLS support)
   - ThreadID: 0

2. **Login**: Client sends authentication credentials
   - Server accepts all logins (no authentication)
   - Sends ENVCHANGE (database=master), INFO (success message), DONE tokens
   - No validation of username/password/database

### Query Execution Flow

1. Client sends SQL Batch packet (0x01)
2. Parse header (22 bytes) and extract UTF-16LE SQL text
3. Create `MSSQL_QUERY_EVENT` with query string
4. Call LLM via `call_llm()` with event and protocol
5. Process action results:
   - `mssql_query_response`: Send COLMETADATA + ROW + DONE tokens
   - `mssql_ok`: Send DONE token with rows_affected
   - `mssql_error`: Send ERROR + DONE tokens
6. If no action found, send empty DONE token

### Response Token Structure

**COLMETADATA (0x81)**:
- Column count (2 bytes)
- For each column:
  - UserType (4 bytes), Flags (2 bytes), Type (1 byte)
  - MaxLength (2 bytes), Collation (5 bytes)
  - Column name (1 byte length + UTF-16LE string)
- Simplified: All types sent as NVARCHAR (0xE7) with max length 0xFFFF

**ROW (0xD1)**:
- For each row:
  - ROW token (1 byte)
  - For each column value:
    - Length (2 bytes for NVARCHAR)
    - UTF-16LE encoded value

**DONE (0xFD)**:
- Status (2 bytes): 0x0000 = final
- CurCmd (2 bytes): 0x00C1 = SELECT
- DoneRowCount (8 bytes): number of rows

**ERROR (0xAA)**:
- Token length (2 bytes)
- Error number (4 bytes)
- State (1 byte), Severity (1 byte)
- Message length (2 bytes) + UTF-16LE message
- Server name (1 byte length), Procedure name (1 byte length)
- Line number (4 bytes)

## LLM Integration

### Action-Based Responses

**Sync Actions** (network event context required):

- `mssql_query_response`: Return result set with columns and rows
- `mssql_ok`: Return completion status (for DDL/DML)
- `mssql_error`: Return error response

**Event Types**:

- `MSSQL_QUERY_EVENT`: Fired for every SQL Batch operation
  - Data: `{ "query": "SELECT * FROM users" }`

### Example LLM Prompts

**Basic SELECT query**:
```
For SELECT 1 query, use mssql_query_response with:
columns=[{name:'result',type:'INT'}]
rows=[[1]]
```

**Multi-row result**:
```
For SELECT * FROM users, use mssql_query_response with:
columns=[{name:'id',type:'INT'},{name:'name',type:'NVARCHAR'}]
rows=[[1,'Alice'],[2,'Bob']]
```

**DDL/DML operations**:
```
For CREATE/INSERT/UPDATE queries, use mssql_ok with rows_affected=1
```

## Connection Management

### Connection Lifecycle

1. Server accepts TCP connection on port 1433
2. Create `MssqlHandler` with unique `ConnectionId`
3. Add connection to `ServerInstance` with `ProtocolConnectionInfo::empty()`
4. Spawn task running `handler.handle_connection(stream)`
5. Handler processes TDS packets until disconnect/error
6. Connection marked closed in ServerInstance

### State Tracking

- Connection state stored in `ServerInstance.connections` HashMap
- Tracks: remote_addr, local_addr, bytes_sent/received, packets_sent/received
- Status: Active or Closed
- Last activity timestamp per packet

### Concurrency

- Multiple connections handled concurrently
- Each connection has independent LLM call queue
- No shared state between connections
- No prepared statement caching (each connection is stateless)

## Limitations

### Authentication

- **No authentication implemented** - all connections accepted
- Username/password/database in login packet ignored
- Production use would require TDS authentication implementation
- No support for Windows Authentication or Azure AD

### Protocol Features

- **No SSL/TLS support** - plain TCP only
- **No prepared statements** - SQL Batch only (no RPC)
- **No transactions** - BEGIN/COMMIT/ROLLBACK handled as regular queries
- **No stored procedures/functions**
- **No bulk operations** - Bulk Load packet returns error
- **No cursors or scrollable result sets**
- **No multiple active result sets (MARS)**

### Type System

- **Limited type support** - all column types sent as NVARCHAR (0xE7)
- Numeric types (INT, BIGINT, FLOAT) mapped but serialized as strings
- No binary data support (VARBINARY, IMAGE)
- No XML, JSON, or spatial types
- Simplified column metadata (no precision/scale for decimals)

### Performance

- Each query triggers LLM call (unless scripting is used)
- No query caching or optimization
- Synchronous query processing per connection
- Manual UTF-16LE encoding/decoding overhead

## Known Issues

1. **Type precision**: All values sent as NVARCHAR strings, may lose type information for clients
2. **Error handling**: ERROR token structure may not match all SQL Server clients' expectations
3. **Collation**: Hardcoded collation (0x00000000) may cause issues with non-ASCII data
4. **System queries**: `SELECT @@VERSION` and similar require explicit LLM prompting

## Type Mapping

### LLM Type Names → TDS Type Codes

- `INT`, `INTEGER` → 0x38 (INTN)
- `BIGINT` → 0x7F (INT8)
- `SMALLINT` → 0x34 (INT2)
- `TINYINT` → 0x30 (INT1)
- `BIT` → 0x32 (BIT)
- `FLOAT`, `REAL` → 0x3B (FLT4/FLT8)
- `NVARCHAR`, `NCHAR`, `NTEXT` → 0xE7 (NVARCHAR)
- `VARCHAR`, `CHAR`, `TEXT` → 0xA7 (VARCHAR)
- **Default**: 0xE7 (NVARCHAR)

**Note**: Despite type code mapping, all values are currently serialized as NVARCHAR (UTF-16LE strings).

## Example Responses

### Successful Query

```json
{
  "actions": [
    {
      "type": "mssql_query_response",
      "columns": [
        {"name": "id", "type": "INT"},
        {"name": "email", "type": "NVARCHAR"}
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
      "type": "mssql_ok",
      "rows_affected": 0
    }
  ]
}
```

### Error Response

```json
{
  "actions": [
    {
      "type": "mssql_error",
      "error_number": 208,
      "message": "Invalid object name 'table_name'",
      "severity": 16
    }
  ]
}
```

## References

- [MS-TDS] Tabular Data Stream Protocol - Microsoft documentation
- TDS Protocol versions: 7.0 (SQL Server 7.0), 7.1 (SQL Server 2000), 7.2 (SQL Server 2005), 7.3 (SQL Server 2008), 7.4 (SQL Server 2012+)
- [tiberius crate](https://docs.rs/tiberius/) - TDS client implementation (used for E2E testing)

## Future Improvements

1. **Type system**: Proper binary encoding for INT/BIGINT/FLOAT (not as strings)
2. **Authentication**: Implement SQL Server authentication (NTLM or basic)
3. **Prepared statements**: Support RPC packet type for parameterized queries
4. **Transactions**: Track BEGIN/COMMIT/ROLLBACK state per connection
5. **TLS**: Add encryption support for pre-login negotiation
6. **Error codes**: Comprehensive mapping of MSSQL error numbers
7. **System tables**: Auto-respond to `SELECT * FROM sys.tables` and similar without LLM
