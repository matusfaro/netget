# PostgreSQL Protocol Implementation

## Overview

PostgreSQL server implementing PostgreSQL wire protocol using the `pgwire` crate. The server handles both simple and extended query protocols with full LLM control over query responses, supporting multiple data types and error handling.

**Port**: 5432 (default PostgreSQL port)
**Protocol Version**: PostgreSQL 14+ compatible
**Stack Representation**: `ETH>IP>TCP>POSTGRESQL`

## Library Choices

**pgwire** (v0.26):
- Chosen for comprehensive PostgreSQL wire protocol support
- Provides `SimpleQueryHandler` and `ExtendedQueryHandler` traits
- Handles connection startup, authentication, and query parsing
- Supports both simple and extended (prepared statement) query protocols
- Stream-based response system for memory efficiency

**NoopStartupHandler**:
- No authentication implemented (all connections accepted)
- Username/database parameters ignored
- Production use would require custom authentication handler

**Manual Response Construction**:
- LLM controls all query responses through action system
- Result sets built from LLM-provided columns and row data
- Type mapping from JSON to PostgreSQL types (INT2/4/8, TEXT, VARCHAR, etc.)

## Architecture Decisions

### Dual Query Protocol Support

**Simple Query Protocol**:
- Used by most clients for ad-hoc queries
- Implemented via `SimpleQueryHandler::do_query()`
- Returns `Vec<Response>` for multiple statement results
- Direct query string from client to LLM

**Extended Query Protocol**:
- Used for prepared statements and parameterized queries
- Implemented via `ExtendedQueryHandler::do_query()`
- Requires Parse/Bind/Execute flow
- Single `Response` per execution
- **Known Issue**: LLM timeout in extended query handler (see Limitations)

### Handler Factory Pattern
- `PostgresqlHandlerFactory` creates fresh handlers per connection
- Each handler owns: connection_id, llm_client, app_state, protocol
- Factory provides both `SimpleQueryHandler` and `ExtendedQueryHandler`
- Handlers are stateless (no prepared statement caching)

### Query Execution Flow
1. Client sends query (simple or extended protocol)
2. Handler extracts SQL text
3. Create `POSTGRESQL_QUERY_EVENT` with query string
4. Call LLM via `call_llm()` with event and protocol
5. Process action results:
   - `postgresql_query_response`: Build result set with columns/rows
   - `postgresql_ok`: Return execution tag (e.g., "SELECT 0", "INSERT 1")
   - `postgresql_error`: Return error with severity/code/message
6. If no action found:
   - SELECT queries → empty result set
   - Other queries → OK tag

### Type System
- LLM specifies column types as strings: "int4", "text", "varchar", etc.
- `json_value_to_string()` converts JSON to PostgreSQL text format
- Boolean: `t`/`f` (not `true`/`false`)
- NULL: empty string (pgwire convention)
- All values serialized as text (pgwire FieldFormat::Text)

### Stream-Based Results
- Rows converted to stream via `futures::stream::iter()`
- Memory-efficient for large result sets
- Field encoders handle serialization per row
- Arc-wrapped field metadata for sharing

## LLM Integration

### Action-Based Responses

**Sync Actions** (network event context required):
- `postgresql_query_response`: Return result set with columns and rows
- `postgresql_ok`: Return OK response with custom tag
- `postgresql_error`: Return error with severity/code/message

**Event Types**:
- `POSTGRESQL_QUERY_EVENT`: Fired for every query operation
  - Data: `{ "query": "SELECT * FROM users" }`
  - Used for both simple and extended queries

### Example LLM Prompts

**Basic SELECT query**:
```
For SELECT 1 query, use postgresql_query_response with:
columns=[{name:'?column?',type:'int4'}]
rows=[[1]]
```

**Multi-row result**:
```
For SELECT * FROM users, use postgresql_query_response with:
columns=[{name:'id',type:'int4'},{name:'name',type:'text'}]
rows=[[1,'Alice'],[2,'Bob']]
```

**DDL/DML operations**:
```
For CREATE TABLE queries, use postgresql_ok with tag='CREATE TABLE'
For INSERT queries, use postgresql_ok with tag='INSERT 0 1'
```

**Error responses**:
```
For invalid_table queries, use postgresql_error with:
severity='ERROR' code='42P01' message='relation "invalid_table" does not exist'
```

## Connection Management

### Connection Lifecycle
1. Server accepts TCP connection on port 5432
2. Create `PostgresqlHandlerFactory` with unique `ConnectionId`
3. Add connection to `ServerInstance` with `ProtocolConnectionInfo::Postgresql`
4. Spawn task running `process_socket(stream, None, handler_factory)`
5. pgwire handles protocol framing and dispatches to handlers
6. Connection marked closed in ServerInstance when stream ends

### State Tracking
- Connection state stored in `ServerInstance.connections` HashMap
- Tracks: remote_addr, local_addr, bytes_sent/received, packets_sent/received
- Status: Active or Closed
- Last activity timestamp updated per query

### Concurrency
- Multiple connections handled concurrently
- Each connection has independent handler instances
- No shared state between connections
- LLM calls queued per connection via `ProtocolConnectionInfo`

## Limitations

### Extended Query Protocol Timeout
**CRITICAL ISSUE**: LLM calls in `ExtendedQueryHandler::do_query()` do not complete within pgwire's internal timeout.

**Symptoms**:
- Simple queries work correctly
- Extended queries (prepared statements, parameterized queries) timeout
- Client sees "connection closed" or timeout errors

**Root Cause**: Unknown - possible pgwire internal timeout or protocol flow issue

**Workaround**: Tests explicitly disable or increase timeouts
- `connect_timeout=60` in connection string
- `statement_timeout=0` to disable server-side timeout
- Still unreliable in practice

**TODO**: Investigate pgwire `ExtendedQueryHandler` timeout behavior and fix LLM call completion

### Authentication
- **No authentication implemented** - all connections accepted
- Uses `NoopStartupHandler` (pgwire limitation)
- Username/database parameters ignored
- Production use would require custom startup handler

### Protocol Features
- **No SSL/TLS support** - plain TCP only
- **No binary format** - all values sent as text (FieldFormat::Text)
- **Limited type support** - complex types (arrays, JSON, etc.) not fully supported
- **No transactions** - BEGIN/COMMIT/ROLLBACK handled as regular queries
- **No cursors/portals** - extended query protocol not fully implemented
- **No prepared statement caching** - each execution re-calls LLM

### Performance
- Each query triggers LLM call (unless scripting is used)
- No query planning or optimization
- No connection pooling
- Stream processing adds overhead for small result sets

## Known Issues

1. **Extended query timeout**: LLM calls do not complete in extended query protocol (see above)
2. **Empty result fallback**: If LLM returns no action, SELECT queries get empty result set (may confuse some clients)
3. **Type precision**: All numeric values sent as text, client must parse
4. **Boolean format**: Uses `t`/`f` (PostgreSQL format), not `true`/`false`
5. **Version queries**: `SELECT version()` requires explicit prompting

## Example Responses

### Successful Query
```json
{
  "actions": [
    {
      "type": "postgresql_query_response",
      "columns": [
        {"name": "id", "type": "int4"},
        {"name": "email", "type": "text"}
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
      "type": "postgresql_ok",
      "tag": "CREATE TABLE"
    }
  ]
}
```

### Error Response
```json
{
  "actions": [
    {
      "type": "postgresql_error",
      "severity": "ERROR",
      "code": "42P01",
      "message": "relation \"invalid_table\" does not exist"
    }
  ]
}
```

## References

- [PostgreSQL Wire Protocol](https://www.postgresql.org/docs/current/protocol.html)
- [pgwire crate](https://docs.rs/pgwire/)
- [tokio-postgres client](https://docs.rs/tokio-postgres/) - used in tests
- [PostgreSQL Error Codes](https://www.postgresql.org/docs/current/errcodes-appendix.html)
