# SAML Service Provider (SP) Implementation

## Overview

SAML SP implements a SAML 2.0 Service Provider that validates SAML assertions and manages application sessions. This is
an Experimental-status implementation where the LLM controls authorization decisions, attribute extraction, and session
management.

**Protocol Compliance**: SAML 2.0 Web Browser SSO Profile
**Transport**: HTTP/1.1 over TCP
**Status**: Experimental - LLM-controlled authorization and session management

## Library Choices

### HTTP Implementation

- **hyper v1.5** - HTTP/1.1 server with async/await support
- **http-body-util** - Request body collection
- **tokio** - Async runtime

**Rationale**: SAML uses HTTP as transport layer. Hyper provides robust HTTP server capabilities. No external SAML
library needed since LLM validates assertions.

### No SAML Library

- **Manual assertion parsing** - LLM extracts user data from assertions
- **Manual metadata generation** - LLM generates EntityDescriptor XML
- **No XML signature verification** - Validation can be performed by LLM if needed

**Rationale**: The `samael` crate (0.0.19) is work-in-progress and may constrain LLM flexibility. Manual XML parsing
allows full LLM control over validation logic, attribute extraction, and session creation.

## Architecture Decisions

### 1. LLM-Controlled Authorization

**Design Philosophy**: All authorization decisions and session management are determined by the LLM based on user
instructions.

**Control Points**:

- Assertion validation (signature, expiration, issuer)
- Attribute extraction (email, roles, groups)
- Authorization logic (role-based access)
- Session creation and management
- Error responses

**Benefits**:

- Flexible authorization rules
- Dynamic attribute processing
- Testing/demonstration scenarios
- Custom session management
- Integration with existing systems

### 2. HTTP-Based Request Handling

**Request Flow**:

1. Accept HTTP connection
2. Parse HTTP request (method, path, query, body)
3. Create `SAML_SP_REQUEST_EVENT` with request details
4. Call LLM with event
5. Execute action result (send response)

**Supported Paths**:

- `/login` - Initiate SP-initiated SSO (redirect to IDP)
- `/acs` or `/AssertionConsumerService` - ACS endpoint (receives SAMLResponse)
- `/metadata` - SP metadata endpoint
- Custom paths (LLM-defined)

**HTTP Bindings**:

- **HTTP-Redirect**: Redirect user to IDP with AuthnRequest
- **HTTP-POST**: Receive SAMLResponse in form body (POST)

### 3. Action-Based Response System

**Sync Actions** (requires network context):

- `send_authn_request` - Initiate authentication with IDP
- `process_assertion` - Validate assertion and create session
- `send_metadata` - Send SP metadata XML
- `send_error_response` - Return authorization error

**Async Actions** (user-triggered):

- None currently (SP is request-response)

**Action Execution**:

- LLM returns action with request_xml, user_id, or metadata_xml
- Action handler builds HTTP response
- For AuthnRequest: HTTP-Redirect or HTTP-POST form
- For successful auth: Set session cookie
- Response sent to client

### 4. SP-Initiated SSO Flow

**LLM Responsibility**: Generate AuthnRequest XML when user accesses `/login`:

```xml
<samlp:AuthnRequest ID="_xyz789" IssueInstant="2025-01-01T00:00:00Z">
  <saml:Issuer>https://sp.example.com</saml:Issuer>
  <samlp:NameIDPolicy Format="urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress"/>
</samlp:AuthnRequest>
```

**HTTP-Redirect Flow**:

1. LLM generates AuthnRequest XML
2. Server base64-encodes request
3. Server builds redirect URL: `https://idp.example.com/sso?SAMLRequest=...`
4. Browser redirects to IDP
5. User authenticates at IDP
6. IDP redirects back to SP ACS endpoint

### 5. IDP-Initiated SSO Flow

**LLM Responsibility**: Receive and validate SAMLResponse at `/acs`:

1. Extract SAMLResponse from POST body
2. Base64-decode assertion XML
3. Validate assertion (signature, expiration, issuer)
4. Extract user attributes (NameID, email, roles)
5. Create application session (set cookie)
6. Redirect user to application

**Session Management**: LLM controls session creation, typically via HTTP cookies.

### 6. Assertion Validation

**LLM Responsibility**: Validate incoming assertions for:

- **Signature Verification** - Check XML signature (if LLM implements)
- **Expiration** - Verify NotBefore/NotOnOrAfter conditions
- **Issuer Validation** - Ensure assertion from trusted IDP
- **Audience Restriction** - Verify assertion intended for this SP
- **Replay Protection** - Check assertion ID uniqueness (if LLM implements)

**Flexible Validation**: LLM can implement strict or relaxed validation based on requirements.

### 7. Connection Management

**HTTP Keep-Alive**: Each request uses a new HTTP connection (hyper default).

**Connection Tracking**:

- Connections tracked in AppState with connection_id
- Bytes sent/received tracked per request
- Recent requests stored in ProtocolConnectionInfo::SamlSp

**Dual Logging**:

- All logs to tracing macros (debug!, info!, etc.)
- Status updates sent via status_tx channel
- Request summaries at DEBUG level
- Full payloads at TRACE level

## LLM Integration

**Event Type**: `SAML_SP_REQUEST_EVENT`

**Event Data** (ACS endpoint receiving assertion):

```json
{
  "method": "POST",
  "path": "/acs",
  "query": null,
  "headers": [["Content-Type", "application/x-www-form-urlencoded"]],
  "body": "SAMLResponse=...&RelayState=...",
  "client_ip": "127.0.0.1"
}
```

**LLM Prompt Context**:

- Available actions: send_authn_request, process_assertion, send_metadata, send_error_response
- Path information (login, ACS, metadata)
- Client IP (for logging)
- Request parameters (SAMLResponse, RelayState)

**Response Actions** (successful authentication):

```json
{
  "actions": [
    {
      "type": "process_assertion",
      "user_id": "testuser",
      "attributes": {
        "email": "test@example.com",
        "role": "admin"
      }
    }
  ]
}
```

**Scripting**: Possible - cache validation logic for known assertions.

## Limitations

### Not Implemented

1. **XML Signature Verification** - Assertion signatures not cryptographically verified
2. **Certificate Management** - No cert storage or trust management
3. **Metadata Refresh** - No automatic metadata generation
4. **Persistent Sessions** - No session database
5. **Logout Support** - No SingleLogoutService endpoint
6. **Artifact Binding** - Only HTTP-Redirect and HTTP-POST supported
7. **Encryption Support** - Encrypted assertions not supported
8. **Replay Protection** - No assertion ID tracking

### Current Capabilities

- Initiate SP-initiated SSO
- Receive and process assertions via LLM
- Serve SP metadata via LLM
- Flexible attribute extraction
- Custom authorization logic
- Session creation (cookies)
- Error responses

### Known Issues

- No cryptographic signature verification
- No persistent session storage
- No replay attack prevention
- LLM must correctly parse XML

## Example Prompts

### Start a simple SP

```
Start a SAML Service Provider on port 8081.
When users access /login, redirect them to IDP at https://idp.example.com/sso.
When receiving assertions at /acs, accept all assertions and create a session.
When users request /metadata, return a basic EntityDescriptor with ACS endpoint at http://localhost:8081/acs.
```

### SP with role-based access

```
Start a SAML SP on port 8081.
When receiving assertions at /acs:
- Extract email and role attributes
- Grant access to users with role='admin'
- Deny access to users with role='guest'
- Set session cookie with username
```

### SP with attribute extraction

```
Start a SAML SP on port 8081.
When receiving assertions:
- Extract attributes: email, displayName, memberOf
- Validate assertion is from issuer 'https://myidp.example.com'
- Check assertion is not expired
- Create session with extracted attributes
```

## References

- [SAML 2.0 Specification](https://docs.oasis-open.org/security/saml/Post2.0/sstc-saml-tech-overview-2.0.html)
- [SAML 2.0 Web Browser SSO Profile](https://docs.oasis-open.org/security/saml/v2.0/saml-profiles-2.0-os.pdf)
- [SAML 2.0 Bindings](https://docs.oasis-open.org/security/saml/v2.0/saml-bindings-2.0-os.pdf)
- [Okta SAML Guide](https://developer.okta.com/docs/concepts/saml/)

## Implementation Statistics

| Module       | Lines of Code | Purpose                                 |
|--------------|---------------|-----------------------------------------|
| `mod.rs`     | ~270          | HTTP session handling, request parsing  |
| `actions.rs` | ~380          | Action definitions, response generation |
| **Total**    | **~650**      | Basic SP implementation                 |

This is an Experimental implementation focused on LLM-controlled authorization and session management for testing,
demonstration, and integration scenarios. Production use requires cryptographic signature verification and persistent
session storage.
