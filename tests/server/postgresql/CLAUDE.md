# PostgreSQL Protocol E2E Tests

## Test Overview

Tests PostgreSQL server implementation using real `tokio-postgres` client library. Validates simple query execution, multi-row results, DDL operations, and error responses.

## Test Strategy

**Consolidated Approach**: Multiple test functions but each tests a distinct PostgreSQL feature. Each test spawns its own server with specific instructions.

**Why not more consolidated?** PostgreSQL tests are already efficient and test different aspects of the protocol (queries, DDL, errors).

## LLM Call Budget

### Test: `test_postgresql_simple_query`
- **1 server startup** (scripting disabled, action-based only)
- **1 SELECT 1 query** → LLM call for query response
- **Total: 2 LLM calls**

### Test: `test_postgresql_multi_row_query`
- **1 server startup**
- **1 SELECT * FROM users query** → LLM call for multi-row response
- **Total: 2 LLM calls**

### Test: `test_postgresql_create_table`
- **1 server startup**
- **1 CREATE TABLE query** → LLM call for DDL response
- **Total: 2 LLM calls**

### Test: `test_postgresql_error_response`
- **1 server startup**
- **1 SELECT * FROM invalid_table query** → LLM call for error response
- **Total: 2 LLM calls**

**Total for PostgreSQL test suite: 8 LLM calls** (well under 10 limit)

## Scripting Usage

**Scripting Disabled**: All tests use `ServerConfig::new()` which disables scripting by default. PostgreSQL tests rely on action-based responses for flexibility in testing different query patterns and error conditions.

**Why no scripting?** PostgreSQL queries are highly variable (SELECT vs DDL vs errors), making scripting less practical. Action-based responses provide better test coverage.

## Client Library

**tokio-postgres** v0.7:
- Full-featured async PostgreSQL client using tokio
- Supports both simple and extended query protocols
- Handles connection startup and authentication
- Provides typed result extraction
- Used for protocol correctness validation

**Client Setup**:
```rust
let connection_string = format!(
    "host=127.0.0.1 port={} user=postgres dbname=test connect_timeout=60 options='-c statement_timeout=0'",
    port
);
let (client, connection) = tokio_postgres::connect(&connection_string, NoTls).await?;
tokio::spawn(async move {
    if let Err(e) = connection.await {
        eprintln!("connection error: {}", e);
    }
});
```

**Timeout Configuration**:
- `connect_timeout=60` - Long connection timeout for slow LLM startup
- `statement_timeout=0` - Disables server-side query timeout (critical for extended queries)

## Expected Runtime

**Model**: qwen3-coder:30b (default)
**Total Runtime**: ~60-90 seconds for all 4 tests
**Breakdown**:
- Each test: ~15-20 seconds (1 startup + 1 query call)
- Extended query timeout adds variability
- Variability: LLM response time, pgwire protocol overhead

**Optimization**: Tests could be parallelized but already fast enough individually.

## Failure Rate

**Historical**: ~10-15% failure rate (higher than MySQL/Redis)
**Causes**:
1. **Extended query timeout**: LLM call does not complete within pgwire timeout (CRITICAL)
2. **Connection timeout**: Client timeout (60s) expires before LLM responds
3. **Type mismatch**: LLM returns wrong column type (e.g., text instead of int4)
4. **Empty result**: LLM forgets to include query_response action
5. **Version queries**: `SELECT version()` may confuse LLM

**Mitigation**:
- Explicit prompts for system queries (`SELECT version()`)
- Extended timeouts (60s connection, statement_timeout=0)
- Tests use simple query protocol when possible
- Fallback to empty result set for SELECT queries

**Known Flaky Test**: None explicitly flaky, but extended query protocol is unreliable

## Test Cases

### 1. Simple Query (`test_postgresql_simple_query`)
**Validates**: Basic SELECT query with single row using simple query protocol
- Connects to PostgreSQL server
- Executes `SELECT 1`
- Verifies result is integer `1`
- **Expected LLM Response**: `postgresql_query_response` with int4 column
- **Protocol**: Simple query (no prepare/bind/execute)

### 2. Multi-Row Query (`test_postgresql_multi_row_query`)
**Validates**: SELECT query returning multiple rows
- Executes `SELECT * FROM users`
- Expects 3 rows: Alice, Bob, Charlie
- Verifies row count and data structure
- **Expected LLM Response**: `postgresql_query_response` with 3-row array

### 3. CREATE TABLE (`test_postgresql_create_table`)
**Validates**: DDL operation handling
- Executes `CREATE TABLE test (id INT PRIMARY KEY)`
- Expects success or non-fatal error
- Tests server doesn't crash on DDL
- **Expected LLM Response**: `postgresql_ok` with tag='CREATE TABLE'

### 4. Error Response (`test_postgresql_error_response`)
**Validates**: Error handling and proper error codes
- Executes `SELECT * FROM invalid_table`
- Expects error response
- Verifies error code 42P01 (undefined_table)
- **Expected LLM Response**: `postgresql_error` with code/message

## Known Issues

### Extended Query Protocol Timeout (CRITICAL)
**Issue**: LLM calls in `ExtendedQueryHandler::do_query()` do not complete within pgwire timeout
**Symptom**: Prepared statements and extended protocol queries timeout or hang
**Affected Tests**: Any test using `client.query()` with parameters (extended protocol)
**Workaround**:
- Use simple queries (`client.simple_query()`) instead
- Increase timeouts: `connect_timeout=60 statement_timeout=0`
- Tests avoid parameterized queries
**Status**: **UNRESOLVED** - root cause unknown, requires investigation

### Version Query
**Issue**: `SELECT version()` system query may not be handled by LLM
**Symptom**: tokio-postgres client sends version query during startup
**Workaround**: Prompt explicitly instructs LLM to return `postgresql_query_response` for version queries
**Example Fix**: `For SELECT version() queries, return postgresql_query_response columns=[{name:'version',type:'text'}] rows=[['PostgreSQL 16.0 (LLM)']]`

### Connection Timeout
**Issue**: LLM takes >60s to respond, client times out
**Symptom**: "Connection timeout" error during `tokio_postgres::connect()`
**Workaround**: Extended timeout to 60s (may need longer for slow models)
**Not Flaky**: Consistent on slow hardware/models

### Boolean Format
**Issue**: PostgreSQL uses `t`/`f` for booleans, not `true`/`false`
**Symptom**: Client parsing errors if LLM returns `true`/`false`
**Workaround**: Implementation correctly converts JSON booleans to `t`/`f` format
**Status**: Works correctly in practice

## Test Execution

```bash
# Build release binary first (REQUIRED)
./cargo-isolated.sh build --release --all-features

# Run all PostgreSQL tests
./cargo-isolated.sh test --features postgresql --test server::postgresql::test

# Run specific test
./cargo-isolated.sh test --features postgresql --test server::postgresql::test test_postgresql_simple_query

# Run with output
./cargo-isolated.sh test --features postgresql --test server::postgresql::test -- --nocapture
```

## Test Output Example

```
=== E2E Test: PostgreSQL Simple Query ===
Server started on port 54321
Connecting to PostgreSQL server...
✓ PostgreSQL connected
Executing SELECT 1...
✓ Received result: 1
✓ PostgreSQL simple query test passed
```

## Future Improvements

1. **Fix Extended Query Timeout**: Investigate pgwire timeout behavior and resolve LLM completion issue
2. **Prepared Statements**: Test explicit PREPARE/EXECUTE once extended protocol works
3. **Transactions**: Test BEGIN/COMMIT/ROLLBACK sequences
4. **Binary Format**: Test binary protocol encoding (FieldFormat::Binary)
5. **Complex Types**: Test arrays, JSON, composite types
6. **Consolidation**: Merge tests into single server with multiple queries (once reliable)
