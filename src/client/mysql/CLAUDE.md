# MySQL Client Implementation

## Overview

The MySQL client allows NetGet to connect to MySQL database servers and execute SQL queries under LLM control. The LLM
can execute SELECT, INSERT, UPDATE, DELETE queries, manage transactions, and analyze query results.

## Library Choice

**Primary Library:** `mysql_async` v0.34

**Rationale:**

- **Async-first design** - Built on Tokio, integrates seamlessly with NetGet's async architecture
- **Full protocol support** - Implements MySQL wire protocol with all data types
- **Connection pooling** - Efficient connection management (though we use single connections)
- **Mature crate** - Well-maintained, widely used in production
- **Type conversion** - Automatic conversion between MySQL and Rust types

**Alternative Considered:** `sqlx`

- More generic (supports multiple databases)
- Compile-time query checking with macros
- Rejected because: More complex for our use case, requires compile-time database connection for query validation

## Architecture

### Connection Model

**Type:** Single persistent connection per client instance

**Flow:**

1. Parse startup params (username, password, database)
2. Build `OptsBuilder` with connection details
3. Establish `Conn` to MySQL server
4. Wrap connection in `Arc<Mutex<Conn>>` for shared access
5. Call LLM with `mysql_connected` event
6. Execute queries asynchronously based on LLM actions

**No Read Loop:** Unlike TCP/Redis clients, MySQL client is **query-response** based:

- LLM initiates queries via actions
- Queries are executed synchronously (await response)
- Results trigger new LLM call with `mysql_result_received` event
- No background read loop needed (connection is idle between queries)

### State Machine

Uses client connection state machine (Idle → Processing):

- **Idle:** Ready to execute queries
- **Processing:** LLM call in progress, queue new queries
- No "Accumulating" state (MySQL has discrete query/response, not streaming)

State transitions:

1. LLM action → `try_start_client_llm_call()` → Processing
2. Execute query → Get result
3. Call LLM with result → `finish_client_llm_call()` → Idle

### Data Flow

```
User Instruction
    ↓
LLM generates action: execute_query("SELECT * FROM users")
    ↓
execute_llm_action() → Check state machine
    ↓
Execute query via mysql_async
    ↓
Convert Row results to JSON
    ↓
Call LLM with mysql_result_received event
    ↓
LLM analyzes results, generates next action
    ↓
Loop
```

## LLM Integration

### Event Types

**1. `mysql_connected`**

- **Trigger:** Initial connection to MySQL server
- **Data:** `remote_addr` (server address)
- **LLM Decision:** Execute initial query, set up schema, begin transaction

**2. `mysql_result_received`**

- **Trigger:** Query result received from server
- **Data:**
    - `result`: Array of row objects (JSON)
    - `row_count`: Number of rows returned
- **LLM Decision:** Analyze results, execute follow-up queries, commit/rollback transaction

### Actions

**Async Actions (User-Triggered):**

1. `execute_query` - Execute any SQL query
    - Parameters: `query` (SQL string)
    - Example: `SELECT * FROM users WHERE id = 1`

2. `begin_transaction` - Start a transaction
    - Executes: `BEGIN`

3. `commit_transaction` - Commit current transaction
    - Executes: `COMMIT`

4. `rollback_transaction` - Rollback current transaction
    - Executes: `ROLLBACK`

5. `disconnect` - Close connection

**Sync Actions (Response-Triggered):**

1. `execute_query` - Execute query based on previous result
2. `wait_for_more` - Wait without executing new queries

### Structured Data Design

**CRITICAL:** No base64-encoded binary data. All queries are SQL strings (text).

**Query Examples:**

```json
{
  "type": "execute_query",
  "query": "SELECT id, name, email FROM users WHERE active = 1"
}
```

**Result Format:**

```json
{
  "result": [
    {"id": 1, "name": "Alice", "email": "alice@example.com"},
    {"id": 2, "name": "Bob", "email": "bob@example.com"}
  ],
  "row_count": 2
}
```

### Type Conversion

MySQL types → JSON:

- `NULL` → `null`
- `INT/BIGINT` → `number`
- `VARCHAR/TEXT` → `string`
- `FLOAT/DOUBLE` → `number`
- `DATE/DATETIME` → `string` (ISO format)
- `TIME` → `string` (duration format)
- `BLOB` → `string` (UTF-8 lossy)

## Startup Parameters

**Optional parameters** when opening client:

1. `username` - MySQL username (default: "root")
2. `password` - MySQL password (default: "")
3. `database` - Initial database to connect to (default: none)

**Example:**

```json
{
  "username": "myuser",
  "password": "mypass",
  "database": "mydb"
}
```

## Query Execution

### Simple Queries

LLM generates SQL, client executes via `conn.query()`:

```rust
let result: Vec<Row> = conn_guard.query(query_str).await?;
```

Results are converted to JSON arrays and sent to LLM.

### Transactions

LLM controls transaction boundaries:

1. `begin_transaction` → Execute `BEGIN`
2. Multiple `execute_query` actions within transaction
3. `commit_transaction` → Execute `COMMIT`
   OR
   `rollback_transaction` → Execute `ROLLBACK`

### Error Handling

Query errors:

- Set client status to `Error(message)`
- Update UI
- Finish LLM call (return to Idle state)
- Do NOT disconnect (allow LLM to retry or fix query)

## Limitations

### 1. No Prepared Statements (Yet)

Current implementation uses simple text queries. Prepared statements could be added:

```rust
conn.exec("SELECT * FROM users WHERE id = ?", (user_id,)).await?;
```

**Why not implemented:** LLM generates SQL strings naturally, prepared statements add complexity.

### 2. Single Connection

No connection pooling. Each client instance has one connection. For high concurrency, open multiple client instances.

### 3. No Streaming Results

Large result sets are loaded entirely into memory. For huge queries (millions of rows), consider:

- LIMIT clauses
- Pagination
- Streaming with `query_iter()`

### 4. Limited Type Support

Complex types (JSON, GEOMETRY, ENUM) are converted to strings. For precise handling, enhance type conversion.

### 5. No Binary Protocol Features

Uses text protocol. Binary protocol (for prepared statements) not utilized.

## Security Considerations

### SQL Injection

**CRITICAL:** LLM generates raw SQL strings. Potential for injection if user input flows to LLM without sanitization.

**Mitigations:**

- LLM prompt engineering: "Never execute DROP, DELETE FROM without WHERE clause"
- Read-only user accounts
- Database permissions
- Future: Prepared statements with parameter binding

### Credential Management

Credentials passed in startup params. For production:

- Use secrets management
- Environment variables
- Credential vaulting

## Example Prompts

### 1. Simple Query

```
Connect to MySQL at localhost:3306 as root with password 'secret' and database 'testdb'.
Query the users table and show all active users.
```

LLM Flow:

1. Connected → `execute_query("SELECT * FROM users WHERE active = 1")`
2. Result received → Analyze rows → `disconnect`

### 2. Transaction

```
Connect to MySQL at localhost:3306 and transfer $100 from account 1 to account 2.
Use a transaction to ensure atomicity.
```

LLM Flow:

1. Connected → `begin_transaction`
2. `execute_query("UPDATE accounts SET balance = balance - 100 WHERE id = 1")`
3. Result → `execute_query("UPDATE accounts SET balance = balance + 100 WHERE id = 2")`
4. Result → `commit_transaction`
5. `disconnect`

### 3. Schema Analysis

```
Connect to MySQL at localhost:3306 and describe the structure of the 'products' table.
```

LLM Flow:

1. Connected → `execute_query("DESCRIBE products")`
2. Result → Analyze schema → `disconnect`

## Future Enhancements

### 1. Prepared Statements

Add `execute_prepared` action:

```json
{
  "type": "execute_prepared",
  "query": "SELECT * FROM users WHERE id = ?",
  "params": [123]
}
```

### 2. Streaming Results

For large result sets:

```rust
let mut result = conn.query_iter(query_str).await?;
while let Some(row) = result.next().await? {
    // Send row to LLM incrementally
}
```

### 3. Multi-Statement Queries

Execute multiple statements in one call:

```sql
CREATE TABLE temp (id INT); INSERT INTO temp VALUES (1);
```

### 4. Connection Pooling

Use `mysql_async::Pool` for better resource management:

```rust
let pool = Pool::new(opts);
let conn = pool.get_conn().await?;
```

### 5. LOAD DATA INFILE

Bulk data loading for LLM-generated datasets.

## Testing Strategy

See `tests/client/mysql/CLAUDE.md` for E2E test details.

**Test Approach:**

- Docker MySQL container (official `mysql:8` image)
- Seed database with test schema/data
- LLM executes queries, validates results
- Test transactions (commit/rollback)
- Test error handling (invalid queries)

**LLM Call Budget:** < 10 calls per suite

- 1 connection test
- 1 simple SELECT
- 1 transaction test
- 1 error handling test

## Maintenance Notes

**Dependencies:**

- `mysql_async = "0.34"` - Main MySQL client library
- Feature-gated in `Cargo.toml` under `[features]`

**Code Organization:**

- `mod.rs` - Connection logic, query execution, LLM integration
- `actions.rs` - `Client` trait implementation, action definitions
- `CLAUDE.md` - This document

**Common Issues:**

1. **Connection refused** - Ensure MySQL is running, check host/port
2. **Access denied** - Verify username/password
3. **Unknown database** - Check database exists or omit database param
4. **Query errors** - LLM generates invalid SQL, improves with better prompting

## References

- [mysql_async crate](https://docs.rs/mysql_async/)
- [MySQL Protocol](https://dev.mysql.com/doc/dev/mysql-server/latest/page_protocol_basics.html)
- [SQL Injection Prevention](https://cheatsheetseries.owasp.org/cheatsheets/SQL_Injection_Prevention_Cheat_Sheet.html)
