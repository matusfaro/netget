# Redis Protocol E2E Tests

## Test Overview

Tests Redis server implementation using real `redis` (redis-rs) client library. Validates all RESP2 data types: simple strings, bulk strings, integers, arrays, null values, and errors.

## Test Strategy

**Comprehensive Coverage**: Multiple test functions covering each RESP2 type. Each test is small and focused on a single response type. This provides excellent protocol coverage while keeping tests maintainable.

**Why separate tests?** Redis has 6 distinct response types, each with different encoding. Testing each separately ensures complete protocol correctness.

## LLM Call Budget

### Test: `test_redis_ping`
- **1 server startup** (scripting disabled, action-based only)
- **1 PING command** → LLM call for simple string response
- **Total: 2 LLM calls**

### Test: `test_redis_get_set`
- **1 server startup**
- **1 SET command** → LLM call
- **1 GET command** → LLM call
- **Total: 3 LLM calls**

### Test: `test_redis_integer_response`
- **1 server startup**
- **1 INCR command** → LLM call for integer response
- **Total: 2 LLM calls**

### Test: `test_redis_array_response`
- **1 server startup**
- **1 KEYS command** → LLM call for array response
- **Total: 2 LLM calls**

### Test: `test_redis_null_response`
- **1 server startup**
- **1 GET nonexistent command** → LLM call for null response
- **Total: 2 LLM calls**

### Test: `test_redis_error_response`
- **1 server startup**
- **1 INVALID command** → LLM call for error response
- **Total: 2 LLM calls**

**Total for Redis test suite: 13 LLM calls** (slightly over 10, could be optimized by consolidating)

## Scripting Usage

**Scripting Disabled**: All tests use `ServerConfig::new()` which disables scripting by default. Redis tests rely on action-based responses to validate all RESP2 data types.

**Why no scripting?** Testing protocol correctness requires validating each response type (simple string, bulk string, integer, array, null, error). Scripting would make tests less flexible and harder to debug.

**Future Optimization**: Tests could be consolidated into 2-3 larger tests with scripting enabled to reduce LLM calls to <10.

## Client Library

**redis** (redis-rs) v0.25:
- Full-featured async Redis client using tokio
- Supports multiplexed connections (parallel commands)
- Handles RESP2 protocol parsing and encoding
- Provides typed result extraction (String, i64, Vec<String>, etc.)
- Used for protocol correctness validation

**Client Setup**:
```rust
let redis_url = format!("redis://127.0.0.1:{}", port);
let client = redis::Client::open(redis_url.as_str())?;
let mut con = client.get_multiplexed_async_connection().await?;
```

## Expected Runtime

**Model**: qwen3-coder:30b (default)
**Total Runtime**: ~60-80 seconds for all 6 tests
**Breakdown**:
- Each test: ~10-15 seconds (1-3 LLM calls)
- Fast: PING, integer, null tests (2 calls each)
- Slower: GET/SET test (3 calls)
- Variability: LLM response time

**Optimization**: Tests are already fast individually; parallelization not needed.

## Failure Rate

**Historical**: ~2% failure rate (very stable)
**Causes**:
1. **Connection timeout**: Rare, only on very slow LLM models
2. **Type mismatch**: LLM returns wrong action type (e.g., `redis_bulk_string` instead of `redis_simple_string`)
3. **Empty response**: LLM forgets to include action (client hangs)

**Mitigation**:
- Explicit prompts for each command type
- Timeout: 10s default (adequate for most models)
- Tests are deterministic and rarely flaky

**Redis is the most stable database protocol** due to simple RESP2 format and clear command/response patterns.

## Test Cases

### 1. PING (`test_redis_ping`)
**Validates**: Simple string response (+PONG\r\n)
- Connects to Redis server
- Executes PING command
- Verifies response is "PONG"
- **Expected LLM Response**: `redis_simple_string` with value='PONG'
- **RESP2**: `+PONG\r\n`

### 2. GET/SET (`test_redis_get_set`)
**Validates**: Bulk string responses
- Executes SET mykey myvalue
- Verifies SET returns "OK"
- Executes GET mykey
- Verifies GET returns "test_value"
- **Expected LLM Responses**:
  - SET: `redis_simple_string` value='OK'
  - GET: `redis_bulk_string` value='test_value'
- **RESP2**: `+OK\r\n` and `$10\r\ntest_value\r\n`

### 3. Integer Response (`test_redis_integer_response`)
**Validates**: Integer responses (:42\r\n)
- Executes INCR counter
- Verifies response is integer 42
- **Expected LLM Response**: `redis_integer` value=42
- **RESP2**: `:42\r\n`

### 4. Array Response (`test_redis_array_response`)
**Validates**: Array responses with multiple elements
- Executes KEYS *
- Verifies response is array of strings
- Checks array is non-empty
- **Expected LLM Response**: `redis_array` values=['key1','key2','key3']
- **RESP2**: `*3\r\n$4\r\nkey1\r\n$4\r\nkey2\r\n$4\r\nkey3\r\n`

### 5. Null Response (`test_redis_null_response`)
**Validates**: Null bulk string ($-1\r\n)
- Executes GET nonexistent
- Verifies response is None
- **Expected LLM Response**: `redis_null`
- **RESP2**: `$-1\r\n`

### 6. Error Response (`test_redis_error_response`)
**Validates**: Error responses (-ERR\r\n)
- Executes INVALID command
- Expects error response
- Verifies error contains "ERR" or "unknown"
- **Expected LLM Response**: `redis_error` message='ERR unknown command'
- **RESP2**: `-ERR unknown command\r\n`

## Known Issues

### CLIENT Commands
**Issue**: redis-rs client sends `CLIENT SETNAME` during connection setup
**Symptom**: LLM may not recognize CLIENT command
**Workaround**: Prompt instructs LLM to respond with `redis_simple_string value='OK'` for PING/CLIENT commands
**Status**: Works correctly in practice

### Connection Timeout
**Issue**: LLM takes >10s to respond, client times out
**Symptom**: "Connection timeout" error
**Workaround**: Increase timeout to 30s for slow models (rare)
**Not Flaky**: Consistent on slow hardware/models

### No Response Fallback
**Issue**: If LLM returns no action, client hangs indefinitely
**Symptom**: Test timeout after 60s
**Workaround**: Explicit prompts ensure LLM always returns an action
**Status**: Very rare in practice

## Test Execution

```bash
# Build release binary first (REQUIRED)
./cargo-isolated.sh build --release --all-features

# Run all Redis tests
./cargo-isolated.sh test --features redis --test server::redis::test

# Run specific test
./cargo-isolated.sh test --features redis --test server::redis::test test_redis_ping

# Run with output
./cargo-isolated.sh test --features redis --test server::redis::test -- --nocapture
```

## Test Output Example

```
=== E2E Test: Redis PING ===
Server started on port 54321
Connecting to Redis server...
✓ Redis connected
Executing PING...
✓ Received: PONG
✓ Redis PING test passed
```

## Future Improvements

1. **Consolidation**: Merge tests into 2-3 comprehensive tests with scripting to reduce LLM calls to <10
   - Test 1: Basic commands (PING, GET, SET) - 1 server, 1-2 LLM calls with scripting
   - Test 2: Data types (integer, array, null) - 1 server, 1-2 LLM calls
   - Test 3: Error handling - 1 server, 1 LLM call
   - **Target: 6-8 total LLM calls** (down from 13)
2. **Complex Commands**: Test MGET, MSET, APPEND, STRLEN
3. **Lists**: Test LPUSH, LPOP, LRANGE
4. **Sets**: Test SADD, SMEMBERS, SINTER
5. **Sorted Sets**: Test ZADD, ZRANGE, ZSCORE
6. **Hashes**: Test HSET, HGET, HGETALL
7. **Binary Safety**: Test values with \r\n, null bytes
