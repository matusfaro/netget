# LDAP Protocol E2E Tests

## Test Overview
Tests LDAP server with real LDAP client library (`ldap3`) validating directory operations including bind, search, add, modify, and delete.

## Test Strategy
- **Consolidated per operation** - Each test focuses on a specific LDAP operation
- **Multiple server instances** - 6 separate servers (one per test)
- **Real LDAP client** - Uses `ldap3` Rust library for protocol correctness
- **No scripting** - Action-based responses only

## LLM Call Budget
- `test_ldap_bind_success()`: 1 startup call + 1 bind operation
- `test_ldap_bind_failure()`: 1 startup call + 1 bind operation
- `test_ldap_search()`: 1 startup call + 2 operations (bind, search)
- `test_ldap_search_filter()`: 1 startup call + 2 operations (anonymous bind, search)
- `test_ldap_add_entry()`: 1 startup call + 2 operations (bind, add)
- `test_ldap_modify_entry()`: 1 startup call + 2 operations (bind, modify)
- `test_ldap_delete_entry()`: 1 startup call + 2 operations (bind, delete)
- **Total: 19 LLM calls** (7 startups + 12 operations)

**Note**: Target was <10 calls but LDAP test coverage prioritizes completeness.

## Scripting Usage
**Scripting Disabled** (`ServerConfig::new_no_scripts()`)
- LDAP protocol requires context-aware responses (authentication state, directory contents)
- Script generation not beneficial for stateful operations
- LLM interprets each operation with full session context

## Client Library
**ldap3** v0.11+ - Async LDAP client
- `LdapConnAsync::new()` - Connect to server
- `simple_bind()` - Authenticate with DN and password
- `search()` - Search directory with filters
- `add()` - Add new entry
- `modify()` - Modify existing entry
- `delete()` - Delete entry
- `unbind()` - Close connection
- Real LDAP library ensures protocol correctness

## Expected Runtime
- Model: qwen3-coder:30b
- Runtime: ~80-120 seconds for full test suite
- Moderate speed due to 19 LLM calls

## Failure Rate
- **Medium-High** (15-25%) - LLM struggles with ASN.1 BER encoding expectations
- Common issues:
  - LLM returns prose instead of LDAP response actions
  - Search response format errors (missing entries or malformed attributes)
  - Result code inconsistencies
  - LLM forgets to include `message_id` in responses

## Test Cases
1. **test_ldap_bind_success** - Tests successful bind with correct credentials
2. **test_ldap_bind_failure** - Tests bind rejection with wrong credentials
3. **test_ldap_search** - Tests search returning multiple entries with attributes
4. **test_ldap_search_filter** - Tests filtered search with specific criteria
5. **test_ldap_add_entry** - Tests adding new directory entry
6. **test_ldap_modify_entry** - Tests modifying existing entry attributes
7. **test_ldap_delete_entry** - Tests deleting directory entry

## Known Issues
- **Binary protocol complexity** - LLM must understand BER encoding indirectly
- Tests sleep 2 seconds after server start for initialization
- Some tests check result codes loosely (any success vs specific code)
- Anonymous bind test assumes LLM accepts empty DN/password
- No verification of entry persistence across operations

## Example Test Pattern
```rust
// Start server with --no-scripts flag
let server = start_netget_server(ServerConfig::new_no_scripts(prompt)).await?;
sleep(Duration::from_secs(2)).await;

// Connect to LDAP server
let ldap_url = format!("ldap://127.0.0.1:{}", server.port);
let (conn, mut ldap) = LdapConnAsync::new(&ldap_url).await?;
ldap3::drive!(conn);

// Bind (authenticate)
let bind_result = ldap.simple_bind("cn=admin,dc=example,dc=com", "secret").await?;
assert_eq!(bind_result.rc, 0, "Bind should succeed");

// Perform operation (search example)
let (rs, _res) = ldap.search(
    "dc=example,dc=com",
    Scope::Subtree,
    "(objectClass=*)",
    vec!["cn", "mail"]
).await?.success()?;

// Validate results
assert!(rs.len() >= 2, "Should find at least 2 entries");

// Unbind
ldap.unbind().await?;
server.stop().await?;
```

## LDAP Protocol Notes
- **DN format**: `cn=username,dc=example,dc=com`
- **Bind types**: Simple (username/password), anonymous (empty DN)
- **Search scopes**: Base, OneLevel, Subtree
- **Result codes**: 0 = success, 49 = invalidCredentials, 32 = noSuchObject, 68 = entryAlreadyExists
- **Object classes**: person, inetOrgPerson, organizationalUnit, etc.

## Performance Considerations
- LDAP client connection has ~1-2 second overhead
- LLM must generate BER-encoded binary responses correctly
- Test suite slower than protocols with simpler text-based formats
