# SAML Identity Provider (IDP) Implementation

## Overview

SAML IDP implements a SAML 2.0 Identity Provider that authenticates users and generates signed SAML assertions. This is an Experimental-status implementation where the LLM controls all authentication decisions, user attributes, and assertion generation.

**Protocol Compliance**: SAML 2.0 Web Browser SSO Profile
**Transport**: HTTP/1.1 over TCP
**Status**: Experimental - LLM-controlled authentication and assertion generation

## Library Choices

### HTTP Implementation
- **hyper v1.5** - HTTP/1.1 server with async/await support
- **http-body-util** - Request body collection
- **tokio** - Async runtime

**Rationale**: SAML uses HTTP as transport layer. Hyper provides robust HTTP server capabilities. No external SAML library needed since LLM generates assertions.

### No SAML Library
- **Manual assertion generation** - LLM generates SAML assertion XML
- **Manual metadata generation** - LLM generates EntityDescriptor XML
- **No XML signing** - Signatures can be added by LLM if needed

**Rationale**: The `samael` crate (0.0.19) is work-in-progress and may constrain LLM flexibility. Manual XML generation allows full LLM control over assertion structure, attributes, and signing.

## Architecture Decisions

### 1. LLM-Controlled Authentication

**Design Philosophy**: All authentication decisions and user attributes are determined by the LLM based on user instructions.

**Control Points**:
- Authentication logic (username/password validation, MFA, etc.)
- User attribute mapping (email, roles, groups)
- SAML assertion structure and content
- Session management
- Error responses

**Benefits**:
- Flexible authentication rules
- Dynamic attribute mapping
- Testing/demonstration scenarios
- Honeypot mode
- Custom authentication flows

### 2. HTTP-Based Request Handling

**Request Flow**:
1. Accept HTTP connection
2. Parse HTTP request (method, path, query, body)
3. Create `SAML_IDP_REQUEST_EVENT` with request details
4. Call LLM with event
5. Execute action result (send response)

**Supported Paths**:
- `/sso` or `/SingleSignOnService` - SSO endpoint (receives AuthnRequest)
- `/metadata` - IDP metadata endpoint
- Custom paths (LLM-defined)

**HTTP Bindings**:
- **HTTP-Redirect**: AuthnRequest in query parameter (GET)
- **HTTP-POST**: AuthnRequest in form body (POST)

### 3. Action-Based Response System

**Sync Actions** (requires network context):
- `send_saml_response` - Send SAML assertion to SP
- `send_metadata` - Send IDP metadata XML
- `send_error_response` - Return authentication error

**Async Actions** (user-triggered):
- None currently (IDP is request-response)

**Action Execution**:
- LLM returns action with assertion_xml or metadata_xml
- Action handler builds HTTP response
- For assertions: HTTP-POST form with auto-submit
- Response sent to client

### 4. SAML Assertion Structure

**LLM Responsibility**: Generate complete SAML assertion XML including:
- `<saml:Assertion>` - Root element with ID, IssueInstant
- `<saml:Issuer>` - IDP entity ID
- `<saml:Subject>` - User identifier (NameID)
- `<saml:Conditions>` - Validity period (NotBefore, NotOnOrAfter)
- `<saml:AuthnStatement>` - Authentication method and time
- `<saml:AttributeStatement>` - User attributes (email, roles, etc.)
- `<ds:Signature>` - Optional XML signature (can be added by LLM)

**Example Assertion** (simplified):
```xml
<saml:Assertion ID="_abc123" IssueInstant="2025-01-01T00:00:00Z">
  <saml:Issuer>https://idp.example.com</saml:Issuer>
  <saml:Subject>
    <saml:NameID>testuser</saml:NameID>
  </saml:Subject>
  <saml:Conditions NotBefore="2025-01-01T00:00:00Z" NotOnOrAfter="2025-01-01T01:00:00Z"/>
  <saml:AuthnStatement AuthnInstant="2025-01-01T00:00:00Z"/>
  <saml:AttributeStatement>
    <saml:Attribute Name="email">
      <saml:AttributeValue>test@example.com</saml:AttributeValue>
    </saml:Attribute>
  </saml:AttributeStatement>
</saml:Assertion>
```

### 5. HTTP-POST Binding

**Auto-Submit Form**: When LLM sends SAML response, the server generates an HTML form with:
- Hidden field `SAMLResponse` - Base64-encoded assertion XML
- Hidden field `RelayState` - Optional state parameter
- Form action - SP Assertion Consumer Service (ACS) URL
- JavaScript auto-submit on page load

**User Experience**: Browser automatically redirects to SP with assertion.

### 6. Connection Management

**HTTP Keep-Alive**: Each request uses a new HTTP connection (hyper default).

**Connection Tracking**:
- Connections tracked in AppState with connection_id
- Bytes sent/received tracked per request
- Recent requests stored in ProtocolConnectionInfo::SamlIdp

**Dual Logging**:
- All logs to tracing macros (debug!, info!, etc.)
- Status updates sent via status_tx channel
- Request summaries at DEBUG level
- Full payloads at TRACE level

## LLM Integration

**Event Type**: `SAML_IDP_REQUEST_EVENT`

**Event Data**:
```json
{
  "method": "GET",
  "path": "/sso",
  "query": "SAMLRequest=...",
  "headers": [["User-Agent", "Mozilla/5.0"]],
  "body": null,
  "client_ip": "127.0.0.1"
}
```

**LLM Prompt Context**:
- Available actions: send_saml_response, send_metadata, send_error_response
- Path information (SSO vs metadata)
- Client IP (for logging/filtering)
- Request parameters (SAMLRequest, RelayState)

**Response Actions**:
```json
{
  "actions": [
    {
      "type": "send_saml_response",
      "assertion_xml": "<saml:Assertion>...</saml:Assertion>",
      "relay_state": "original-sp-url"
    }
  ]
}
```

**Scripting**: Possible - cache LLM-generated assertions for known requests.

## Limitations

### Not Implemented
1. **XML Signature Verification** - AuthnRequest signatures not verified
2. **XML Signature Generation** - Assertions not cryptographically signed (LLM can add fake signatures)
3. **Certificate Management** - No key generation or storage
4. **Metadata Refresh** - No automatic metadata generation
5. **Multi-Factor Authentication** - No built-in MFA support
6. **Session Management** - No persistent sessions
7. **Logout Support** - No SingleLogoutService endpoint
8. **Artifact Binding** - Only HTTP-Redirect and HTTP-POST supported

### Current Capabilities
- Serve SAML assertions via LLM
- Serve IDP metadata via LLM
- HTTP-Redirect and HTTP-POST bindings
- Flexible attribute mapping
- Custom authentication logic
- Error responses

### Known Issues
- No cryptographic signing (assertions not tamper-proof)
- No AuthnRequest validation
- No replay attack prevention
- LLM must generate well-formed XML

## Example Prompts

### Start a simple IDP
```
Start a SAML Identity Provider on port 8080.
When clients request /sso, authenticate all users as 'testuser' with email 'test@example.com'.
When clients request /metadata, return a basic EntityDescriptor with SSO endpoint at http://localhost:8080/sso.
```

### IDP with attribute mapping
```
Start a SAML IDP on port 8080.
For user 'alice', return attributes: email=alice@example.com, role=admin.
For user 'bob', return attributes: email=bob@example.com, role=user.
For any other user, return an authentication error.
```

### IDP with custom assertions
```
Start a SAML IDP on port 8080.
Generate SAML assertions with:
- Issuer: https://myidp.example.com
- Validity: 1 hour
- AuthnContext: urn:oasis:names:tc:SAML:2.0:ac:classes:PasswordProtectedTransport
Include attributes: email, displayName, memberOf
```

## References

- [SAML 2.0 Specification](https://docs.oasis-open.org/security/saml/Post2.0/sstc-saml-tech-overview-2.0.html)
- [SAML 2.0 Web Browser SSO Profile](https://docs.oasis-open.org/security/saml/v2.0/saml-profiles-2.0-os.pdf)
- [SAML 2.0 Bindings](https://docs.oasis-open.org/security/saml/v2.0/saml-bindings-2.0-os.pdf)
- [Okta SAML Guide](https://developer.okta.com/docs/concepts/saml/)

## Implementation Statistics

| Module | Lines of Code | Purpose |
|--------|--------------|---------|
| `mod.rs` | ~270 | HTTP session handling, request parsing |
| `actions.rs` | ~330 | Action definitions, response generation |
| **Total** | **~600** | Basic IDP implementation |

This is an Experimental implementation focused on LLM-controlled authentication and assertion generation for testing, demonstration, and honeypot scenarios. Production use requires cryptographic signing and certificate management.
