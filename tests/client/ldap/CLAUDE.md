# LDAP Client E2E Testing

## Overview
Tests for the LDAP client protocol implementation. Tests use a real OpenLDAP server running in Docker and validate LLM-controlled LDAP operations (bind, search, add, modify, delete).

## Test Strategy
**Black-box, prompt-driven testing** - LLM interprets prompts and performs LDAP operations, tests validate with real OpenLDAP server.

### Test Categories
1. **Unit Tests** (no LLM/Docker) - Protocol metadata, action parsing
2. **Integration Tests** (Docker, no LLM) - Connection handling, manual action execution
3. **E2E Tests** (Docker + LLM) - Full prompt-driven workflows

## Test Infrastructure

### OpenLDAP Docker Container
```bash
docker run -d -p 1389:389 \
  -e LDAP_ORGANISATION="Example Inc" \
  -e LDAP_DOMAIN="example.com" \
  -e LDAP_ADMIN_PASSWORD="admin" \
  --name openldap \
  osixia/openldap:1.5.0
```

**Default Admin Credentials:**
- DN: `cn=admin,dc=example,dc=com`
- Password: `admin`

**Default Base DN:** `dc=example,dc=com`

### Pre-populated Entries
The osixia/openldap image creates:
- Admin user: `cn=admin,dc=example,dc=com`
- Base domain: `dc=example,dc=com`

Tests can add/modify/delete entries as needed.

## LLM Call Budget
**Target: < 10 LLM calls per test suite**

### Call Breakdown
1. **test_ldap_client_connect** - 0 LLM calls (just connection, no LLM needed)
2. **test_ldap_client_bind_and_search** - 3-4 LLM calls:
   - Connected event → LLM decides bind
   - Bind response → LLM decides search
   - Search results → LLM decides disconnect
3. **test_ldap_client_add_modify_delete** - 5-6 LLM calls:
   - Connected event → LLM decides bind
   - Bind response → LLM decides add
   - Add response → LLM decides modify
   - Modify response → LLM decides delete
   - Delete response → LLM decides disconnect

**Total: ~8-10 LLM calls** (within budget)

### Budget Optimization
- **Single client instance** - Reuse connection across operations
- **Sequential operations** - Bind once, then chain operations
- **Clear instructions** - LLM understands complete workflow from initial prompt
- **No retries** - Tests should pass on first attempt

## Test Cases

### 1. test_ldap_client_connect
**Purpose:** Verify basic LDAP connection establishment
**LLM Calls:** 0
**Duration:** < 1 second
**Assertions:**
- Client connects to localhost:1389
- Client status becomes Connected

### 2. test_ldap_client_bind_and_search
**Purpose:** Test authentication and search operations
**LLM Calls:** 3-4
**Duration:** 5-10 seconds
**Workflow:**
1. Connect to LDAP
2. LLM performs bind with admin credentials
3. LLM searches for all entries under base DN
4. Verify search results received

**Expected LLM Actions:**
```json
{"type": "bind", "dn": "cn=admin,dc=example,dc=com", "password": "admin"}
{"type": "search", "base_dn": "dc=example,dc=com", "filter": "(objectClass=*)", "scope": "subtree"}
{"type": "disconnect"}
```

### 3. test_ldap_client_add_modify_delete
**Purpose:** Test full CRUD operations on LDAP entries
**LLM Calls:** 5-6
**Duration:** 10-15 seconds
**Workflow:**
1. Connect and bind
2. Add new entry: `cn=testuser,dc=example,dc=com`
3. Modify entry: update mail attribute
4. Delete entry
5. Verify no errors

**Expected LLM Actions:**
```json
{"type": "bind", "dn": "cn=admin,dc=example,dc=com", "password": "admin"}
{"type": "add", "dn": "cn=testuser,dc=example,dc=com", "attributes": {...}}
{"type": "modify", "dn": "cn=testuser,dc=example,dc=com", "operation": "replace", ...}
{"type": "delete", "dn": "cn=testuser,dc=example,dc=com"}
{"type": "disconnect"}
```

### 4. test_ldap_client_protocol_metadata (Unit)
**Purpose:** Validate protocol metadata
**LLM Calls:** 0
**Duration:** < 100ms
**Assertions:**
- Protocol name is "LDAP"
- Stack name is "ETH>IP>TCP>LDAP"
- Keywords include "ldap", "ldap client"
- Description and example prompt exist

### 5. test_ldap_client_actions (Unit)
**Purpose:** Validate action definitions
**LLM Calls:** 0
**Duration:** < 100ms
**Assertions:**
- All essential actions defined (bind, search, add, modify, delete, disconnect)
- Each action has example JSON
- Parameters are documented

### 6. test_ldap_client_execute_*_action (Unit)
**Purpose:** Test action parsing and execution
**LLM Calls:** 0
**Duration:** < 100ms each
**Assertions:**
- Bind action parsed correctly
- Search action parsed correctly
- Add/modify/delete actions parsed correctly

## Expected Runtime
- **Unit tests:** < 1 second total
- **Integration tests (ignored):** ~1 second (requires Docker)
- **E2E tests (ignored):** ~15-25 seconds total (requires Docker + Ollama)
- **Full suite (with Docker + Ollama):** ~30 seconds

## Running Tests

### Unit Tests Only (Fast)
```bash
./cargo-isolated.sh test --no-default-features --features ldap --test client::ldap::e2e_test -- --skip ignored
```

### All Tests (Requires Docker + Ollama)
```bash
# Start OpenLDAP
docker run -d -p 1389:389 \
  -e LDAP_ORGANISATION="Example Inc" \
  -e LDAP_DOMAIN="example.com" \
  -e LDAP_ADMIN_PASSWORD="admin" \
  --name openldap \
  osixia/openldap:1.5.0

# Run tests
./cargo-isolated.sh test --no-default-features --features ldap --test client::ldap::e2e_test -- --include-ignored

# Cleanup
docker stop openldap && docker rm openldap
```

## Known Issues
- **Timing-sensitive:** E2E tests use fixed sleep durations, may fail on slow systems
- **LLM variability:** LLM may generate unexpected actions, tests designed to be robust
- **Docker dependency:** Integration/E2E tests require Docker, marked with `#[ignore]`
- **Ollama dependency:** E2E tests require Ollama with a working model
- **No cleanup:** Tests don't clean up added LDAP entries (fresh container per run recommended)

## Test Data

### Valid LDAP Filters
- `(objectClass=*)` - All entries
- `(objectClass=person)` - All person entries
- `(cn=admin)` - Specific entry by CN
- `(&(objectClass=person)(mail=*))` - All persons with mail attribute

### Sample Entry for Add
```json
{
  "dn": "cn=testuser,dc=example,dc=com",
  "attributes": {
    "objectClass": ["person", "inetOrgPerson"],
    "cn": ["testuser"],
    "sn": ["User"],
    "mail": ["testuser@example.com"]
  }
}
```

## Debugging
Enable LDAP client logging:
```bash
RUST_LOG=netget::client::ldap=trace ./cargo-isolated.sh test ...
```

View OpenLDAP logs:
```bash
docker logs openldap
```

## Success Criteria
- ✅ All unit tests pass
- ✅ Connection test passes (with Docker)
- ✅ Bind and search test passes (with Docker + Ollama)
- ✅ Add/modify/delete test passes (with Docker + Ollama)
- ✅ Total LLM calls < 10
- ✅ Total E2E runtime < 30 seconds
- ✅ No flaky tests (should pass consistently)

## References
- RFC 4511 - LDAP Protocol Specification
- osixia/openldap Docker image: https://github.com/osixia/docker-openldap
- ldap3 crate documentation: https://docs.rs/ldap3/
