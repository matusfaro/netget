# E2E Test Failure Report

**Date**: 2025-11-16
**Test Run**: `./cargo-isolated.sh test --all-features --no-fail-fast -- --test-threads=100`
**Status**: INCOMPLETE (killed after ~35 minutes due to hanging tests)
**Total Failed**: 144 tests
**Total Hanging**: 16 tests (exceeded 60s timeout)

## Executive Summary

The e2e test suite has **160 failures** across **8 priority groups**. The test run was terminated early due to hanging tests in Redis and PyPI protocols that blocked completion. Tests are grouped by root cause to enable parallel fixes by multiple Claude instances.

### Priority Order

1. **CRITICAL** - Hanging Tests (16): Blocking test suite completion
2. **HIGH** - Bluetooth BLE (21): Common platform/dependency issue
3. **HIGH** - Database Protocols (23): Mock expectation issues
4. **HIGH** - UDP Protocol Mocks (7): Need dynamic transaction ID matching
5. **MEDIUM** - Application Protocols (26): Various mock issues
6. **MEDIUM** - HTTP Scheduled Tasks (3): Timeout/async issues
7. **MEDIUM** - Datalink (3): Packet capture mocking
8. **LOW** - Single Failures (14): Protocol-specific investigation

---

## Group 1: HANGING TESTS (CRITICAL)

**Priority**: CRITICAL
**Tests**: 16
**Status**: BLOCKING - Prevents full test suite completion
**Estimated Fix Time**: 2-4 hours

### Failed Tests

#### Redis (6 tests) - All mock-based tests hanging >60s
```
server::redis::e2e_test::redis_server_tests::test_redis_array_response_with_mocks
server::redis::e2e_test::redis_server_tests::test_redis_error_response_with_mocks
server::redis::e2e_test::redis_server_tests::test_redis_get_set_with_mocks
server::redis::e2e_test::redis_server_tests::test_redis_integer_response_with_mocks
server::redis::e2e_test::redis_server_tests::test_redis_null_response_with_mocks
server::redis::e2e_test::redis_server_tests::test_redis_ping_with_mocks
```

#### PyPI (5 tests) - Both mock and non-mock tests hanging
```
server::pypi::e2e_test_mocked::pypi_server_tests::test_pypi_package_index_with_mocks
server::pypi::e2e_test_mocked::pypi_server_tests::test_pypi_package_not_found_with_mocks
server::pypi::e2e_test_mocked::pypi_server_tests::test_pypi_package_page_with_mocks
server::pypi::e2e_test::test_pypi_comprehensive
server::pypi::e2e_test::test_pypi_single_package
```

#### Cassandra (3 tests) - Connection/query tests hanging then failing
```
server::cassandra::e2e_test::e2e_cassandra::test_cassandra_connection
server::cassandra::e2e_test::e2e_cassandra::test_cassandra_prepared_statement_param_mismatch
server::cassandra::e2e_test::e2e_cassandra::test_cassandra_select_query
```

#### Git (1 test) - System git integration hanging
```
server::git::e2e_test::test_git_clone_with_system_git
```

#### NPM (1 test) - NPM CLI integration completed successfully
```
server::npm::e2e_test::test_npm_with_real_cli (showed >60s warning but passed)
```

### Root Cause Analysis

**Redis Tests**: These tests use `.with_mock()` pattern and call `.verify_mocks().await?` at the end. The hanging suggests:
1. Mock expectations not being met (verify waits indefinitely)
2. Deadlock in mock verification system
3. Client connection not closing properly before verification

**PyPI Tests**: Mix of mocked and non-mocked tests hanging suggests HTTP server or client issue:
1. HTTP connections not closing
2. Infinite loops in request handling
3. Mock verification deadlock (mocked tests)

**Cassandra Tests**: Hung for >60s then failed, indicating:
1. Connection timeout during handshake
2. Protocol negotiation issues
3. Mock expectations not matching actual protocol flow

**Git/NPM Tests**: External tool integration, likely:
1. Process not terminating
2. Git clone hanging on network or subprocess
3. NPM install/command hanging

### Test File Locations

- `tests/server/redis/e2e_test.rs`
- `tests/server/redis/CLAUDE.md`
- `tests/server/pypi/e2e_test.rs`
- `tests/server/pypi/e2e_test_mocked.rs`
- `tests/server/pypi/CLAUDE.md`
- `tests/server/cassandra/e2e_test.rs`
- `tests/server/cassandra/CLAUDE.md`
- `tests/server/git/e2e_test.rs`
- `tests/server/npm/e2e_test.rs`

### Fix Instructions

#### Step 1: Debug Redis Hanging Tests

1. **Add timeout to mock verification**:
   ```rust
   // In tests/server/redis/e2e_test.rs
   use tokio::time::{timeout, Duration};

   // Replace: server.verify_mocks().await?;
   // With:
   timeout(Duration::from_secs(5), server.verify_mocks())
       .await
       .map_err(|_| anyhow::anyhow!("Mock verification timed out after 5s"))??;
   ```

2. **Add explicit connection cleanup**:
   ```rust
   // Before verify_mocks
   drop(con); // Explicitly drop Redis connection
   tokio::time::sleep(Duration::from_millis(100)).await; // Allow cleanup
   ```

3. **Check mock expectations match actual calls**:
   - Review Redis client behavior (CLIENT SETINFO, CLIENT SETNAME commands)
   - Add `.debug()` to mock builder to see what's being matched
   - Ensure generic `.on_event("redis_command")` catches all commands

4. **Run single test with debugging**:
   ```bash
   ./cargo-isolated.sh test --no-default-features --features redis \
     --test server::redis::e2e_test test_redis_ping_with_mocks -- --nocapture
   ```

#### Step 2: Debug PyPI Hanging Tests

1. **Check HTTP connection handling**:
   ```rust
   // In tests/server/pypi/e2e_test_mocked.rs
   // Ensure reqwest client is properly configured
   let client = reqwest::Client::builder()
       .timeout(Duration::from_secs(5))
       .build()?;
   ```

2. **Add timeouts to all HTTP requests**:
   ```rust
   timeout(Duration::from_secs(10), client.get(url).send()).await??;
   ```

3. **Check server shutdown**:
   - Ensure HTTP server closes all connections before `verify_mocks()`
   - Add explicit `server.stop().await?` before verification

4. **Test individual endpoints**:
   ```bash
   ./cargo-isolated.sh test --no-default-features --features pypi \
     --test server::pypi::e2e_test_mocked test_pypi_package_index_with_mocks -- --nocapture
   ```

#### Step 3: Fix Cassandra Tests

1. **Add connection timeout**:
   ```rust
   // In tests/server/cassandra/e2e_test.rs
   let session = timeout(
       Duration::from_secs(10),
       cluster.connect()
   ).await??;
   ```

2. **Check Cassandra protocol handshake**:
   - Review `src/server/cassandra/mod.rs` for protocol startup
   - Ensure READY frame is sent after OPTIONS/STARTUP
   - Check mock expectations match Cassandra native protocol

3. **Simplify mock expectations**:
   - Cassandra protocol is complex with multiple handshake frames
   - Consider using scripting mode instead of mocks for simpler tests

#### Step 4: Fix Git/NPM External Tool Tests

1. **Add process timeout**:
   ```rust
   // In tests/server/git/e2e_test.rs
   use tokio::process::Command;

   let output = timeout(
       Duration::from_secs(30),
       Command::new("git")
           .args(&["clone", &clone_url, &temp_dir])
           .output()
   ).await??;
   ```

2. **Check git server endpoints**:
   - Ensure `/info/refs?service=git-upload-pack` responds correctly
   - Verify `POST /git-upload-pack` handles pack negotiation
   - Test with manual `curl` first

3. **For NPM**:
   - Test passed but showed >60s warning
   - Consider reducing test scope or adding intermediate status checks

### Success Criteria

- [ ] All Redis tests complete in <5s each
- [ ] PyPI tests complete in <10s each
- [ ] Cassandra tests connect in <5s
- [ ] Git clone test completes or fails (not hangs) in <30s
- [ ] All mocked tests properly verify expectations
- [ ] Full test suite completes without hanging

### Files to Modify

**Tests**:
- `tests/server/redis/e2e_test.rs` - Add timeouts, connection cleanup
- `tests/server/pypi/e2e_test_mocked.rs` - Add HTTP timeouts
- `tests/server/pypi/e2e_test.rs` - Add HTTP timeouts
- `tests/server/cassandra/e2e_test.rs` - Add connection timeout
- `tests/server/git/e2e_test.rs` - Add process timeout

**Protocol Implementation** (if needed):
- `src/server/redis/mod.rs` - Check connection handling
- `src/server/pypi/mod.rs` - Check HTTP server shutdown
- `src/server/cassandra/mod.rs` - Check protocol handshake

**Test Infrastructure**:
- `tests/server/helpers.rs` - May need helper for timeout wrapper

---

## Group 2: BLUETOOTH BLE FAILURES (HIGH)

**Priority**: HIGH
**Tests**: 21
**Status**: Common root cause - likely platform/dependency issue
**Estimated Fix Time**: 1-2 hours

### Failed Tests

```
server::bluetooth_ble::e2e_test::test_bluetooth_battery_service
server::bluetooth_ble::e2e_test::test_bluetooth_ble_startup
server::bluetooth_ble::e2e_test::test_bluetooth_heart_rate_server
server::bluetooth_ble_battery::e2e_test::test_battery_level_update
server::bluetooth_ble_battery::e2e_test::test_battery_service_startup
server::bluetooth_ble_beacon::e2e_test::test_eddystone_uid_advertising
server::bluetooth_ble_beacon::e2e_test::test_eddystone_url_advertising
server::bluetooth_ble_beacon::e2e_test::test_ibeacon_advertising
server::bluetooth_ble_cycling::e2e_test::test_cycling_service_startup
server::bluetooth_ble_data_stream::e2e_test::test_data_stream_service_startup
server::bluetooth_ble_environmental::e2e_test::test_environmental_service_startup
server::bluetooth_ble_file_transfer::e2e_test::test_file_transfer_service_startup
server::bluetooth_ble_gamepad::e2e_test::test_gamepad_service_startup
server::bluetooth_ble_heart_rate::e2e_test::test_heart_rate_service_startup
server::bluetooth_ble_heart_rate::e2e_test::test_heart_rate_updates
server::bluetooth_ble_presenter::e2e_test::test_presenter_service_startup
server::bluetooth_ble_proximity::e2e_test::test_proximity_service_startup
server::bluetooth_ble_remote::e2e_test::test_remote_service_startup
server::bluetooth_ble_running::e2e_test::test_running_service_startup
server::bluetooth_ble_thermometer::e2e_test::test_thermometer_service_startup
server::bluetooth_ble_weight_scale::e2e_test::test_weight_scale_service_startup
```

### Root Cause Analysis

All Bluetooth BLE tests failing suggests **common platform or dependency issue**:

1. **Missing system library**: `libbluetooth-dev` not installed (see CLAUDE.md - Claude Code for Web section)
2. **Permission issues**: BLE requires root/admin on some platforms
3. **Hardware requirement**: No Bluetooth adapter present
4. **Common initialization failure**: BLE manager/adapter initialization failing for all tests

**Evidence**: 13 different BLE protocols all failing identically suggests issue is in shared BLE infrastructure, not individual protocol implementations.

### Test File Locations

- `tests/server/bluetooth_ble/e2e_test.rs`
- `tests/server/bluetooth_ble_*/e2e_test.rs` (13 service-specific test files)
- `src/server/bluetooth_ble/mod.rs` (shared BLE infrastructure)

### Fix Instructions

#### Step 1: Check Platform and Dependencies

1. **Verify system is NOT Claude Code for Web**:
   ```bash
   ./am_i_claude_code_for_web.sh
   ```
   If "Claude Code for Web", BLE tests MUST be skipped (no `libbluetooth-dev`).

2. **Check if running on macOS**:
   ```bash
   uname -s  # Should show "Darwin" for macOS
   ```

3. **Check Bluetooth dependencies**:
   ```bash
   # On Linux
   dpkg -l | grep bluetooth

   # Should show libbluetooth-dev if installed
   ```

4. **Check for Bluetooth adapter**:
   ```bash
   # On macOS
   system_profiler SPBluetoothDataType

   # On Linux
   hciconfig -a
   ```

#### Step 2: Run Single BLE Test with Debug Output

```bash
./cargo-isolated.sh test --no-default-features --features bluetooth-ble \
  --test server::bluetooth_ble::e2e_test test_bluetooth_ble_startup -- --nocapture
```

Look for error messages:
- "Permission denied" → Need root or capabilities
- "No adapter found" → No Bluetooth hardware
- "Library not found" → Missing `libbluetooth-dev`
- Initialization errors from `bluer` or `btleplug` crate

#### Step 3: Fix Based on Error

**If missing libbluetooth-dev**:
```bash
# Linux
sudo apt-get install libbluetooth-dev

# macOS - BLE should work natively, no action needed
```

**If permission denied**:
```bash
# Add BLE capabilities (Linux)
sudo setcap 'cap_net_raw,cap_net_admin+eip' target/debug/deps/server-*

# Or run tests with sudo (not recommended)
sudo ./cargo-isolated.sh test --features bluetooth-ble ...
```

**If no adapter**:
- Tests cannot run on hardware without Bluetooth
- Consider adding `#[ignore]` or `#[cfg(has_bluetooth)]` attribute
- Document in CLAUDE.md that tests require Bluetooth hardware

**If in Claude Code for Web**:
- BLE tests should already be excluded via feature flags
- Verify `--all-features` is not used
- Use `--no-default-features` with explicit protocol list

#### Step 4: Update Test Infrastructure (if needed)

If BLE tests cannot run in all environments:

1. **Add capability detection**:
   ```rust
   // In tests/server/bluetooth_ble/e2e_test.rs
   fn has_bluetooth() -> bool {
       // Try to initialize BLE adapter
       // Return false if no hardware
   }

   #[tokio::test]
   async fn test_bluetooth_ble_startup() -> E2EResult<()> {
       if !has_bluetooth() {
           println!("⚠️  Skipping test: No Bluetooth adapter");
           return Ok(());
       }
       // ... rest of test
   }
   ```

2. **Update CLAUDE.md** to document requirements:
   ```markdown
   ## Test Requirements

   - Bluetooth hardware adapter required
   - Linux: `libbluetooth-dev` package
   - Permissions: CAP_NET_RAW, CAP_NET_ADMIN (Linux)
   - Not supported: Claude Code for Web
   ```

#### Step 5: Consider Alternative Testing Strategy

If hardware requirements are too restrictive:

1. **Mock BLE adapter**:
   - Create mock BLE adapter for tests
   - Test LLM integration without real Bluetooth

2. **Use test-only mode**:
   - Add `--test-mode` flag that simulates BLE without hardware
   - Document that real BLE testing requires hardware

### Success Criteria

- [ ] Identify root cause (dependency, permission, hardware, or initialization)
- [ ] All BLE tests pass OR properly skip when hardware unavailable
- [ ] Document BLE test requirements in CLAUDE.md
- [ ] Add graceful skipping for environments without BLE support

### Files to Modify

**Tests**:
- `tests/server/bluetooth_ble/e2e_test.rs` - Add capability detection
- All `tests/server/bluetooth_ble_*/e2e_test.rs` files - Add skipping logic

**Documentation**:
- `tests/server/bluetooth_ble/CLAUDE.md` - Document requirements
- Each service's CLAUDE.md - Document hardware needs

**Protocol** (if initialization is broken):
- `src/server/bluetooth_ble/mod.rs` - Fix BLE adapter init

---

## Group 3: DATABASE PROTOCOLS (HIGH)

**Priority**: HIGH
**Tests**: 23 (plus 8 Cassandra from Group 1)
**Status**: Mock expectations and client integration issues
**Estimated Fix Time**: 3-5 hours

### Failed Tests

#### IMAP (10 tests) - Client test failures
```
server::imap::e2e_client_test::e2e_imap_client::test_imap_capability
server::imap::e2e_client_test::e2e_imap_client::test_imap_concurrent_connections
server::imap::e2e_client_test::e2e_imap_client::test_imap_examine_readonly
server::imap::e2e_client_test::e2e_imap_client::test_imap_fetch_messages
server::imap::e2e_client_test::e2e_imap_client::test_imap_list_mailboxes
server::imap::e2e_client_test::e2e_imap_client::test_imap_login_success
server::imap::e2e_client_test::e2e_imap_client::test_imap_noop_and_logout
server::imap::e2e_client_test::e2e_imap_client::test_imap_search_messages
server::imap::e2e_client_test::e2e_imap_client::test_imap_select_mailbox
server::imap::e2e_client_test::e2e_imap_client::test_imap_status_command
```

#### DynamoDB (8 tests) - AWS SDK integration
```
server::dynamo::e2e_aws_sdk_test::tests::test_aws_sdk_batch_write
server::dynamo::e2e_aws_sdk_test::tests::test_aws_sdk_create_table
server::dynamo::e2e_aws_sdk_test::tests::test_aws_sdk_delete_item
server::dynamo::e2e_aws_sdk_test::tests::test_aws_sdk_describe_table
server::dynamo::e2e_aws_sdk_test::tests::test_aws_sdk_put_and_get_item
server::dynamo::e2e_aws_sdk_test::tests::test_aws_sdk_query
server::dynamo::e2e_aws_sdk_test::tests::test_aws_sdk_scan
server::dynamo::e2e_aws_sdk_test::tests::test_aws_sdk_update_item
```

#### PostgreSQL (4 tests)
```
server::postgresql::test::test_postgresql_create_table
server::postgresql::test::test_postgresql_error_response
server::postgresql::test::test_postgresql_multi_row_query
server::postgresql::test::test_postgresql_simple_query
```

#### MySQL (3 tests)
```
server::mysql::test::test_mysql_create_table
server::mysql::test::test_mysql_multi_row_query
server::mysql::test::test_mysql_simple_query
```

### Root Cause Analysis

**IMAP**: All client tests failing suggests:
1. IMAP server not responding to client library correctly
2. Missing mock expectations for IMAP protocol flow
3. Authentication or capability negotiation issues
4. IMAP client library expecting specific server responses

**DynamoDB**: AWS SDK tests failing indicates:
1. Endpoint configuration incorrect
2. Missing authentication/signature handling
3. Protocol format issues (AWS uses custom binary protocol)
4. Mock expectations don't match AWS SDK behavior

**PostgreSQL/MySQL**: SQL protocol tests suggest:
1. Connection handshake issues
2. Query response format incorrect
3. Missing result metadata (column names, types)
4. Mock expectations for SQL protocols incomplete

### Test File Locations

- `tests/server/imap/e2e_client_test.rs`
- `tests/server/imap/CLAUDE.md`
- `tests/server/dynamo/e2e_aws_sdk_test.rs`
- `tests/server/dynamo/CLAUDE.md`
- `tests/server/postgresql/test.rs`
- `tests/server/postgresql/CLAUDE.md`
- `tests/server/mysql/test.rs`
- `tests/server/mysql/CLAUDE.md`

### Fix Instructions

#### Step 1: Fix IMAP Tests

1. **Run single IMAP test with debugging**:
   ```bash
   ./cargo-isolated.sh test --no-default-features --features imap \
     --test server::imap::e2e_client_test test_imap_login_success -- --nocapture
   ```

2. **Check IMAP server responses**:
   - IMAP requires specific greeting: `* OK [CAPABILITY ...] Server ready`
   - LOGIN command expects tagged response: `A001 OK LOGIN completed`
   - Review `src/server/imap/mod.rs` for protocol implementation

3. **Check if using mocks**:
   ```rust
   // In tests/server/imap/e2e_client_test.rs
   // If using mocks, ensure expectations match IMAP protocol flow:

   .with_mock(|mock| {
       mock
           // Greeting on connection
           .on_event("imap_connection")
           .respond_with_actions(json!([{
               "type": "imap_send_raw",
               "data": "* OK [CAPABILITY IMAP4rev1 AUTH=PLAIN] Ready\r\n"
           }]))
           .and()
           // LOGIN command
           .on_event("imap_command")
           .and_event_data_contains("command", "LOGIN")
           .respond_with_actions(json!([{
               "type": "imap_ok_response",
               "tag": "A001",
               "message": "LOGIN completed"
           }]))
   })
   ```

4. **Check IMAP client library behavior**:
   - Review `async-imap` or `imap` crate documentation
   - May require specific server capabilities
   - Check for TLS/STARTTLS requirements

5. **Consult CLAUDE.md**:
   - Read `tests/server/imap/CLAUDE.md` for known issues
   - Check if tests are supposed to use mocks or real LLM

#### Step 2: Fix DynamoDB Tests

1. **Check endpoint configuration**:
   ```rust
   // In tests/server/dynamo/e2e_aws_sdk_test.rs
   let config = aws_config::from_env()
       .endpoint_url(format!("http://127.0.0.1:{}", port))
       .region(Region::new("us-east-1"))
       .credentials_provider(Credentials::new(
           "test_key",
           "test_secret",
           None,
           None,
           "static"
       ))
       .load()
       .await;
   ```

2. **Check DynamoDB protocol implementation**:
   - DynamoDB uses JSON over HTTP with AWS signature
   - Check if `src/server/dynamo/mod.rs` handles AWS auth
   - Verify HTTP headers are parsed correctly

3. **Add mock expectations for AWS SDK**:
   ```rust
   .with_mock(|mock| {
       mock
           .on_event("http_request")
           .and_event_data_contains("headers", "X-Amz-Target")
           .respond_with_actions(json!([{
               "type": "http_response",
               "status": 200,
               "headers": {"Content-Type": "application/x-amz-json-1.0"},
               "body": "{\"TableDescription\":{\"TableName\":\"test\"}}"
           }]))
   })
   ```

4. **Test with curl first**:
   ```bash
   # Start DynamoDB server manually
   # Test with curl to verify basic HTTP response
   curl -v http://localhost:PORT \
     -H "X-Amz-Target: DynamoDB_20120810.ListTables" \
     -H "Content-Type: application/x-amz-json-1.0" \
     -d '{}'
   ```

#### Step 3: Fix PostgreSQL Tests

1. **Check PostgreSQL wire protocol**:
   - PostgreSQL uses binary wire protocol with specific message types
   - StartupMessage → AuthenticationOk → ReadyForQuery
   - Query → RowDescription → DataRow → CommandComplete

2. **Review mock expectations**:
   ```rust
   // In tests/server/postgresql/test.rs
   .with_mock(|mock| {
       mock
           .on_event("postgres_query")
           .and_event_data_contains("query", "SELECT")
           .respond_with_actions(json!([{
               "type": "postgres_row_description",
               "fields": [{"name": "id", "type": "int4"}]
           }, {
               "type": "postgres_data_row",
               "values": ["1"]
           }, {
               "type": "postgres_command_complete",
               "tag": "SELECT 1"
           }]))
   })
   ```

3. **Check connection handshake**:
   - PostgreSQL client sends StartupMessage immediately
   - Server must respond with AuthenticationOk (or challenge)
   - Must send ReadyForQuery before accepting queries

4. **Run single test**:
   ```bash
   ./cargo-isolated.sh test --no-default-features --features postgresql \
     --test server::postgresql::test test_postgresql_simple_query -- --nocapture
   ```

#### Step 4: Fix MySQL Tests

1. **Check MySQL handshake**:
   - Server sends HandshakeV10 packet on connection
   - Client responds with HandshakeResponse41
   - Server sends OK packet

2. **Review protocol implementation**:
   - Check `src/server/mysql/mod.rs`
   - Ensure handshake is sent before query processing
   - Verify result packet format

3. **Add proper mock expectations**:
   ```rust
   .with_mock(|mock| {
       mock
           .on_event("mysql_query")
           .respond_with_actions(json!([{
               "type": "mysql_resultset",
               "columns": [{"name": "id", "type": "LONG"}],
               "rows": [[1]]
           }]))
   })
   ```

### Success Criteria

- [ ] All IMAP tests pass with correct protocol flow
- [ ] DynamoDB tests connect to local server with AWS SDK
- [ ] PostgreSQL tests execute queries and receive results
- [ ] MySQL tests connect and query successfully
- [ ] All tests use mocks or document LLM call budget

### Files to Modify

**Tests**:
- `tests/server/imap/e2e_client_test.rs` - Fix mock expectations or protocol flow
- `tests/server/dynamo/e2e_aws_sdk_test.rs` - Fix endpoint and auth
- `tests/server/postgresql/test.rs` - Fix query response format
- `tests/server/mysql/test.rs` - Fix handshake and results

**Protocols** (if broken):
- `src/server/imap/mod.rs` - Fix IMAP server responses
- `src/server/dynamo/mod.rs` - Fix DynamoDB HTTP handling
- `src/server/postgresql/mod.rs` - Fix PostgreSQL wire protocol
- `src/server/mysql/mod.rs` - Fix MySQL handshake

---

## Group 4: UDP PROTOCOL MOCKS (HIGH)

**Priority**: HIGH
**Tests**: 7
**Status**: Need dynamic mock pattern for transaction ID matching
**Estimated Fix Time**: 1-2 hours

### Failed Tests

#### STUN (5 tests)
```
server::stun::e2e_test::test_stun_basic_binding_request
server::stun::e2e_test::test_stun_multiple_clients
server::stun::e2e_test::test_stun_rapid_requests
server::stun::e2e_test::test_stun_request_with_attributes
server::stun::e2e_test::test_stun_xor_mapped_address
```

#### DNS (1 test)
```
server::dns::test::test_dns_multiple_records
```

#### BOOTP (1 test)
```
server::bootp::e2e_test::test_bootp_static_assignment
```

### Root Cause Analysis

**UDP protocols require transaction ID matching**. Clients generate random IDs, but static mocks use hardcoded IDs, causing mismatches:

1. **Client sends**: STUN request with transaction_id=0x1234567890ABCDEF (random)
2. **Static mock expects**: transaction_id=0x0000000000000000 (hardcoded)
3. **Result**: Mock doesn't match, LLM called (slow) or test times out

**Solution**: Use **Dynamic Mock Pattern** (see CLAUDE.md):
```rust
.respond_with_actions_from_event(|event_data| {
    let transaction_id = event_data["transaction_id"].as_u64().unwrap();
    json!([{
        "type": "send_stun_response",
        "transaction_id": transaction_id  // ← DYNAMIC!
    }])
})
```

### Test File Locations

- `tests/server/stun/e2e_test.rs`
- `tests/server/stun/CLAUDE.md`
- `tests/server/dns/test.rs`
- `tests/server/dns/CLAUDE.md` (has comprehensive dynamic mock examples)
- `tests/server/bootp/e2e_test.rs`
- `tests/server/bootp/CLAUDE.md`

### Fix Instructions

#### Step 1: Review DNS Dynamic Mock Pattern

1. **Read the reference implementation**:
   ```bash
   cat tests/server/dns/CLAUDE.md
   ```

   Look for section: "Dynamic Mock Pattern for UDP Protocols (CRITICAL)"

2. **Study working example**:
   ```rust
   // From tests/server/dns/e2e_test.rs
   .on_event("dns_query")
   .and_event_data_contains("domain", "example.com")
   .respond_with_actions_from_event(|event_data| {
       let query_id = event_data["query_id"].as_u64().unwrap();
       json!([{
           "type": "send_dns_a_response",
           "query_id": query_id,  // ← Extracted from event
           "domain": "example.com",
           "ip": "93.184.216.34"
       }])
   })
   .expect_calls(1)
   ```

#### Step 2: Fix STUN Tests

1. **Update all STUN tests to use dynamic mocks**:
   ```rust
   // In tests/server/stun/e2e_test.rs
   // Replace static mocks with:

   .with_mock(|mock| {
       mock
           .on_event("stun_binding_request")
           .respond_with_actions_from_event(|event_data| {
               // Extract transaction ID from event
               let transaction_id = event_data["transaction_id"]
                   .as_str()
                   .unwrap_or("000000000000000000000000");

               // Return response with matching transaction ID
               json!([{
                   "type": "send_stun_binding_response",
                   "transaction_id": transaction_id,
                   "xor_mapped_address": {
                       "ip": "203.0.113.1",
                       "port": 54321
                   }
               }])
           })
           .expect_calls(1)
   })
   ```

2. **Check STUN event data structure**:
   - Review `src/server/stun/mod.rs` to see what data is emitted
   - Ensure `transaction_id` is included in event_data
   - Add if missing

3. **Run test**:
   ```bash
   ./cargo-isolated.sh test --no-default-features --features stun \
     --test server::stun::e2e_test test_stun_basic_binding_request -- --nocapture
   ```

4. **Verify mock expectations**:
   ```rust
   server.verify_mocks().await?;  // Must call!
   ```

#### Step 3: Fix DNS Multiple Records Test

1. **Check if test already uses dynamic mocks**:
   ```bash
   grep -A 20 "test_dns_multiple_records" tests/server/dns/test.rs
   ```

2. **If using static mocks, convert to dynamic**:
   ```rust
   .respond_with_actions_from_event(|event_data| {
       let query_id = event_data["query_id"].as_u64().unwrap();
       json!([{
           "type": "send_dns_a_response",
           "query_id": query_id,
           "domain": "multi.example.com",
           "ips": ["1.1.1.1", "2.2.2.2", "3.3.3.3"]
       }])
   })
   ```

3. **Test**:
   ```bash
   ./cargo-isolated.sh test --no-default-features --features dns \
     --test server::dns::test test_dns_multiple_records -- --nocapture
   ```

#### Step 4: Fix BOOTP Test

1. **BOOTP uses transaction_id (xid) for request/response matching**:
   ```rust
   // In tests/server/bootp/e2e_test.rs
   .with_mock(|mock| {
       mock
           .on_event("bootp_request")
           .respond_with_actions_from_event(|event_data| {
               let xid = event_data["xid"].as_u64().unwrap();
               json!([{
                   "type": "send_bootp_reply",
                   "xid": xid,  // ← Match client transaction ID
                   "yiaddr": "192.168.1.100",
                   "siaddr": "192.168.1.1",
                   "chaddr": event_data["chaddr"]
               }])
           })
           .expect_calls(1)
   })
   ```

2. **Test**:
   ```bash
   ./cargo-isolated.sh test --no-default-features --features bootp \
     --test server::bootp::e2e_test test_bootp_static_assignment -- --nocapture
   ```

### Success Criteria

- [ ] All STUN tests use `.respond_with_actions_from_event()`
- [ ] DNS multiple records test uses dynamic mocks
- [ ] BOOTP test extracts and matches transaction ID
- [ ] All tests call `.verify_mocks().await?`
- [ ] Tests complete in <5s each (no LLM calls)

### Files to Modify

**Tests**:
- `tests/server/stun/e2e_test.rs` - Convert all 5 tests to dynamic mocks
- `tests/server/dns/test.rs` - Fix `test_dns_multiple_records`
- `tests/server/bootp/e2e_test.rs` - Add dynamic xid matching

**Protocols** (check event_data):
- `src/server/stun/mod.rs` - Ensure transaction_id in event_data
- `src/server/bootp/mod.rs` - Ensure xid in event_data

**Documentation**:
- `tests/server/stun/CLAUDE.md` - Add dynamic mock pattern examples
- `tests/server/bootp/CLAUDE.md` - Document xid matching

---

## Group 5: APPLICATION PROTOCOLS (MEDIUM)

**Priority**: MEDIUM
**Tests**: 26
**Status**: Various mock expectations and integration issues
**Estimated Fix Time**: 4-6 hours

### Failed Tests

#### SSH (9 tests) - Server (5) + Agent (4)
```
server::ssh::test::test_sftp_basic_operations
server::ssh::test::test_ssh_banner
server::ssh::test::test_ssh_connection_attempt
server::ssh::test::test_ssh_python_auth_script
server::ssh::test::test_ssh_script_fallback_to_llm
server::ssh_agent::e2e_test::test_ssh_agent_add_identity_with_mocks
server::ssh_agent::e2e_test::test_ssh_agent_multiple_operations_with_mocks
server::ssh_agent::e2e_test::test_ssh_agent_request_identities_with_mocks
server::ssh_agent::e2e_test::test_ssh_agent_sign_request_with_mocks
```

#### SNMP (4 tests)
```
server::snmp::test::test_snmp_basic_get
server::snmp::test::test_snmp_custom_mib
server::snmp::test::test_snmp_get_next
server::snmp::test::test_snmp_interface_stats
```

#### OpenAPI (5 tests)
```
server::openapi::e2e_route_matching_test::test_openapi_llm_on_invalid_override
server::openapi::e2e_route_matching_test::test_openapi_route_matching_comprehensive
server::openapi::e2e_test::test_openapi_create_todo
server::openapi::e2e_test::test_openapi_spec_compliant_flag
server::openapi::e2e_test::test_openapi_todo_list
```

#### Git (4 tests) - Not including hanging test from Group 1
```
server::git::e2e_test::test_git_info_refs_endpoint
server::git::e2e_test::test_git_multiple_repositories
server::git::e2e_test::test_git_repository_not_found
server::git::e2e_test::test_git_with_scripting
```

#### HTTP2 (3 tests)
```
server::http2::e2e_test::test_http2_basic_get_requests
server::http2::e2e_test::test_http2_multiplexing
server::http2::e2e_test::test_http2_post_with_body
```

#### SMB (5 tests)
```
server::smb::e2e_llm_test::test_smb_llm_allows_guest_auth
server::smb::e2e_llm_test::test_smb_llm_denies_user
server::smb::e2e_test::test_smb_auth_llm_controlled
server::smb::e2e_test::test_smb_concurrent_connections
server::smb::e2e_test::test_smb_session_setup
```

#### Proxy (4 tests)
```
server::proxy::e2e_test::proxy_server_tests::test_proxy_http_block_with_mocks
server::proxy::e2e_test::proxy_server_tests::test_proxy_http_passthrough_with_mocks
server::proxy::e2e_test::proxy_server_tests::test_proxy_https_connect_with_mocks
server::proxy::e2e_test::proxy_server_tests::test_proxy_modify_headers_with_mocks
```

#### XML-RPC (5 tests)
```
server::xmlrpc::test::test_xmlrpc_boolean_parameter
server::xmlrpc::test::test_xmlrpc_fault_response
server::xmlrpc::test::test_xmlrpc_introspection_list_methods
server::xmlrpc::test::test_xmlrpc_multiple_parameters
server::xmlrpc::test::test_xmlrpc_string_parameter
```

#### XMPP (3 tests)
```
server::xmpp::test::test_xmpp_message
server::xmpp::test::test_xmpp_presence
server::xmpp::test::test_xmpp_stream_header
```

#### POP3 (4 tests)
```
server::pop3::test::test_pop3_authentication
server::pop3::test::test_pop3_greeting
server::pop3::test::test_pop3_quit
server::pop3::test::test_pop3_stat
```

#### SQS (3 tests)
```
server::sqs::e2e_test::test_sqs_basic_queue_operations
server::sqs::e2e_test::test_sqs_message_visibility
server::sqs::e2e_test::test_sqs_queue_not_found
```

### Root Cause Analysis

**SSH**: Binary protocol with handshake, likely:
- Missing key exchange mock expectations
- SFTP subsystem not responding correctly
- Python scripting tests may have script errors

**SNMP**: ASN.1/BER encoding, transaction IDs (like STUN/DNS):
- May need dynamic mocks for request_id
- OID matching issues
- Response PDU format incorrect

**OpenAPI**: Route matching and validation:
- OpenAPI spec parsing issues
- Route parameter extraction
- Spec compliance validation logic

**Git**: HTTP-based git protocol:
- `/info/refs` endpoint response format
- Pack file negotiation
- Multiple repo routing

**HTTP2**: Binary framing layer:
- HPACK header compression
- Stream multiplexing
- SETTINGS/WINDOW_UPDATE frames

**SMB**: Complex binary protocol:
- Dialect negotiation
- Session setup auth
- Tree connect flow

**Proxy**: HTTP proxy with CONNECT:
- CONNECT tunnel establishment
- Header modification
- SSL/TLS passthrough

**XML-RPC**: XML over HTTP:
- XML parsing/generation
- Method call format
- Fault responses

**XMPP**: XML streaming:
- Stream initialization `<?xml...><stream:stream>`
- Stanza handling
- Presence broadcast

**POP3**: Text protocol:
- Greeting banner "+OK"
- Command/response format
- Multi-line responses

**SQS**: AWS HTTP API:
- Similar to DynamoDB - endpoint/auth
- XML response format
- Queue URL generation

### Fix Instructions

#### General Approach for All Protocols

1. **Run single test with debugging**:
   ```bash
   ./cargo-isolated.sh test --no-default-features --features <protocol> \
     --test server::<protocol>::<test_file> <test_name> -- --nocapture
   ```

2. **Check for mocks**:
   - If test uses `.with_mock()`, verify expectations
   - Add `.debug()` to see what's being matched
   - Ensure `.verify_mocks().await?` is called

3. **Check protocol implementation**:
   - Review `src/server/<protocol>/mod.rs`
   - Ensure events are emitted correctly
   - Verify action handlers work

4. **Consult CLAUDE.md**:
   - Read `tests/server/<protocol>/CLAUDE.md`
   - Check for known issues
   - Review LLM call budget

#### SSH-Specific Instructions

1. **Check SSH handshake**:
   - Server must send banner: `SSH-2.0-NetGet_SSH\r\n`
   - Key exchange (KEX) must complete
   - Auth methods must be advertised

2. **SFTP subsystem**:
   - Check if SFTP subsystem is registered
   - Verify channel open for SFTP works
   - Test file operations (ls, get, put)

3. **Python scripting**:
   - Check script path and execution
   - Verify fallback to LLM works
   - Test both success and failure paths

#### SNMP-Specific Instructions

1. **Apply dynamic mock pattern** (like STUN/DNS):
   ```rust
   .respond_with_actions_from_event(|event_data| {
       let request_id = event_data["request_id"].as_u64().unwrap();
       json!([{
           "type": "send_snmp_response",
           "request_id": request_id,
           "varbinds": [{"oid": "1.3.6.1.2.1.1.1.0", "value": "NetGet"}]
       }])
   })
   ```

2. **Check OID handling**:
   - Ensure OIDs are parsed correctly
   - Verify MIB lookups work (if applicable)
   - Test GetNext OID walking

#### OpenAPI-Specific Instructions

1. **Check OpenAPI spec loading**:
   - Verify spec file is read correctly
   - Test route parameter extraction
   - Check validation logic

2. **Route matching**:
   - Test exact matches: `/api/users`
   - Test parameter matches: `/api/users/{id}`
   - Test wildcard matches

3. **Spec compliance**:
   - Check if `spec_compliant` flag works
   - Verify schema validation (if implemented)
   - Test error responses for invalid routes

#### HTTP2-Specific Instructions

1. **Check HTTP2 handshake**:
   - Client sends HTTP/2 preface
   - Server sends SETTINGS frame
   - SETTINGS acknowledgment

2. **Frame handling**:
   - HEADERS frame parsing
   - DATA frame handling
   - Stream multiplexing (multiple requests)

3. **Test with h2 client**:
   ```rust
   let client = hyper::Client::builder()
       .http2_only(true)
       .build_http();
   ```

### Success Criteria

- [ ] Each protocol group has at least 1 passing test
- [ ] Mock expectations match actual protocol flow
- [ ] All tests document LLM call budget or use mocks
- [ ] Tests complete in reasonable time (<30s each)

### Files to Modify

**Tests** (one section per protocol):
- `tests/server/ssh/*.rs`
- `tests/server/snmp/test.rs`
- `tests/server/openapi/*.rs`
- `tests/server/git/e2e_test.rs`
- `tests/server/http2/e2e_test.rs`
- `tests/server/smb/*.rs`
- `tests/server/proxy/e2e_test.rs`
- `tests/server/xmlrpc/test.rs`
- `tests/server/xmpp/test.rs`
- `tests/server/pop3/test.rs`
- `tests/server/sqs/e2e_test.rs`

**Protocols** (if implementation broken):
- Corresponding `src/server/<protocol>/mod.rs` files

---

## Group 6: HTTP SCHEDULED TASKS (MEDIUM)

**Priority**: MEDIUM
**Tests**: 3
**Status**: Async task execution timing issues
**Estimated Fix Time**: 1-2 hours

### Failed Tests

```
server::http::e2e_scheduled_tasks_test::test_http_with_oneshot_task
server::http::e2e_scheduled_tasks_test::test_http_with_recurring_task
server::http::e2e_scheduled_tasks_test::test_http_with_server_attached_tasks
```

### Root Cause Analysis

Scheduled tasks tests timing out or not executing suggests:
1. Task not triggering within test timeout
2. Task execution not completing before verification
3. Mock expectations for task-generated actions not matching
4. Task cleanup not happening on server shutdown

### Test File Locations

- `tests/server/http/e2e_scheduled_tasks_test.rs`
- `tests/server/http/CLAUDE.md`
- `src/server/http/mod.rs` (check task scheduling)
- `src/state/scheduled_tasks.rs` (task execution infrastructure)

### Fix Instructions

#### Step 1: Run Single Test with Debug Output

```bash
./cargo-isolated.sh test --no-default-features --features http \
  --test server::http::e2e_scheduled_tasks_test test_http_with_oneshot_task -- --nocapture
```

Look for:
- Task creation log messages
- Task execution log messages
- Timeout errors
- Mock verification failures

#### Step 2: Check Task Timing

1. **Oneshot task test**:
   ```rust
   // Task should execute after delay_secs
   "scheduled_tasks": [{
       "task_id": "oneshot",
       "recurring": false,
       "delay_secs": 2,  // ← Too long? Try 1 second
       "instruction": "Log 'task executed'"
   }]
   ```

2. **Add longer wait in test**:
   ```rust
   // After starting server
   tokio::time::sleep(Duration::from_secs(5)).await;  // Wait for task
   ```

3. **Check task execution logs**:
   ```rust
   // In src/state/scheduled_tasks.rs
   debug!("Executing scheduled task: {}", task_id);
   ```

#### Step 3: Check Mock Expectations

Scheduled tasks may generate LLM calls:
```rust
.with_mock(|mock| {
    mock
        // Server startup
        .on_instruction_containing("HTTP")
        .respond_with_actions(json!([{
            "type": "open_server",
            "port": 0,
            "base_stack": "HTTP",
            "scheduled_tasks": [{
                "task_id": "test_task",
                "recurring": false,
                "delay_secs": 1,
                "instruction": "Send HTTP response"
            }]
        }]))
        .expect_calls(1)
        .and()
        // Task execution (after delay)
        .on_instruction_containing("Send HTTP response")
        .respond_with_actions(json!([{
            "type": "http_response",
            "status": 200,
            "body": "Task executed"
        }]))
        .expect_calls(1)  // ← Might not be called if task doesn't run
})
```

#### Step 4: Check Server-Attached vs Connection-Scoped Tasks

From CLAUDE.md:
- **Server-scoped**: Tasks attached to server, cleanup on server close
- **Connection-scoped**: Tasks attached to connection, cleanup on connection close

```rust
// Server-scoped task
"scheduled_tasks": [{
    "task_id": "server_task",
    // No connection_id
}]

// Connection-scoped task (created during request handling)
"connection_id": 123,  // ← Requires active connection
```

**Issue**: Connection-scoped tasks may not have active connection in test.

**Fix**: Use server-scoped tasks for HTTP tests.

#### Step 5: Add Explicit Task Verification

```rust
// After starting server and waiting for task execution
let server_state = app_state.get_server_by_id(server_id)?;
let tasks = server_state.get_scheduled_tasks();
assert!(tasks.is_empty(), "Tasks should be cleaned up");

// Or verify task executed
let logs = server.get_logs();
assert!(logs.contains("task executed"), "Task did not execute");
```

### Success Criteria

- [ ] Oneshot task executes exactly once after delay
- [ ] Recurring task executes multiple times at intervals
- [ ] Server-attached tasks execute without active connections
- [ ] Tasks cleanup on server shutdown
- [ ] All mock expectations met

### Files to Modify

**Tests**:
- `tests/server/http/e2e_scheduled_tasks_test.rs` - Add delays, fix mock expectations

**Infrastructure** (if broken):
- `src/state/scheduled_tasks.rs` - Fix task execution timing
- `src/server/http/mod.rs` - Fix task scheduling during server startup

---

## Group 7: DATALINK (MEDIUM)

**Priority**: MEDIUM
**Tests**: 3
**Status**: Packet capture mocking issues
**Estimated Fix Time**: 1-2 hours

### Failed Tests

```
server::datalink::e2e_test::datalink_server_tests::test_datalink_arp_capture_with_mocks
server::datalink::e2e_test::datalink_server_tests::test_datalink_custom_protocol_with_mocks
server::datalink::e2e_test::datalink_server_tests::test_datalink_ignore_packet_with_mocks
```

### Root Cause Analysis

Datalink layer tests use pcap for packet capture. Issues likely:
1. Mock expectations not matching pcap events
2. Packet injection not working in test environment
3. Permissions required for raw sockets (root/CAP_NET_RAW)
4. Interface selection (may default to wrong interface)

### Test File Locations

- `tests/server/datalink/e2e_test.rs`
- `tests/server/datalink/CLAUDE.md`
- `src/server/datalink/mod.rs`

### Fix Instructions

#### Step 1: Check Permissions

```bash
# On Linux, pcap requires root or capabilities
sudo setcap cap_net_raw,cap_net_admin=eip target/debug/deps/server-*

# Or run with sudo (not recommended)
sudo ./cargo-isolated.sh test --features datalink ...
```

#### Step 2: Run Single Test

```bash
./cargo-isolated.sh test --no-default-features --features datalink \
  --test server::datalink::e2e_test test_datalink_arp_capture_with_mocks -- --nocapture
```

Look for:
- Permission denied errors
- Interface not found errors
- Packet capture timeout errors

#### Step 3: Check Mock Expectations

Datalink tests should use mocks for LLM, not actual packet capture:

```rust
.with_mock(|mock| {
    mock
        // Server startup
        .on_instruction_containing("datalink")
        .respond_with_actions(json!([{
            "type": "open_server",
            "port": 0,  // Datalink doesn't use ports
            "base_stack": "DataLink",
            "interface": "lo"  // Use loopback for testing
        }]))
        .and()
        // Packet received
        .on_event("datalink_packet")
        .and_event_data_contains("protocol", "ARP")
        .respond_with_actions(json!([{
            "type": "send_arp_reply",
            "target_ip": "192.168.1.100"
        }]))
})
```

#### Step 4: Use Loopback Interface

Tests should use loopback (`lo` or `lo0`) to avoid permissions:
```rust
let config = NetGetConfig::new(
    "Capture packets on loopback interface via DataLink"
)
```

#### Step 5: Verify Packet Injection Works

```rust
// In test, inject packet to loopback
use pnet::datalink;
let interface = datalink::interfaces()
    .into_iter()
    .find(|iface| iface.is_loopback())
    .expect("No loopback interface");

// Send test packet
let (mut tx, _rx) = datalink::channel(&interface, Default::default())?;
tx.send_to(&arp_packet, None)?;
```

### Success Criteria

- [ ] Tests run without root permissions (using loopback)
- [ ] ARP packets captured and processed
- [ ] Custom protocols recognized
- [ ] Packet filtering (ignore) works
- [ ] All mock expectations met

### Files to Modify

**Tests**:
- `tests/server/datalink/e2e_test.rs` - Use loopback, fix mocks

**Protocol** (if broken):
- `src/server/datalink/mod.rs` - Fix interface selection, packet capture

---

## Group 8: SINGLE FAILURES (LOW)

**Priority**: LOW
**Tests**: 14
**Status**: Individual protocol issues, investigate separately
**Estimated Fix Time**: 3-5 hours total

### Failed Tests (1 per protocol)

```
server::dns::test::test_dns_multiple_records
server::doh::e2e_test::test_doh_server
server::dot::e2e_test::test_dot_server
server::elasticsearch::e2e_test::tests::test_elasticsearch_bulk_operations
server::etcd::e2e_test::test_etcd_kv_operations
server::ipp::test::test_ipp_basic_http
server::kafka::e2e_test::test_kafka_broker_startup
server::mercurial::e2e_test::test_mercurial_repository_not_found
server::nntp::e2e_test::test_nntp_article_overview
server::nntp::e2e_test::test_nntp_basic_newsgroups
server::oauth2::e2e_test::test_oauth2_authorization_code_flow
server::oauth2::e2e_test::test_oauth2_token_introspection
server::rss::e2e_test::test_rss_comprehensive
server::tcp::test::test_custom_response
server::tcp::test::test_simple_echo
server::torrent_peer::e2e_test::test_peer_piece_request
server::torrent_tracker::e2e_test::test_tracker_announce_and_scrape
server::udp::test::test_udp_echo_server
server::zookeeper::e2e_test::test_zookeeper_error_response
server::zookeeper::e2e_test::test_zookeeper_get_children
server::zookeeper::e2e_test::test_zookeeper_get_data
```

### Fix Instructions

For each protocol:

1. **Run single test**:
   ```bash
   ./cargo-isolated.sh test --no-default-features --features <protocol> \
     --test server::<protocol>::<test_file> <test_name> -- --nocapture
   ```

2. **Check for common issues**:
   - Missing mock expectations
   - Transaction ID matching (UDP protocols)
   - Response format issues
   - Connection handshake problems

3. **Consult CLAUDE.md**:
   - Read protocol's test documentation
   - Check known issues
   - Review LLM call budget

4. **Fix and verify**:
   - Update test or protocol implementation
   - Re-run test
   - Document fix in test's CLAUDE.md

### Quick Notes on Specific Protocols

**TCP/UDP**: Basic protocols, likely mock expectation issues
**DNS**: Already in Group 4 (UDP mocks)
**DoH/DoT**: DNS over HTTPS/TLS, check HTTP/TLS handling
**Elasticsearch**: JSON over HTTP, check bulk operation format
**etcd**: gRPC protocol, check request/response format
**Kafka**: Binary protocol, check message format
**NNTP**: Text protocol, check article format
**OAuth2**: HTTP with redirects, check flow state
**Torrent**: BitTorrent protocol, check handshake
**ZooKeeper**: Binary protocol, check session handling

---

## Test Execution Guide

### Running Tests for a Specific Group

```bash
# Group 1: Hanging Tests
./cargo-isolated.sh test --no-default-features --features redis \
  --test server::redis::e2e_test -- --test-threads=100

./cargo-isolated.sh test --no-default-features --features pypi \
  --test server::pypi::e2e_test_mocked -- --test-threads=100

# Group 2: Bluetooth BLE
./cargo-isolated.sh test --no-default-features --features bluetooth-ble \
  --test server::bluetooth_ble::e2e_test -- --test-threads=100

# Group 3: Databases
./cargo-isolated.sh test --no-default-features --features imap,postgresql,mysql \
  --test server::imap::e2e_client_test -- --test-threads=100

# Group 4: UDP Protocols
./cargo-isolated.sh test --no-default-features --features stun,dns,bootp \
  --test server::stun::e2e_test -- --test-threads=100

# etc.
```

### Verifying Fixes

After fixing a group:
```bash
# Run all tests in that group
./cargo-isolated.sh test --no-default-features --features <protocol1>,<protocol2> \
  --no-fail-fast -- --test-threads=100

# Check that all pass
echo $?  # Should be 0
```

### Full Test Suite

Once all groups are fixed:
```bash
# Run complete test suite
./cargo-isolated.sh test --all-features --no-fail-fast -- --test-threads=100

# Should complete without hanging and with all tests passing
```

---

## Summary Checklist

### Group 1: Hanging Tests
- [ ] Redis tests complete without hanging
- [ ] PyPI tests complete without hanging
- [ ] Cassandra tests connect successfully
- [ ] Git/NPM tests timeout properly if they fail

### Group 2: Bluetooth BLE
- [ ] Identify root cause (dependencies/permissions/hardware)
- [ ] All BLE tests pass OR gracefully skip
- [ ] Document requirements in CLAUDE.md

### Group 3: Databases
- [ ] IMAP tests use correct client library
- [ ] DynamoDB endpoint and auth configured
- [ ] PostgreSQL wire protocol correct
- [ ] MySQL handshake working

### Group 4: UDP Protocols
- [ ] All STUN tests use dynamic mocks
- [ ] DNS multiple records test fixed
- [ ] BOOTP transaction ID matching

### Group 5: Application Protocols
- [ ] SSH handshake working
- [ ] SNMP uses dynamic mocks
- [ ] OpenAPI routing functional
- [ ] HTTP2 frames handled correctly

### Group 6: HTTP Scheduled Tasks
- [ ] Tasks execute within timeout
- [ ] Mock expectations account for task execution
- [ ] Tasks cleanup on shutdown

### Group 7: Datalink
- [ ] Tests use loopback interface
- [ ] Permissions handled correctly
- [ ] Packet capture working

### Group 8: Single Failures
- [ ] Each protocol investigated individually
- [ ] Root causes identified
- [ ] Fixes applied and verified

---

## Notes

- **Log File**: `./tmp/netget-test-46024.log` contains the original test run output
- **Test killed**: Tests were terminated after ~35 minutes due to hanging Redis/PyPI tests
- **Parallel execution**: All tests run with `--test-threads=100` for speed
- **Feature flags**: Use `--no-default-features --features <protocol>` to avoid building all 50+ protocols

## Contact

For questions about specific groups or protocols, consult the protocol's `CLAUDE.md` documentation in:
- `tests/server/<protocol>/CLAUDE.md` (test strategy)
- `src/server/<protocol>/CLAUDE.md` (implementation details)
