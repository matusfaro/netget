# LDAP Client Protocol Implementation

## Overview

LDAP (Lightweight Directory Access Protocol) client for connecting to LDAP directory servers and performing directory
operations. Supports bind (authentication), search, add, modify, and delete operations with full LLM control.

## Library Choices

- **ldap3 v0.11** - Mature Rust LDAP client library
- ASN.1/BER encoding handled by ldap3 library
- Synchronous API wrapped with tokio::task::spawn_blocking for async compatibility
- Chosen for protocol compliance, ease of use, and mature implementation

## Architecture Decisions

### Connection Model

LDAP connections are stateful and persistent:

- **LdapConn** - Synchronous connection wrapped in Arc<Mutex> for thread-safety
- **Blocking operations** - All LDAP operations (bind, search, add, modify, delete) use spawn_blocking to avoid blocking
  async runtime
- **No reconnection** - Connection persists until explicit disconnect or error
- **Single connection per client** - Each client instance manages one LDAP connection

### LLM Integration

- **Event-driven** - LDAP operations trigger events that call LLM for next action
- **Five event types**:
    - `LDAP_CLIENT_CONNECTED_EVENT` - Initial connection established
    - `LDAP_CLIENT_BIND_RESPONSE_EVENT` - Bind (authentication) response
    - `LDAP_CLIENT_SEARCH_RESULTS_EVENT` - Search results received
    - `LDAP_CLIENT_MODIFY_RESPONSE_EVENT` - Add/modify/delete response
- **Action-based operations** - LLM returns JSON actions for directory operations

### Authentication

- **Simple bind** - Username/password authentication (DN + password)
- **Anonymous bind** - Empty credentials (not implemented but supported by ldap3)
- **No SASL** - SASL authentication not implemented
- **Bind state** - Authentication state managed by ldap3 library

### Search Operations

- **Scope control** - Base, OneLevel, or Subtree scope
- **Filter syntax** - Standard LDAP filter syntax (e.g., "(objectClass=person)", "(cn=john)")
- **Attribute selection** - Specify attributes to retrieve or use "*" for all
- **Result parsing** - Search results converted to JSON with DN and attributes

### Modify Operations

Three modification types:

- **Add** - Create new LDAP entry with DN and attributes
- **Modify** - Modify existing entry (add/delete/replace attribute values)
- **Delete** - Delete existing entry by DN

## Response Actions

The LLM controls LDAP client behavior through these actions:

- `bind` - Authenticate with DN and password
- `search` - Search directory with base DN, filter, attributes, scope
- `add` - Add new entry with DN and attributes
- `modify` - Modify entry with operation (add/delete/replace), attribute, values
- `delete` - Delete entry by DN
- `disconnect` - Close LDAP connection
- `wait_for_more` - Wait for more LLM actions (used in async responses)

## Event Flow

### Typical Workflow

1. **Connect** → LDAP_CLIENT_CONNECTED_EVENT
2. **LLM decides** → bind action
3. **Bind** → LDAP_CLIENT_BIND_RESPONSE_EVENT
4. **LLM decides** → search action
5. **Search** → LDAP_CLIENT_SEARCH_RESULTS_EVENT
6. **LLM decides** → add/modify/delete actions or disconnect
7. **Modify** → LDAP_CLIENT_MODIFY_RESPONSE_EVENT
8. **LLM decides** → disconnect action

### Example Interaction

```
User: "Connect to LDAP at localhost:389, bind as cn=admin,dc=example,dc=com with password 'secret' and search for all users"

1. LDAP client connects → Event: ldap_connected
2. LLM returns: {"type": "bind", "dn": "cn=admin,dc=example,dc=com", "password": "secret"}
3. Client performs bind → Event: ldap_bind_response (success)
4. LLM returns: {"type": "search", "base_dn": "dc=example,dc=com", "filter": "(objectClass=person)", "scope": "subtree"}
5. Client performs search → Event: ldap_search_results (entries)
6. LLM returns: {"type": "disconnect"}
7. Client disconnects
```

## Data Structures

### Search Result Entry

```json
{
  "dn": "cn=john,dc=example,dc=com",
  "attributes": {
    "cn": ["john"],
    "mail": ["john@example.com"],
    "objectClass": ["person", "inetOrgPerson"]
  }
}
```

### Add Entry Attributes

```json
{
  "objectClass": ["person", "inetOrgPerson"],
  "cn": ["newuser"],
  "sn": ["User"],
  "mail": ["newuser@example.com"]
}
```

### Modify Operation

```json
{
  "type": "modify",
  "dn": "cn=user,dc=example,dc=com",
  "operation": "replace",
  "attribute": "mail",
  "values": ["newemail@example.com"]
}
```

## State Management

- **Connection state** - Tracked in AppState as ClientStatus (Connected/Disconnected/Error)
- **Authentication state** - Managed internally by ldap3 library
- **Memory** - LLM conversation memory stored per client in AppState
- **No local cache** - Directory data not cached, always fetched from server

## Limitations

- **No LDAPS** - TLS encryption not implemented (plain LDAP only)
- **No SASL** - Only simple bind authentication supported
- **No StartTLS** - Cannot upgrade plain connection to TLS
- **Synchronous library** - ldap3 is synchronous, wrapped with spawn_blocking
- **No paging** - Large search results not paged (could exhaust memory)
- **No referrals** - LDAP referrals not followed
- **No schema introspection** - LLM must know schema/objectClasses
- **No binary attributes** - Binary attributes (photos, certificates) not handled
- **No connection pooling** - Each client instance creates new connection

## Performance Considerations

- **Blocking operations** - All LDAP ops use spawn_blocking, may create thread pressure
- **No async** - ldap3 library is synchronous, not optimal for high concurrency
- **Search size** - Large searches (1000+ entries) can be slow, no streaming
- **Memory usage** - Search results loaded entirely into memory

## Error Handling

- **Bind errors** - Invalid credentials return bind response with success=false
- **Search errors** - Invalid filter/DN returns error via anyhow::Result
- **Modify errors** - Entry not found, constraint violations return response with success=false
- **Connection errors** - Network errors propagate to ClientStatus::Error
- **LLM errors** - Logged and reported to status_tx, don't crash client

## Security Considerations

- **Plain text** - Credentials sent in plain text (no TLS)
- **No certificate validation** - N/A for plain LDAP
- **Password exposure** - Passwords in LLM action JSON (logged in debug mode)
- **Directory exposure** - LLM can search entire directory (no access control)

## Example Prompts

### Basic Authentication

```
Connect to LDAP at ldap.example.com:389, bind as cn=admin,dc=example,dc=com with password 'adminpass'
```

### Search Users

```
Connect to LDAP at localhost:389, bind as cn=readonly,dc=corp,dc=com and search for all users with mail attribute
```

### Add Entry

```
Connect to LDAP at localhost:389, bind as cn=admin,dc=example,dc=com and add user cn=bob,ou=users,dc=example,dc=com with mail bob@example.com
```

### Modify Entry

```
Connect to LDAP at localhost:389, bind as admin and change mail for cn=alice,dc=example,dc=com to alice.new@example.com
```

## Testing

See `tests/client/ldap/CLAUDE.md` for E2E testing strategy.

## References

- RFC 4511 - LDAP: The Protocol
- RFC 4510 - LDAP: Technical Specification Road Map
- ldap3 crate documentation: https://docs.rs/ldap3/
