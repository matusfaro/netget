# Remaining Compilation Fixes - Post Mock Migration

**Total Errors**: 59 compilation errors across 5 groups
**Strategy**: Fix in parallel with separate Claude instances

## Summary of Error Groups

| Group | Description | Files Affected | Errors | Est. Time |
|-------|-------------|----------------|--------|-----------|
| 1 | Missing `retry` imports | 4 files | 20 | 5 min |
| 2 | Missing `wait_for_server_startup` imports | 3 files | 16 | 5 min |
| 3 | Missing `NetGetConfig` imports | 1 file | 5 | 3 min |
| 4 | IPSec tests with missing `output` variable | 1 file | 5 | 5 min |
| 5 | Tor/Torrent tests with missing `NetGetServer` type | 3 files | 7 | 5 min |

---

## GROUP 1: Add Missing `retry` Imports (20 errors)

**Estimated time**: 5 minutes
**Impact**: Fixes DynamoDB, Elasticsearch, and S3 test compilation

### Problem
Tests are calling `retry()` function but missing the import statement. The function exists in `tests/helpers/common.rs` and is now exported via `tests/helpers/mod.rs`.

### Affected Files
1. `tests/server/dynamo/e2e_aws_sdk_test.rs` - 8 errors
2. `tests/server/dynamo/e2e_test.rs` - 5 errors
3. `tests/server/elasticsearch/e2e_test.rs` - 7 errors
4. `tests/server/s3/e2e_test.rs` - 5 errors (lines 153, 175, 195, 300, 387)

### Fix Instructions

**Option 1: Add import at file level**

For each affected file, add to the imports section:

```rust
use crate::helpers::retry;
```

**Option 2: Use fully qualified path**

Change all `retry(` calls to `crate::helpers::retry(`:

```bash
# For each file
sed -i '' 's/retry(/crate::helpers::retry(/g' tests/server/dynamo/e2e_aws_sdk_test.rs
sed -i '' 's/retry(/crate::helpers::retry(/g' tests/server/dynamo/e2e_test.rs
sed -i '' 's/retry(/crate::helpers::retry(/g' tests/server/elasticsearch/e2e_test.rs
sed -i '' 's/retry(/crate::helpers::retry(/g' tests/server/s3/e2e_test.rs
```

**Recommended**: Use Option 1 (cleaner code)

### Success Criteria
- All `error[E0425]: cannot find function 'retry'` errors resolved
- Tests compile successfully

---

## GROUP 2: Add Missing `wait_for_server_startup` Imports (16 errors)

**Estimated time**: 5 minutes
**Impact**: Fixes IMAP, Kafka, and OpenAPI test compilation

### Problem
Tests are calling `wait_for_server_startup()` function but missing the import. Function exists in `tests/helpers/server.rs` and is now exported.

### Affected Files
1. `tests/server/imap/test.rs` - 10 errors (lines 92, 164, 252, 321, 400, 498, 615, 725, 812, 897, 982)
2. `tests/server/kafka/e2e_test.rs` - 3 errors (lines 54, 102, 153)
3. `tests/server/openapi/e2e_route_matching_test.rs` - 2 errors (lines 61, 249)

### Fix Instructions

**Add import to each file:**

```rust
use crate::helpers::wait_for_server_startup;
```

Or use sed:

```bash
# Add import after existing use statements
sed -i '' '8a\
use crate::helpers::wait_for_server_startup;
' tests/server/imap/test.rs

sed -i '' '10a\
use crate::helpers::wait_for_server_startup;
' tests/server/kafka/e2e_test.rs

sed -i '' '1a\
use crate::helpers::wait_for_server_startup;
' tests/server/openapi/e2e_route_matching_test.rs
```

### Success Criteria
- All `error[E0425]: cannot find function 'wait_for_server_startup'` errors resolved
- Tests compile successfully

---

## GROUP 3: Add Missing `NetGetConfig` Imports (5 errors)

**Estimated time**: 3 minutes
**Impact**: Fixes gRPC test compilation

### Problem
gRPC tests reference `NetGetConfig` but don't import it. The type is exported from `tests/helpers/mod.rs`.

### Affected Files
1. `tests/server/grpc/e2e_test.rs` - 5 errors (lines 111, 250, 323, 395, 518)

### Fix Instructions

**Add import at top of file:**

```rust
use crate::helpers::NetGetConfig;
```

Location: After line 8 (after existing use statements)

```bash
sed -i '' '8a\
use crate::helpers::NetGetConfig;
' tests/server/grpc/e2e_test.rs
```

### Success Criteria
- All `error[E0433]: failed to resolve: use of undeclared type 'NetGetConfig'` errors resolved
- gRPC tests compile successfully

---

## GROUP 4: Fix IPSec Tests with Missing `output` Variable (5 errors)

**Estimated time**: 5 minutes
**Impact**: Fixes IPSec honeypot test compilation

### Problem
The `get_server_output()` calls were commented out, leaving dangling references to `output` variable. Need to either:
1. Restore the `get_server_output()` calls, OR
2. Remove the output usage entirely

### Affected Files
1. `tests/server/ipsec/e2e_test.rs` - 5 errors (lines 74, 140, 203, 271, and 2 more in helper functions)

### Context
The commented lines look like:
```rust
// REMOVED: get_server_output call
let output_str = output.join("\n");
```

And in helper functions:
```rust
// REMOVED: get_server_output call
for line in &output {
```

### Fix Instructions

**Option 1: Restore get_server_output calls**

Find all `// REMOVED: get_server_output` comments and uncomment/restore:

```rust
// Before (BROKEN):
// REMOVED: get_server_output call
let output_str = output.join("\n");

// After (FIXED):
let output = get_server_output(&server).await;
let output_str = output.join("\n");
```

**Option 2: Remove output usage**

If `get_server_output` is no longer available/needed, remove the output-dependent code:

```rust
// Remove these lines:
let output = get_server_output(&server).await;
let output_str = output.join("\n");
assert!(output_str.contains("IPSec") || output_str.contains("IKE"));

// Replace with simpler assertion or remove test
```

**Recommended**: Check if `get_server_output` still exists in helpers. If yes, use Option 1. If no, use Option 2.

Check with:
```bash
grep -rn "pub.*fn get_server_output" tests/helpers/
```

### Success Criteria
- All `error[E0425]: cannot find value 'output'` errors resolved in ipsec tests
- Tests compile successfully

---

## GROUP 5: Fix Tor/Torrent Tests with Missing `NetGetServer` Type (7 errors)

**Estimated time**: 5 minutes
**Impact**: Fixes Tor integration and Torrent integration test compilation

### Problem
Integration test helper files reference `helpers::NetGetServer` but the type isn't in scope. Need to import from helpers.

### Affected Files
1. `tests/server/tor_integration/helpers.rs` - 4 errors (lines 37, 38, 183, 229)
2. `tests/server/tor_relay/e2e_test.rs` - 1 error (line 76)
3. `tests/server/torrent_integration/helpers.rs` - 3 errors (lines 10, 11, 12)

### Context

Current broken code:
```rust
pub relay: helpers::NetGetServer,
pub directory: helpers::NetGetServer,

async fn extract_relay_keys(server: &helpers::NetGetServer) -> Result<RelayKeys> {
```

### Fix Instructions

**Option 1: Import NetGetServer from crate::helpers**

Add to imports section of each file:

```rust
use crate::helpers::NetGetServer;
```

Then change references from `helpers::NetGetServer` to `NetGetServer`:

```bash
# tor_integration/helpers.rs
sed -i '' '3a\
use crate::helpers::NetGetServer;
' tests/server/tor_integration/helpers.rs
sed -i '' 's/helpers::NetGetServer/NetGetServer/g' tests/server/tor_integration/helpers.rs

# tor_relay/e2e_test.rs
sed -i '' '12a\
use crate::helpers::NetGetServer;
' tests/server/tor_relay/e2e_test.rs
sed -i '' 's/helpers::NetGetServer/NetGetServer/g' tests/server/tor_relay/e2e_test.rs

# torrent_integration/helpers.rs
sed -i '' '3a\
use crate::helpers::NetGetServer;
' tests/server/torrent_integration/helpers.rs
sed -i '' 's/helpers::NetGetServer/NetGetServer/g' tests/server/torrent_integration/helpers.rs
```

**Option 2: Use server::NetGetServer (if that's where it's defined)**

Alternative if NetGetServer is in server module:

```rust
use crate::helpers::server::NetGetServer;
```

### Success Criteria
- All `error[E0412]: cannot find type 'NetGetServer' in module 'helpers'` errors resolved
- Tor and Torrent integration tests compile successfully

---

## Execution Plan

### Sequential Approach (Single Instance)
If fixing with one Claude instance, execute groups in order:
1. GROUP 3 (fastest, 1 file)
2. GROUP 1 (4 files, similar pattern)
3. GROUP 2 (3 files, similar pattern)
4. GROUP 5 (3 files, import + replace)
5. GROUP 4 (most complex, may require investigation)

**Total time**: ~25 minutes

### Parallel Approach (5 Instances)
Spawn 5 Claude instances, each handling one group:
- Instance 1: GROUP 1
- Instance 2: GROUP 2
- Instance 3: GROUP 3
- Instance 4: GROUP 4
- Instance 5: GROUP 5

**Total time**: ~5-7 minutes (limited by slowest group)

### After All Fixes
Run compilation to verify:
```bash
./cargo-isolated.sh test --all-features --no-fail-fast -- --test-threads=100 2>&1 | tee tmp/e2e_test_after_fixes.log
```

Check for remaining errors:
```bash
grep "^error\[E" tmp/e2e_test_after_fixes.log | wc -l
```

---

## Important Notes

1. **DO NOT RUN TESTS** - Only fix compilation errors. Running tests will take 30+ minutes.

2. **Git Status** - All files are already modified from previous fixes. These are additional fixes to the same files.

3. **Verification** - After fixing, only verify compilation succeeds:
   ```bash
   ./cargo-isolated.sh build --all-features 2>&1 | grep "Finished"
   ```

4. **Conflicts** - Groups are independent and can be fixed in any order without conflicts.

5. **Sed Commands** - Test sed commands work on macOS (BSD sed with `-i ''` syntax). Linux users need `-i` without quotes.

## Success Criteria for All Groups

✅ Zero compilation errors (`grep "^error\[E" log_file | wc -l` returns 0)
✅ Build finishes successfully
✅ No new errors introduced
✅ All groups independently complete

---

## Quick Reference

### Error Counts by File
```
tests/server/dynamo/e2e_aws_sdk_test.rs: 8 errors (GROUP 1)
tests/server/dynamo/e2e_test.rs: 5 errors (GROUP 1)
tests/server/elasticsearch/e2e_test.rs: 7 errors (GROUP 1)
tests/server/s3/e2e_test.rs: 5 errors (GROUP 1)
tests/server/imap/test.rs: 10 errors (GROUP 2)
tests/server/kafka/e2e_test.rs: 3 errors (GROUP 2)
tests/server/openapi/e2e_route_matching_test.rs: 2 errors (GROUP 2)
tests/server/grpc/e2e_test.rs: 5 errors (GROUP 3)
tests/server/ipsec/e2e_test.rs: 5 errors (GROUP 4)
tests/server/tor_integration/helpers.rs: 4 errors (GROUP 5)
tests/server/tor_relay/e2e_test.rs: 1 error (GROUP 5)
tests/server/torrent_integration/helpers.rs: 3 errors (GROUP 5)
```

### Automated Fix Script (All Groups)

**WARNING**: Test before running on entire codebase!

```bash
#!/bin/bash
# Fix all compilation errors automatically

# GROUP 1: retry imports
for file in tests/server/dynamo/e2e_aws_sdk_test.rs tests/server/dynamo/e2e_test.rs tests/server/elasticsearch/e2e_test.rs tests/server/s3/e2e_test.rs; do
    sed -i '' 's/retry(/crate::helpers::retry(/g' "$file"
done

# GROUP 2: wait_for_server_startup imports
sed -i '' '8a\
use crate::helpers::wait_for_server_startup;
' tests/server/imap/test.rs

sed -i '' '10a\
use crate::helpers::wait_for_server_startup;
' tests/server/kafka/e2e_test.rs

sed -i '' '1a\
use crate::helpers::wait_for_server_startup;
' tests/server/openapi/e2e_route_matching_test.rs

# GROUP 3: NetGetConfig import
sed -i '' '8a\
use crate::helpers::NetGetConfig;
' tests/server/grpc/e2e_test.rs

# GROUP 4: Skip (needs manual investigation)

# GROUP 5: NetGetServer imports
for file in tests/server/tor_integration/helpers.rs tests/server/tor_relay/e2e_test.rs tests/server/torrent_integration/helpers.rs; do
    # Add import (adjust line number based on existing imports)
    sed -i '' '3a\
use crate::helpers::NetGetServer;
' "$file"
    # Replace references
    sed -i '' 's/helpers::NetGetServer/NetGetServer/g' "$file"
done

echo "Fixes applied. Run compilation test."
```
