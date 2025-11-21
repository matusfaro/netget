# MSSQL Client Protocol Implementation

## Overview

MSSQL client for connecting to Microsoft SQL Server databases using the `tiberius` TDS client library. The client provides full LLM control over SQL query execution and handles result processing.

**Default Port**: 1433
**Protocol**: TDS (Tabular Data Stream) 7.2+
**Stack Representation**: `ETH>IP>TCP>TDS>MSSQL`

## Library Choices

**tiberius v0.12**:
- Pure Rust TDS 7.2+ client implementation
- Async/await support with Tokio runtime
- Maintained by Prisma (https://github.com/prisma/tiberius)
- Supports SQL Server 2008+ and Azure SQL
- Features:
  - Connection pooling (not used in this implementation)
  - Named instances
  - Windows Authentication (NTLM)
  - TLS encryption
  - Multiple result sets
  - Prepared statements

**tokio-util**:
- Used for `compat` layer (`TokioAsyncWriteCompatExt`) to bridge Tokio's `AsyncRead`/`AsyncWrite` with tiberius expectations

## Architecture Decisions

### Connection Model

**Query-driven (not event-driven)**:
- Unlike Redis client (which has a continuous read loop), MSSQL client is query-driven
- Queries are executed on-demand when LLM requests them via `execute_query` action
- Each query triggers a result event, which the LLM can respond to with more queries
- Connection remains open but idle between LLM interactions

**State Management**:
- `TiberiusClient` wrapped in `Arc<Mutex<>>` for thread-safe access
- Mutex held only during query execution, released before LLM call
- No connection pooling (single connection per client instance)

### Authentication

**Configurable via connection string**:
- Format: `host:port;database=db;user=user`
- Parameters parsed from semicolon-separated key=value pairs
- Default: `AuthMethod::None` (for testing with no-auth servers)
- Supports SQL Server authentication (username/password)
- Supports Windows Authentication (NTLM) - requires additional config

**TLS/Encryption**:
- `trust_cert()` called for testing (accepts self-signed certificates)
- Production use should validate certificates properly

### Query Execution Flow

1. LLM returns `execute_query` action with SQL text
2. Lock `TiberiusClient` mutex
3. Call `client.query(sql, &[])` to execute query
4. Release mutex
5. Collect query results:
   - Extract column metadata (name, type)
   - Collect all rows as JSON arrays
   - Get `rows_affected` for non-SELECT queries
6. Create `MSSQL_CLIENT_QUERY_RESULT_EVENT` with columns/rows/rows_affected
7. Call LLM with result event
8. Execute any follow-up actions from LLM response

### Result Processing

**Type Conversion**:
- Row values extracted using `row.try_get::<T, _>(index)`
- Supported types: `&str`, `i32`, `i64`, `bool`, `f32`, `f64`
- NULL or unsupported types → `json!(null)`
- Column types reported as debug strings (e.g., "Int4", "NVarchar")

**Stream Processing**:
- tiberius uses `QueryStream` which yields `QueryItem::Row` or `QueryItem::Metadata`
- Metadata processed once at start to get column definitions
- Rows collected into Vec for LLM consumption
- `rows_affected()` called at end for DDL/DML queries

## LLM Integration

### Events

**Connection Event**:
- `MSSQL_CLIENT_CONNECTED_EVENT`: Fired after successful connection
  - Data: `{ "remote_addr": "127.0.0.1:1433" }`

**Query Result Event**:
- `MSSQL_CLIENT_QUERY_RESULT_EVENT`: Fired after each query execution
  - Data:
    ```json
    {
      "columns": [{"name": "id", "type": "Int4"}, {"name": "name", "type": "NVarchar"}],
      "rows": [[1, "Alice"], [2, "Bob"]],
      "rows_affected": 2
    }
    ```

**Error Event**:
- `MSSQL_CLIENT_ERROR_EVENT`: Fired on query errors
  - Data: `{ "error_number": 50000, "message": "..." }`

### Actions

**Async Actions** (user-triggered):
- `execute_query`: Execute SQL query
  - Parameters: `query` (string)
  - Example: `{"type": "execute_query", "query": "SELECT @@VERSION"}`

- `disconnect`: Close connection
  - No parameters

**Sync Actions** (response to events):
- `execute_query`: Execute follow-up query based on results
- `wait_for_more`: Do nothing (wait for next LLM interaction)

### Example LLM Interaction

**Initial connection**:
```
Event: mssql_connected
LLM: {"actions": [{"type": "execute_query", "query": "SELECT @@VERSION"}]}
```

**After query result**:
```
Event: mssql_query_result
  columns: [{"name": "version", "type": "NVarchar"}]
  rows: [["Microsoft SQL Server 2022..."]]
LLM: {"actions": [{"type": "execute_query", "query": "SELECT * FROM sys.tables"}]}
```

## Connection String Format

**Basic**:
```
localhost:1433
```

**With database**:
```
localhost:1433;database=mydb
```

**With authentication**:
```
localhost:1433;database=mydb;user=sa
```

**Parameters**:
- `host:port` - Required (host and port separated by colon)
- `database=name` - Optional database name
- `user=username` - Optional SQL Server username
- Other tiberius `Config` options not currently parsed (can be added)

## Limitations

### Authentication

- **Password not supported in connection string** - tiberius `AuthMethod::sql_server` requires password but we don't parse it
- Workaround: Use `AuthMethod::None` or extend parsing
- Windows Authentication requires additional NTLM setup

### Protocol Features

- **No prepared statements** - only simple queries via `client.query()`
- **No connection pooling** - single connection per client
- **No transaction management** - BEGIN/COMMIT/ROLLBACK treated as regular queries (no state tracking)
- **No multiple active result sets (MARS)** - tiberius supports it but we don't expose control

### Type Support

- **Limited type extraction** - only basic types (`&str`, `i32`, `i64`, `bool`, `f32`, `f64`)
- **No binary data** - `VARBINARY`, `IMAGE` not extracted (would need `Vec<u8>` handling)
- **No date/time types** - `DateTime`, `Date`, `Time` not extracted (need `chrono` types)
- **No decimal types** - `DECIMAL`, `NUMERIC` not extracted (need `rust_decimal`)
- **No XML/JSON** - treated as strings if extractable

### Error Handling

- Query errors trigger error event but don't include detailed error metadata
- Error number hardcoded to 50000 (should extract from tiberius error)
- No severity/state information

## Known Issues

1. **Connection string parsing**: Limited to `host:port;database=db;user=user` - no password, no advanced options
2. **Type reporting**: Column types shown as debug strings (`"Int4"`) instead of SQL names (`"INT"`)
3. **NULL handling**: All NULLs converted to `json!(null)`, no type distinction
4. **Memory usage**: All rows loaded into memory before sending to LLM (no streaming)
5. **Concurrent queries**: Mutex prevents concurrent queries on same connection (expected for TDS protocol)

## Future Improvements

1. **Password support**: Parse password from connection string or use separate config
2. **Advanced types**: Support `DateTime`, `Decimal`, `Guid`, binary data
3. **Prepared statements**: Expose `client.execute()` for parameterized queries
4. **Connection pooling**: Use `deadpool-tiberius` or similar
5. **Transaction tracking**: Track BEGIN/COMMIT/ROLLBACK state per connection
6. **Streaming results**: Yield rows to LLM incrementally for large result sets
7. **Error detail**: Extract error number, severity, state from tiberius errors
8. **Named instances**: Support SQL Server named instances (`host\\instance` syntax)

## Example Queries

**Version check**:
```sql
SELECT @@VERSION
```

**List databases**:
```sql
SELECT name FROM sys.databases
```

**Create table**:
```sql
CREATE TABLE users (id INT PRIMARY KEY, name NVARCHAR(100))
```

**Insert data**:
```sql
INSERT INTO users VALUES (1, 'Alice'), (2, 'Bob')
```

**Select data**:
```sql
SELECT * FROM users WHERE id > 0
```

## Dependencies

- `tiberius = "0.12"` - TDS client library
- `tokio-util = { version = "0.7", features = ["compat"] }` - Compat layer for async I/O
- Requires Tokio runtime (already part of NetGet)

## References

- [tiberius crate](https://docs.rs/tiberius/)
- [tiberius GitHub](https://github.com/prisma/tiberius)
- [MS-TDS] Tabular Data Stream Protocol - Microsoft specification
- [SQL Server TDS versions](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-tds/): 7.0 (SQL 7.0), 7.1 (SQL 2000), 7.2 (SQL 2005), 7.3 (SQL 2008), 7.4 (SQL 2012+)
