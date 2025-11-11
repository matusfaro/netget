# PostgreSQL Client Implementation

## Overview

The PostgreSQL client implementation provides LLM-controlled access to PostgreSQL databases. The LLM can execute SQL
queries, manage transactions, and interpret results.

## Implementation Details

### Library Choice

- **tokio-postgres v0.7** - Official async PostgreSQL client
- Full PostgreSQL wire protocol implementation
- TLS support (using NoTls for now)
- Native connection pooling support (not used yet)

### Architecture

```
┌──────────────────────────────────────────────┐
│  PostgresqlClient::connect_with_llm_actions  │
│  - Build connection string                   │
│  - Connect via tokio-postgres                │
│  - Spawn connection task                     │
│  - Store client in app_state                 │
└──────────────────────────────────────────────┘
         │
         ├─► Connection Task
         │   - Handle PostgreSQL protocol
         │   - Monitor for disconnection
         │
         └─► LLM Integration
             - Send connected event
             - Execute queries on demand
             - Convert results to JSON
             - Call LLM with query results
```

### Connection Parameters

The client accepts startup parameters:

- `database` - Database name (default: "postgres")
- `user` - Username (default: "postgres")
- `password` - Password (default: empty)

Connection string format:

```
host=127.0.0.1:5432 user=postgres password=secret dbname=mydb
```

### LLM Control

**Async Actions** (user-triggered):

- `execute_query` - Execute SQL query
    - Parameter: query (string)
    - Examples: "SELECT * FROM users", "INSERT INTO logs VALUES (...)"
- `begin_transaction` - Begin a transaction
- `commit_transaction` - Commit current transaction
- `rollback_transaction` - Roll back current transaction
- `disconnect` - Close connection

**Sync Actions** (in response to query results):

- `execute_query` - Execute follow-up query based on results

**Events:**

- `postgresql_connected` - Fired when connection established
    - Data includes: remote_addr, database, user
- `postgresql_query_result` - Fired when query results received
    - Data includes: query, rows (array), row_count

### Query Execution

Queries are executed via `tokio_postgres::Client::query()`:

```rust
let rows = pg_client.query("SELECT * FROM users", &[]).await?;
```

Results are converted to JSON:

```json
[
  {"id": "1", "name": "Alice", "email": "alice@example.com"},
  {"id": "2", "name": "Bob", "email": "bob@example.com"}
]
```

### Structured Actions

```json
// Query action
{
  "type": "execute_query",
  "query": "SELECT * FROM users WHERE id = 1"
}

// Transaction actions
{
  "type": "begin_transaction"
}

{
  "type": "commit_transaction"
}

// Query result event
{
  "event_type": "postgresql_query_result",
  "data": {
    "query": "SELECT * FROM users",
    "rows": [...],
    "row_count": 42
  }
}
```

### Dual Logging

```rust
info!("PostgreSQL client {} connected", client_id);           // → netget.log
status_tx.send("[CLIENT] PostgreSQL client connected");      // → TUI
```

## Limitations

- **No TLS** - Currently uses NoTls, should add rustls support
- **No Connection Pooling** - Single connection per client
- **No Prepared Statements** - All queries use simple query protocol
- **Limited Type Support** - All values converted to strings
- **No LISTEN/NOTIFY** - PostgreSQL pub/sub not implemented
- **No COPY** - Bulk data operations not supported
- **Synchronous Query Execution** - One query at a time

## Usage Examples

### SELECT Query

**User**: "Connect to PostgreSQL and select all users"

**LLM Action**:

```json
{
  "type": "execute_query",
  "query": "SELECT * FROM users"
}
```

### INSERT Query

**User**: "Insert a new user named Alice"

**LLM Action**:

```json
{
  "type": "execute_query",
  "query": "INSERT INTO users (name, email) VALUES ('Alice', 'alice@example.com')"
}
```

### Transaction Example

**User**: "Begin a transaction, update user 123, and commit"

**LLM Actions**:

```json
[
  {
    "type": "begin_transaction"
  },
  {
    "type": "execute_query",
    "query": "UPDATE users SET active = true WHERE id = 123"
  },
  {
    "type": "commit_transaction"
  }
]
```

### DDL Query

**User**: "Create a table named logs"

**LLM Action**:

```json
{
  "type": "execute_query",
  "query": "CREATE TABLE logs (id SERIAL PRIMARY KEY, message TEXT, created_at TIMESTAMP DEFAULT NOW())"
}
```

## Testing Strategy

See `tests/client/postgresql/CLAUDE.md` for E2E testing approach.

## Future Enhancements

- **TLS Support** - Add rustls/native-tls configuration
- **Prepared Statements** - Use extended query protocol
- **Type-Safe Results** - Better type conversion (not all strings)
- **Connection Pooling** - Support multiple connections
- **LISTEN/NOTIFY** - PostgreSQL pub/sub support
- **COPY Protocol** - Bulk data import/export
- **Advanced Types** - JSON, arrays, custom types
- **Authentication Methods** - SCRAM-SHA-256, MD5, etc.
- **Connection Options** - SSL mode, timeouts, keepalive
