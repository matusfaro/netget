# Fix Compilation Errors - ServerConfig Import Issues

## Problem
30+ test files are failing to compile because they're importing `ServerConfig` which was renamed to `NetGetConfig` during the mock migration.

**Error Pattern:**
```
error[E0432]: unresolved import `...::helpers::ServerConfig`
no `ServerConfig` in `helpers`
help: a similar name exists in the module: `NetGetConfig`
```

## Single Fix Group: Replace ServerConfig with NetGetConfig

**All affected files (30+ files):**

```bash
tests/server/datalink/test.rs
tests/server/dns/test.rs
tests/server/git/e2e_test.rs
tests/server/http/test.rs
tests/server/http/e2e_scheduled_tasks_test.rs
tests/server/ipp/test.rs
tests/server/maven/e2e_test.rs
tests/server/mdns/test.rs
tests/server/mysql/test.rs
tests/server/nfs/test.rs
tests/server/openai/test.rs
tests/server/proxy/test.rs
tests/server/redis/test.rs
tests/server/snmp/test.rs
tests/server/socket_file/test.rs
tests/server/ssh/test.rs
tests/server/tcp/test.rs
tests/server/telnet/test.rs
tests/server/udp/test.rs
tests/server/xmlrpc/test.rs
tests/server/xmpp/test.rs
tests/server/bgp/test.rs
tests/server/dynamo/e2e_aws_sdk_test.rs
tests/server/dynamo/e2e_test.rs
tests/server/elasticsearch/e2e_test.rs
tests/server/etcd/e2e_test.rs
tests/server/imap/test.rs
tests/server/jsonrpc/e2e_test.rs
tests/server/kafka/e2e_test.rs
tests/server/ldap/e2e_test.rs
tests/server/mcp/e2e_test.rs
tests/server/mongodb/e2e_test.rs
tests/server/mqtt/test.rs
tests/server/npm/e2e_test.rs
tests/server/ntp/test.rs
tests/server/ollama/test.rs
tests/server/ooxml/e2e_test.rs
tests/server/postgresql/test.rs
tests/server/s3/e2e_test.rs
tests/server/socks5/test.rs
tests/server/stun/test.rs
tests/server/tftp/test.rs
tests/server/torrent/e2e_test.rs
tests/server/turn/test.rs
tests/server/wireguard/test.rs
```

## Fix Instructions

**Option 1: Automated fix (fastest)**
```bash
# Replace ServerConfig with NetGetConfig in all test files
find tests/server -name "*.rs" -type f -exec sed -i '' 's/ServerConfig/NetGetConfig/g' {} +

# Also remove unused imports that were deleted:
# - Remove `retry` import
# - Remove `wait_for_server_startup` import
# - Remove `assert_stack_name` import
# - Remove `get_server_output` import

# These were removed from helpers/mod.rs so tests shouldn't import them
```

**Option 2: Manual fix pattern**
```rust
// BEFORE
use super::super::super::helpers::{self, E2EResult, ServerConfig};
// or
use crate::server::helpers::{start_netget_server, E2EResult, ServerConfig};

// AFTER
use super::super::super::helpers::{self, E2EResult, NetGetConfig};
// or
use crate::server::helpers::{start_netget_server, E2EResult, NetGetConfig};
```

**Additional fixes needed for specific files:**

**dynamo/elasticsearch tests (remove `retry` import):**
```rust
// BEFORE
use crate::server::helpers::{retry, start_netget_server, E2EResult, ServerConfig};

// AFTER
use crate::server::helpers::{start_netget_server, E2EResult, NetGetConfig};
// Then find another retry implementation or remove retry usage
```

**etcd test (remove `assert_stack_name` import):**
```rust
// BEFORE
use super::super::helpers::{assert_stack_name, start_netget_server, ServerConfig};

// AFTER
use super::super::helpers::{start_netget_server, NetGetConfig};
// Remove usages of assert_stack_name() in test
```

**imap/kafka tests (remove `wait_for_server_startup` import):**
```rust
// BEFORE
use crate::server::helpers::{start_netget_server, wait_for_server_startup, ServerConfig};

// AFTER
use crate::server::helpers::{start_netget_server, NetGetConfig};
// Remove usages of wait_for_server_startup() or implement locally
```

## Automated Fix Script

```bash
# Run this command to fix all ServerConfig imports
find tests/server -name "*.rs" -type f -exec sed -i '' 's/ServerConfig/NetGetConfig/g' {} +

# Verify no more ServerConfig references in tests
grep -r "ServerConfig" tests/server/ --include="*.rs"
```

## Success Criteria
- All `error[E0432]: unresolved import ServerConfig` errors resolved
- Code compiles successfully
- No references to `ServerConfig` remain in test files

## DO NOT RUN TESTS
Just fix the imports and verify compilation succeeds.
