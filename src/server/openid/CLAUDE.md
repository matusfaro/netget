# OpenID Connect Server Implementation

## Overview

OpenID Connect (OIDC) is an authentication layer built on top of OAuth 2.0. This implementation provides a fully
LLM-controlled OIDC provider that can handle all standard OIDC endpoints and flows.

## Library Choices

### HTTP Server

- **Hyper** (via `hyper` and `hyper-util`): HTTP/1.1 server
- **http-body-util**: Body handling utilities
- **bytes**: Zero-copy byte buffer handling

### URL Handling

- **urlencoding**: Query parameter and form data encoding/decoding

### Why No Dedicated OIDC Library?

The Rust ecosystem lacks mature OIDC **provider** libraries. Available libraries focus on OIDC **clients** (relying
parties):

- `openidconnect-rs`: Client library for consuming OIDC providers
- `openid`: Client library supporting OIDC Core 1.0 and Discovery 1.0

Building a lightweight HTTP-based provider allows the LLM complete control over all responses, making this ideal for:

- Testing OIDC clients
- Security research and honeypot scenarios
- Customized authentication flows

## Architecture

### Endpoints

The server implements the following OIDC endpoints:

1. **Discovery** (`/.well-known/openid-configuration`)
    - Returns provider metadata (endpoints, supported features)
    - Always responds with JSON

2. **Authorization** (`/authorize`)
    - Handles authentication and authorization requests
    - Responds with 302 redirect containing authorization code or error
    - Query parameters: `response_type`, `client_id`, `redirect_uri`, `scope`, `state`, etc.

3. **Token** (`/token`)
    - Exchanges authorization code for tokens
    - POST request with form-encoded body
    - Returns access_token, id_token (JWT), refresh_token

4. **UserInfo** (`/userinfo`)
    - Returns user profile information
    - Requires Authorization header with access token
    - Returns JSON with user claims (sub, name, email, etc.)

5. **JWKS** (`/jwks.json`)
    - Provides public keys for JWT verification
    - Returns JSON Web Key Set (JWKS)

### State Management

The server maintains minimal state:

- **Issuer URL**: Provider's base URL
- **Supported Scopes**: OAuth scopes (default: `["openid", "profile", "email"]`)

All other state (authorization codes, tokens, user sessions) is managed by the LLM on a per-request basis.

### Request Flow

```
Client Request
    ↓
Classify Endpoint (discovery, authorization, token, userinfo, jwks)
    ↓
Parse Request (method, path, query params, headers, body, form data)
    ↓
Create Event → Send to LLM
    ↓
LLM Responds with Action (send_discovery_document, send_token_response, etc.)
    ↓
Build HTTP Response (JSON, redirect, etc.)
    ↓
Send to Client
```

### LLM Integration

The LLM has complete control over:

1. **Discovery Documents**: Endpoint URLs, supported features, algorithms
2. **Authorization Responses**: Authorization codes, redirect URLs, errors
3. **Token Generation**: JWT structure, claims, expiration, signing algorithm
4. **User Information**: Profile data, custom claims
5. **Public Keys**: JWKS format, key rotation
6. **Error Handling**: OAuth error codes, descriptions

#### Structured Data Pattern

Following NetGet's design principles, actions use structured data instead of raw bytes:

**Good** (Structured):

```json
{
  "type": "send_token_response",
  "access_token": "eyJhbGci...",
  "id_token": "eyJhbGci...",
  "token_type": "Bearer",
  "expires_in": 3600
}
```

**Bad** (Avoided):

```json
{
  "type": "send_response",
  "data": "eyJhY2Nlc3NfdG9rZW4iOi..."  // Base64-encoded
}
```

## Logging Strategy

Dual logging to both `netget.log` and TUI:

- **DEBUG**: Request summaries (method, path, endpoint type, size)
- **TRACE**: Full request details (headers, body, query params, form data)
- **INFO**: Provider configuration, connection lifecycle
- **WARN**: OAuth errors, invalid requests
- **ERROR**: Server errors, LLM failures

Example:

```
[DEBUG] OpenID GET /.well-known/openid-configuration (discovery)
→ OpenID GET /.well-known/openid-configuration → 200 (523 bytes)
```

## JWT Token Handling

The LLM is responsible for:

1. **Generating JWT tokens** (access_token, id_token)
2. **Including proper claims** (sub, iss, aud, exp, iat, nonce)
3. **Signing tokens** (or providing unsigned tokens for testing)
4. **Providing matching public keys** in JWKS endpoint

This design allows:

- Testing with valid/invalid signatures
- Custom claim structures
- Expired tokens for error testing
- Multiple signing algorithms

## Limitations

1. **No Built-in Cryptography**: LLM generates JWT strings directly (no automatic signing)
2. **No Session Management**: Stateless by design (LLM manages authorization codes/tokens)
3. **No User Database**: LLM fabricates user data for each request
4. **HTTP Only**: HTTPS requires external reverse proxy
5. **Single Protocol**: OAuth 2.0 flows only (no SAML, CAS, etc.)

## Security Considerations

This is a **testing/research** server, not production-ready:

- ⚠️ No persistent state or user validation
- ⚠️ LLM-generated tokens may not be cryptographically secure
- ⚠️ Designed for localhost/lab environments only
- ✅ Useful for testing OIDC clients
- ✅ Useful for security research and fuzzing
- ✅ Useful for honeypot scenarios

## Example Prompts

### Basic Provider

```
Start an OpenID Connect server on port 8080 with issuer http://localhost:8080
```

### Custom Scopes

```
Start an OIDC server on 9000 supporting scopes: openid, profile, email, admin
```

### Testing Scenario

```
Start an OIDC provider on 4443 that:
1. Accepts any client_id
2. Issues tokens with 1-hour expiration
3. Includes custom claim "department": "engineering"
```

## Future Enhancements

Potential improvements:

- OAuth 2.0 Device Flow support
- PKCE (Proof Key for Code Exchange) validation
- Client registration endpoint
- Token introspection endpoint
- Token revocation endpoint
- Session management endpoint
