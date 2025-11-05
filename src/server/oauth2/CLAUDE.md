# OAuth2 Protocol Implementation

## Overview
OAuth2 authorization server implementing RFC 6749 (OAuth 2.0 Authorization Framework) and RFC 7662 (Token Introspection) and RFC 7009 (Token Revocation). The LLM controls authorization decisions, token generation, client validation, and scope management.

**Status**: Experimental (AI & API Protocol)
**RFC**: RFC 6749 (OAuth 2.0), RFC 7662 (Introspection), RFC 7009 (Revocation)

## Library Choices
- **hyper v1.0** - HTTP server foundation
- **Manual OAuth2 implementation** - LLM generates OAuth2 responses
- **URL encoding** - Parse query parameters and form bodies

**Rationale**: OAuth2 is fundamentally an HTTP-based protocol with specific request/response formats. Rather than using a heavyweight OAuth2 library like oxide-auth (which would limit LLM control), we implement OAuth2 endpoints manually. This gives the LLM complete control over:
- Authorization decisions (which clients to approve)
- Token generation (format, lifetime, scopes)
- Client credential validation
- Token introspection responses
- Custom OAuth2 flows and extensions

The LLM focuses on OAuth2 business logic rather than HTTP protocol details.

## Architecture Decisions

### 1. HTTP-Based Protocol
OAuth2 runs over HTTP/1.1:
- Each OAuth2 endpoint is an HTTP route
- Requests parsed from HTTP (query params for /authorize, form body for /token)
- Responses formatted as JSON (RFC 6749 format)
- No persistent connection state between requests

### 2. OAuth2 Endpoints
Four main endpoints per RFC 6749 and related RFCs:

**Authorization Endpoint** (`/authorize`):
- Handles authorization requests from clients
- Supports both GET (query params) and POST (form body)
- LLM receives: `response_type`, `client_id`, `redirect_uri`, `scope`, `state`
- LLM returns: authorization code via redirect
- Flow: Client → Authorize → LLM decision → Redirect with code

**Token Endpoint** (`/token`):
- Handles token requests (exchange code for token)
- POST only with form-encoded body
- Supports multiple grant types:
  - `authorization_code` - Exchange code for token
  - `refresh_token` - Refresh an access token
  - `password` - Resource Owner Password Credentials
  - `client_credentials` - Client Credentials grant
- LLM receives: `grant_type`, `code`, `client_id`, `client_secret`, etc.
- LLM returns: access token, refresh token, expiry
- Flow: Client → Token request → LLM validation → Token response

**Introspection Endpoint** (`/introspect`):
- RFC 7662 token introspection
- POST only with form-encoded body
- LLM receives: `token`, `token_type_hint`
- LLM returns: token metadata (active, scope, exp, etc.)
- Used by resource servers to validate tokens

**Revocation Endpoint** (`/revoke`):
- RFC 7009 token revocation
- POST only with form-encoded body
- LLM receives: `token`, `token_type_hint`
- Always returns 200 OK (per RFC 7009)
- LLM can track revoked tokens

### 3. Parameter Parsing
OAuth2 uses both query strings and form bodies:
- Authorization requests: Query parameters (GET) or form body (POST)
- Token/Introspect/Revoke: Form-encoded body only
- Helper function `parse_query_params()` handles URL decoding
- All parameters passed to LLM as structured JSON

### 4. LLM Integration
The LLM responds to OAuth2 events with actions:

**Events**:
- `oauth2_authorize` - Authorization request received
- `oauth2_token` - Token request received
- `oauth2_introspect` - Token introspection request
- `oauth2_revoke` - Token revocation request

**Available Actions**:
- `oauth2_authorize_response` - Return authorization code
- `oauth2_token_response` - Issue access token
- `oauth2_introspect_response` - Return token metadata
- `oauth2_error_response` - Return OAuth2 error
- Common actions: `show_message`, `update_instruction`, etc.

### 5. Response Format
OAuth2 responses follow RFC 6749 JSON format:

**Authorization Response** (302 redirect):
```
Location: <redirect_uri>?code=AUTH_CODE&state=STATE
```

**Token Response** (200 JSON):
```json
{
  "access_token": "ACCESS_TOKEN",
  "token_type": "Bearer",
  "expires_in": 3600,
  "refresh_token": "REFRESH_TOKEN",
  "scope": "read write"
}
```

**Introspection Response** (200 JSON):
```json
{
  "active": true,
  "scope": "read write",
  "client_id": "client123",
  "token_type": "Bearer",
  "exp": 1234567890
}
```

**Error Response** (400 JSON):
```json
{
  "error": "invalid_client",
  "error_description": "Client authentication failed"
}
```

### 6. Dual Logging
All OAuth2 operations use dual logging:
- **DEBUG**: Request summary (endpoint, client_id, grant_type)
- **TRACE**: Full request parameters
- **INFO**: Authorization/token decisions
- **ERROR**: Validation failures
- Both go to `netget.log` (via tracing) and TUI Status panel (via status_tx)

### 7. Connection Tracking
Unlike raw TCP, OAuth2 connections are tracked per-TCP-connection:
- One `ConnectionId` per TCP connection (not per request)
- `ProtocolConnectionInfo::OAuth2` tracks recent requests
- Multiple OAuth2 requests can occur on same TCP connection (HTTP keep-alive)
- Connection closed when TCP socket closes

## LLM Integration

### Action-Based Response Model
The LLM responds to OAuth2 events with actions:

**Example Authorization Response**:
```json
{
  "actions": [
    {
      "type": "oauth2_authorize_response",
      "code": "AUTH_xyz123",
      "state": "random_state"
    },
    {
      "type": "show_message",
      "message": "Authorization approved for client 'myapp'"
    }
  ]
}
```

**Example Token Response**:
```json
{
  "actions": [
    {
      "type": "oauth2_token_response",
      "access_token": "ACCESS_xyz123",
      "token_type": "Bearer",
      "expires_in": 3600,
      "refresh_token": "REFRESH_xyz123",
      "scope": "read write"
    }
  ]
}
```

**Example Error Response**:
```json
{
  "actions": [
    {
      "type": "oauth2_error_response",
      "error": "invalid_client",
      "error_description": "Unknown client ID"
    }
  ]
}
```

### Default Responses
If LLM doesn't provide an action:
- Authorization: Returns code "AUTH_CODE_123" with redirect
- Token: Returns access token "ACCESS_TOKEN_123"
- Introspect: Returns active=true
- Revoke: Returns 200 OK

This ensures the server always responds correctly.

## Connection Management

### Connection Lifecycle
1. **Accept**: `TcpListener::accept()` creates new TCP connection
2. **Register**: Connection added to `ServerInstance` with `ProtocolConnectionInfo::OAuth2`
3. **Serve**: Hyper's `http1::Builder::new().serve_connection()` handles HTTP/1.1 protocol
4. **Request Loop**: Each request on the connection calls the service function
5. **Route**: Service routes to appropriate OAuth2 endpoint handler
6. **LLM Call**: Handler calls LLM to generate OAuth2 response
7. **Close**: Connection removed when TCP socket closes

### Connection Data Structure
```rust
ProtocolConnectionInfo::OAuth2 {
    recent_requests: Vec<String>, // Track recent OAuth2 requests
}
```

## Known Limitations

### 1. No Token Storage
- Tokens are generated but not persisted
- LLM must track tokens via memory/instructions
- No database for token validation
- Each request is stateless from server's perspective

**Workaround**: For testing/demo scenarios, LLM can accept any well-formed token or use simple patterns (e.g., "ACCESS_*" tokens are valid).

### 2. No Client Registration
- No dynamic client registration (RFC 7591)
- Clients must be specified in LLM instructions
- No client database or management UI

**Workaround**: Specify allowed clients in initial prompt.

### 3. No User Authentication UI
- Authorization endpoint doesn't render login page
- Assumes user is already authenticated
- No session management

**Rationale**: NetGet focuses on protocol implementation, not full web UI. Authorization decisions are made by LLM based on prompt instructions.

### 4. No PKCE Support
- Proof Key for Code Exchange (RFC 7636) not implemented
- No code_challenge/code_verifier validation
- Less secure for public clients (mobile apps, SPAs)

**Future Enhancement**: Add PKCE parameter parsing and validation.

### 5. No Scope Validation
- Scopes are passed through without validation
- LLM can grant any scope requested
- No scope registry or enforcement

**Rationale**: LLM has flexibility to implement custom scope logic.

### 6. No TLS/HTTPS
- Raw HTTP only (no encryption)
- OAuth2 requires HTTPS in production (RFC 6749)
- Tokens transmitted in clear text

**Security Note**: Only suitable for testing/development. Never use in production without TLS.

## Example Prompts

### Simple OAuth2 Server
```
listen on port 8080 via oauth2
Accept client 'myapp' with secret 'secret123'
For authorization requests, approve all requests and return authorization codes
For token requests with valid code, issue access tokens with 1-hour expiry
Include refresh tokens with 30-day expiry
```

### Multi-Client OAuth2 Server
```
listen on port 8080 via oauth2
Clients:
- client_id: webapp, secret: web_secret, redirect_uri: http://localhost:3000/callback
- client_id: mobile, secret: mobile_secret, redirect_uri: myapp://callback
- client_id: service, secret: service_secret (client credentials only)

For authorization requests:
- Approve webapp and mobile with scopes: read, write
- Reject other clients with error: unauthorized_client

For token requests:
- authorization_code grant: Validate code and issue token
- client_credentials grant: Allow for 'service' client with admin scope
- refresh_token grant: Validate refresh token and issue new access token
```

### OAuth2 with Custom Scopes
```
listen on port 8080 via oauth2
Accept client 'api_client' with secret 'api_secret'

Scopes:
- users:read - Read user data
- users:write - Modify user data
- admin - Full admin access

For authorization requests:
- Grant only requested scopes that are valid
- Default to 'users:read' if no scope specified

For token requests:
- Issue tokens with granted scopes only
- Access tokens expire in 2 hours
- Refresh tokens expire in 7 days
```

### Token Introspection & Revocation
```
listen on port 8080 via oauth2
Accept client 'api' with secret 'secret'

For token introspection:
- Tokens starting with "ACCESS_" are active
- Return scope: "read write", exp: 1 hour from now
- Other tokens are inactive

For token revocation:
- Accept all revocation requests
- Log revoked tokens
- Return success (200 OK)
```

## Performance Characteristics

### Latency
- One LLM call per OAuth2 request
- Typical latency: 2-5 seconds per request with qwen3-coder:30b
- Authorization: ~2-5s (LLM decides)
- Token: ~2-5s (LLM validates and generates)

### Throughput
- Limited by LLM response time (2-5s per request)
- Concurrent requests processed in parallel
- Hyper handles connection multiplexing efficiently

### Concurrency
- Unlimited concurrent connections (bounded by system resources)
- Each connection processed on separate tokio task
- Ollama lock serializes LLM API calls across all connections

## Comparison with HTTP

| Feature | HTTP | OAuth2 |
|---------|------|--------|
| Protocol Base | HTTP/1.1 | HTTP/1.1 |
| Request Structure | Any | OAuth2-specific (query params, form body) |
| Response Structure | Any | OAuth2 JSON format |
| Use Case | Web APIs, general | Authorization/token server |
| Complexity | LLM implements any logic | LLM implements OAuth2 logic |

## Future Enhancements

### PKCE Support
Add Proof Key for Code Exchange (RFC 7636):
- Parse `code_challenge` and `code_challenge_method` from /authorize
- Validate `code_verifier` in /token request
- Enhance security for public clients

### OpenID Connect
Add OpenID Connect layer (RFC 7517):
- `/userinfo` endpoint - Return user claims
- ID token generation (JWT with user info)
- Discovery endpoint (`/.well-known/openid-configuration`)

### Dynamic Client Registration
Implement RFC 7591:
- `/register` endpoint - Register new clients
- Client metadata storage
- Client management (update, delete)

### JWT Access Tokens
Generate JWT access tokens instead of opaque tokens:
- Self-contained tokens with claims
- No introspection needed
- JWKS endpoint for public key distribution

### HTTPS/TLS Support
Add TLS using rustls (similar to DoT/DoH):
- Wrap listener with TLS acceptor
- Generate or load server certificate
- Secure token transmission

## References
- [RFC 6749: OAuth 2.0 Authorization Framework](https://datatracker.ietf.org/doc/html/rfc6749)
- [RFC 7662: OAuth 2.0 Token Introspection](https://datatracker.ietf.org/doc/html/rfc7662)
- [RFC 7009: OAuth 2.0 Token Revocation](https://datatracker.ietf.org/doc/html/rfc7009)
- [RFC 7636: Proof Key for Code Exchange (PKCE)](https://datatracker.ietf.org/doc/html/rfc7636)
- [RFC 7591: OAuth 2.0 Dynamic Client Registration](https://datatracker.ietf.org/doc/html/rfc7591)
- [OAuth 2.0 Security Best Current Practice](https://datatracker.ietf.org/doc/html/draft-ietf-oauth-security-topics)
- [Hyper Documentation](https://docs.rs/hyper/latest/hyper/)
