# MySQL Protocol E2E Tests

## Test Overview

Tests MySQL server implementation using real `mysql_async` client library. Validates query execution, multi-row results, and DDL operations.

## Test Strategy

**Consolidated Approach**: Multiple test functions but each tests a distinct MySQL feature (simple query, multi-row, DDL). Each test spawns its own server with specific instructions.

**Why not more consolidated?** MySQL tests are already efficient - each test targets a different protocol feature and completes quickly.

## LLM Call Budget

### Test: `test_mysql_simple_query`
- **1 server startup** (scripting disabled, action-based only)
- **1 SELECT 1 query** → LLM call for query response
- **Total: 2 LLM calls**

### Test: `test_mysql_multi_row_query`
- **1 server startup**
- **1 SELECT * FROM users query** → LLM call for multi-row response
- **Total: 2 LLM calls**

### Test: `test_mysql_create_table`
- **1 server startup**
- **1 CREATE TABLE query** → LLM call for DDL response
- **Total: 2 LLM calls**

**Total for MySQL test suite: 6 LLM calls** (well under 10 limit)

## Scripting Usage

**Scripting Disabled**: All tests use `ServerConfig::new()` which disables scripting by default. MySQL tests rely on action-based responses for flexibility in testing different query patterns.

**Why no scripting?** MySQL queries are highly variable (SELECT vs DDL vs DML), making scripting less practical. Action-based responses provide better test coverage.

## Client Library

**mysql_async** v0.34:
- Full-featured async MySQL client using tokio
- Supports both simple and prepared statements
- Handles connection handshake and authentication
- Provides typed result extraction
- Used for protocol correctness validation

**Client Setup**:
```rust
let opts = mysql_async::OptsBuilder::default()
    .ip_or_hostname("127.0.0.1")
    .tcp_port(port)
    .user(Some("root"))
    .pass(Some(""));
let pool = mysql_async::Pool::new(opts);
let mut conn = pool.get_conn().await?;
```

## Expected Runtime

**Model**: qwen3-coder:30b (default)
**Total Runtime**: ~40-60 seconds for all 3 tests
**Breakdown**:
- Each test: ~15-20 seconds (1 startup + 1 query call)
- Variability: LLM response time, network latency

**Optimization**: Tests could be parallelized but already fast enough individually.

## Failure Rate

**Historical**: ~5% failure rate
**Causes**:
1. **Connection timeout**: Client timeout (10s) expires before LLM responds
2. **Type mismatch**: LLM returns wrong column type (e.g., VARCHAR instead of INT)
3. **Empty result**: LLM forgets to include query_response action
4. **Variable queries**: `SELECT @@*` system variable queries may confuse LLM

**Mitigation**:
- Explicit prompts for system variable queries
- Timeout extended to 10s (may need longer for slow models)
- Test retries on timeout failures

## Test Cases

### 1. Simple Query (`test_mysql_simple_query`)
**Validates**: Basic SELECT query with single row
- Connects to MySQL server
- Executes `SELECT 1`
- Verifies result is integer `1`
- **Expected LLM Response**: `mysql_query_response` with INT column

### 2. Multi-Row Query (`test_mysql_multi_row_query`)
**Validates**: SELECT query returning multiple rows
- Executes `SELECT * FROM users`
- Expects 3 rows: Alice, Bob, Charlie
- Verifies row count and data structure
- **Expected LLM Response**: `mysql_query_response` with 3-row array

### 3. CREATE TABLE (`test_mysql_create_table`)
**Validates**: DDL operation handling
- Executes `CREATE TABLE test (id INT PRIMARY KEY)`
- Expects success or non-fatal error
- Tests server doesn't crash on DDL
- **Expected LLM Response**: `mysql_ok` with affected_rows=1

## Known Issues

### System Variable Queries
**Issue**: `SELECT @@version_comment`, `SELECT @@max_allowed_packet` etc.
**Symptom**: mysql_async client sends these during connection setup, LLM may not handle them
**Workaround**: Prompt explicitly instructs LLM to return `mysql_query_response` for `SELECT @@*` queries
**Example Fix**: `For SELECT @@* queries, return mysql_query_response columns=[{name:'value',type:'VARCHAR'}] rows=[['1000']]`

### Connection Timeout
**Issue**: LLM takes >10s to respond, client times out
**Symptom**: "Connection timeout" error
**Workaround**: Consider increasing timeout to 30s for slow models
**Not Flaky**: Consistent on slow hardware/models

### Type Precision
**Issue**: LLM may return string "1" instead of integer 1
**Symptom**: Type parsing error in mysql_async
**Workaround**: Implementation converts JSON to strings; client parses strings to expected types
**Status**: Works correctly in practice

## Test Execution

```bash
# Build release binary first (REQUIRED)
cargo build --release --all-features

# Run all MySQL tests
cargo test --features e2e-tests,mysql --test server::mysql::test

# Run specific test
cargo test --features e2e-tests,mysql --test server::mysql::test test_mysql_simple_query

# Run with output
cargo test --features e2e-tests,mysql --test server::mysql::test -- --nocapture
```

## Test Output Example

```
=== E2E Test: MySQL Simple Query ===
Server started on port 54321
Connecting to MySQL server...
✓ MySQL connected
Executing SELECT 1...
✓ Received correct result: 1
✓ MySQL simple query test passed
```

## Future Improvements

1. **Prepared Statements**: Test PREPARE/EXECUTE/CLOSE flow explicitly
2. **Transactions**: Test BEGIN/COMMIT/ROLLBACK sequences
3. **Error Handling**: Test LLM-generated error responses (once opensrv supports errors)
4. **Binary Protocol**: Test prepared statements with binary data
5. **Consolidation**: Merge tests into single server with multiple queries
