# LDAP Protocol Implementation

## Overview
LDAP (Lightweight Directory Access Protocol) server implementing basic LDAPv3 functionality for directory operations. Supports bind (authentication), search, add, modify, delete, and unbind operations.

## Library Choices
- **Manual Implementation** - No external LDAP library used
- **Manual ASN.1 BER encoding/decoding** - Custom parsers for LDAP messages
- Raw TCP handling with tokio for async I/O
- Chosen for maximum LLM control over directory data and responses

## Architecture Decisions

### ASN.1 BER Protocol
LDAP uses ASN.1 BER (Basic Encoding Rules) for message encoding:
- **Manual parsers** implemented for:
  - `parse_ber_length()` - Decode BER length encoding (short/long form)
  - `parse_ber_integer()` - Decode BER INTEGER
  - `parse_ldap_message()` - Parse LDAP message SEQUENCE
- **Manual encoders** implemented for:
  - `encode_ber_length()` - Encode BER length
  - `encode_ber_integer()` - Encode BER INTEGER
  - `encode_ber_string()` - Encode BER OCTET STRING
  - `encode_ldap_message()` - Build LDAP message SEQUENCE

### LLM Integration
- **Three event types**:
  - `LDAP_BIND_EVENT` - Bind (authentication) request
  - `LDAP_SEARCH_EVENT` - Search request
  - `LDAP_UNBIND_EVENT` - Unbind (disconnect) notification
- **Action-based responses** - LLM returns JSON actions for directory operations
- **Binary protocol** - Actions produce raw BER-encoded binary data
- **No state persistence** - LLM manages directory entries in memory/context

### Session Management
Session tracked in local `LdapSession` struct:
```rust
authenticated: bool
bind_dn: Option<String>
```

Operations:
- **Bind** - Authentication sets `authenticated = true`, stores `bind_dn`
- **Search** - Passes authentication status to LLM
- **Add/Modify/Delete** - LLM decides authorization based on session
- **Unbind** - Fire-and-forget, no response required

### Response Actions
The LLM controls LDAP responses through these actions:
- `ldap_bind_response` - Bind response (success/failure)
- `ldap_search_response` - Search results (entries + done)
- `ldap_add_response` - Add entry result
- `ldap_modify_response` - Modify entry result
- `ldap_delete_response` - Delete entry result
- `wait_for_more` - Accumulate data (not used)
- `close_connection` - Terminate session

### Binary Response Encoding
Actions produce BER-encoded binary responses:
- **BindResponse** `[APPLICATION 1]` - Result code 0 = success, 49 = invalidCredentials
- **SearchResultEntry** `[APPLICATION 4]` - DN + attributes (repeated for each entry)
- **SearchResultDone** `[APPLICATION 5]` - Final result code
- **AddResponse** `[APPLICATION 9]` - Result code 0 = success, 68 = entryAlreadyExists
- **ModifyResponse** `[APPLICATION 7]` - Result code 0 = success, 32 = noSuchObject
- **DelResponse** `[APPLICATION 11]` - Result code 0 = success, 32 = noSuchObject

## Connection Management
- Connections spawn independent async tasks
- No connection tracking in `AppState` (not properly integrated)
- Binary message parsing with buffer reading
- Write operations directly to TcpStream

## State Management
- **Session state** - Local to `LdapSession` (not in `AppState`)
- **Directory data** - Managed by LLM in conversation context
- **No persistence** - Entries exist only in LLM memory
- **Authentication** - Checked per operation, passed to LLM

## Limitations
- **No LDAP persistence** - All directory data ephemeral
- **No LDAPS** - TLS not implemented (port 389 only)
- **No SASL** - Only simple bind (username/password) supported
- **Simplified parsing** - Only handles basic BindRequest, SearchRequest, UnbindRequest
- **No ADD/MODIFY/DELETE parsing** - Operations detected but not fully parsed
- **No schema validation** - LLM decides attribute validity
- **No referrals** - No LDAP server chaining
- **No access control** - LLM decides authorization
- **No operational attributes** - createTimestamp, modifyTimestamp not tracked

## Examples

### Example LLM Prompt
```
Start LDAP server on port 389. Accept bind for 'cn=admin,dc=example,dc=com' with password 'secret'.
For search on 'dc=example,dc=com', return 2 users:
- cn=john,dc=example,dc=com with mail=john@example.com
- cn=jane,dc=example,dc=com with mail=jane@example.com
```

### Example LLM Response (Bind Success)
```json
{
  "actions": [
    {
      "type": "ldap_bind_response",
      "message_id": 1,
      "success": true,
      "message": "Bind successful"
    }
  ]
}
```

### Example LLM Response (Bind Failure)
```json
{
  "actions": [
    {
      "type": "ldap_bind_response",
      "message_id": 1,
      "success": false,
      "message": "Invalid credentials"
    }
  ]
}
```

### Example LLM Response (Search)
```json
{
  "actions": [
    {
      "type": "ldap_search_response",
      "message_id": 2,
      "entries": [
        {
          "dn": "cn=john,dc=example,dc=com",
          "attributes": {
            "cn": ["john"],
            "mail": ["john@example.com"],
            "objectClass": ["person", "inetOrgPerson"]
          }
        },
        {
          "dn": "cn=jane,dc=example,dc=com",
          "attributes": {
            "cn": ["jane"],
            "mail": ["jane@example.com"],
            "objectClass": ["person", "inetOrgPerson"]
          }
        }
      ],
      "result_code": 0
    }
  ]
}
```

### Example LLM Response (Add)
```json
{
  "actions": [
    {
      "type": "ldap_add_response",
      "message_id": 3,
      "success": true,
      "result_code": 0,
      "message": "Entry added successfully"
    }
  ]
}
```

## ASN.1 BER Encoding Notes
LDAP messages are SEQUENCE structures:
```
LDAPMessage ::= SEQUENCE {
    messageID       INTEGER,
    protocolOp      CHOICE { ... },
    controls        [0] Controls OPTIONAL
}
```

Result codes:
- 0 = success
- 2 = protocolError
- 32 = noSuchObject
- 49 = invalidCredentials
- 68 = entryAlreadyExists

## References
- RFC 4511 - Lightweight Directory Access Protocol (LDAP): The Protocol
- RFC 4510 - Lightweight Directory Access Protocol (LDAP): Technical Specification Road Map
- ITU-T X.690 - ASN.1 encoding rules (BER, CER, DER)
