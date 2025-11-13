# Parallel Fix Prompts - Copy Each to Separate Claude Instance

**Context**: After mock migration, 59 compilation errors remain. Each group below can be fixed independently in parallel.

**IMPORTANT**: Do NOT run tests. Only fix compilation errors and verify with `./cargo-isolated.sh build --all-features`.

---

## PROMPT FOR GROUP 1 (Give to Claude Instance #1)

```
Fix missing retry function imports in DynamoDB, Elasticsearch, and S3 tests.

Task:
Add `use crate::helpers::retry;` import to these 4 files:
1. tests/server/dynamo/e2e_aws_sdk_test.rs
2. tests/server/dynamo/e2e_test.rs
3. tests/server/elasticsearch/e2e_test.rs
4. tests/server/s3/e2e_test.rs

The retry function exists in helpers and is exported, but these files are missing the import.

Alternative: You can use fully qualified path `crate::helpers::retry(` instead of adding imports.

Verify: Build succeeds with `./cargo-isolated.sh build --all-features 2>&1 | tail -20`

Do NOT run tests.
```

---

## PROMPT FOR GROUP 2 (Give to Claude Instance #2)

```
Fix missing wait_for_server_startup function imports in IMAP, Kafka, and OpenAPI tests.

Task:
Add `use crate::helpers::wait_for_server_startup;` import to these 3 files:
1. tests/server/imap/test.rs (10 calls to this function)
2. tests/server/kafka/e2e_test.rs (3 calls)
3. tests/server/openapi/e2e_route_matching_test.rs (2 calls)

The function exists in helpers and is exported, but these files are missing the import.

Verify: Build succeeds with `./cargo-isolated.sh build --all-features 2>&1 | tail -20`

Do NOT run tests.
```

---

## PROMPT FOR GROUP 3 (Give to Claude Instance #3)

```
Fix missing NetGetConfig import in gRPC tests.

Task:
Add `use crate::helpers::NetGetConfig;` to:
- tests/server/grpc/e2e_test.rs (5 usages of NetGetConfig at lines 111, 250, 323, 395, 518)

The type is exported from helpers but this file is missing the import.

Verify: Build succeeds with `./cargo-isolated.sh build --all-features 2>&1 | tail -20`

Do NOT run tests.
```

---

## PROMPT FOR GROUP 4 (Give to Claude Instance #4)

```
Fix IPSec tests with missing output variable.

Problem: File tests/server/ipsec/e2e_test.rs has commented-out get_server_output() calls but still references the `output` variable, causing compilation errors.

Task:
1. Check if `get_server_output` function exists:
   grep -rn "pub.*fn get_server_output" tests/helpers/

2. If it EXISTS: Uncomment the get_server_output calls (search for "// REMOVED: get_server_output")

3. If it DOES NOT exist: Remove all code that references the `output` variable (around lines 74, 140, 203, 271)

Affected lines:
- Line 74: let output_str = output.join("\n");
- Line 140: let output_str = output.join("\n");
- Line 203: let output_str = output.join("\n");
- Line 271: let output_str = output.join("\n");
- Lines 188, 236: for line in &output {

Verify: Build succeeds with `./cargo-isolated.sh build --all-features 2>&1 | tail -20`

Do NOT run tests.
```

---

## PROMPT FOR GROUP 5 (Give to Claude Instance #5)

```
Fix Tor and Torrent integration tests with missing NetGetServer type.

Task:
For these 3 files, add import and fix type references:

1. tests/server/tor_integration/helpers.rs
2. tests/server/tor_relay/e2e_test.rs
3. tests/server/torrent_integration/helpers.rs

Steps for each file:
1. Add import: `use crate::helpers::NetGetServer;`
2. Replace `helpers::NetGetServer` with `NetGetServer` (remove the `helpers::` prefix)

Example:
// Before:
pub relay: helpers::NetGetServer,

// After:
use crate::helpers::NetGetServer;
pub relay: NetGetServer,

Verify: Build succeeds with `./cargo-isolated.sh build --all-features 2>&1 | tail -20`

Do NOT run tests.
```

---

## After All Groups Complete

Once all 5 Claude instances report success, verify the full build:

```bash
./cargo-isolated.sh build --all-features 2>&1 | tee tmp/build_after_all_fixes.log
grep "^error\[E" tmp/build_after_all_fixes.log | wc -l
# Should output: 0
```

If build succeeds (0 errors), you can then run the full test suite:

```bash
./cargo-isolated.sh test --all-features --no-fail-fast -- --test-threads=100 2>&1 | tee tmp/e2e_test_final_run.log
```

---

## Execution Timeline

**Parallel (5 instances)**: ~5-7 minutes total
**Sequential (1 instance)**: ~25 minutes total

Choose parallel execution for fastest results.
