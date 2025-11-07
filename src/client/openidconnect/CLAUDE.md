# OpenID Connect Client Implementation

## Overview

The OpenID Connect (OIDC) client implements a full OAuth2/OIDC authentication client for NetGet. It supports multiple OAuth2 flows and allows the LLM to control authentication, token management, and user information retrieval.

## Library Choice

**Primary**: `openidconnect` crate v3.5

The `openidconnect` crate is the official Rust implementation of OpenID Connect, providing:
- Full OpenID Connect Discovery (`.well-known/openid-configuration`)
- Multiple OAuth2 flows (Password, Client Credentials, Device Code, Authorization Code)
- Automatic token validation and JWT parsing
- Type-safe API with strong guarantees
- Built on top of the `oauth2` crate
- Async HTTP client support via `reqwest`

**Why this library**:
- Official implementation with active maintenance
- Comprehensive OAuth2/OIDC specification support
- Type-safe configuration and error handling
- Well-documented with extensive examples
- Used by many production Rust applications

## Architecture

### Connection Model

The OIDC client is **logically connected** (not a persistent TCP connection):
- Connects to an OpenID Connect provider via HTTP/HTTPS
- Discovery happens automatically on connection
- Stores provider metadata and configuration
- Makes HTTP requests on-demand for tokens and user info

### State Management

Client state stored in `protocol_data`:
- `provider_url`: OIDC provider issuer URL
- `provider_metadata`: Discovered configuration (endpoints, scopes)
- `client_id`: OAuth2 client identifier
- `client_secret`: OAuth2 client secret (if confidential client)
- `access_token`: Current access token
- `id_token`: OpenID Connect ID token (JWT with user claims)
- `refresh_token`: Refresh token for obtaining new access tokens
- `redirect_uri`: Redirect URI for authorization code flow

### OAuth2/OIDC Flows Supported

1. **Resource Owner Password Credentials** (`exchange_password`)
   - User provides username and password directly
   - NetGet exchanges credentials for tokens
   - **Pros**: Simple, no browser redirect
   - **Cons**: Less secure (user trusts NetGet with credentials)
   - **Use case**: Internal apps, testing, automation

2. **Client Credentials** (`exchange_client_credentials`)
   - Machine-to-machine authentication
   - Uses client ID and client secret only
   - No user context
   - **Use case**: Service accounts, API access

3. **Device Code Flow** (`start_device_flow`) - **Partially Implemented**
   - User authenticates via browser on another device
   - NetGet displays code and URL for user
   - Polls for completion
   - **Use case**: CLI apps, limited input devices
   - **Status**: Placeholder - requires polling implementation

4. **Authorization Code Flow** - **Not Implemented**
   - Traditional web flow with browser redirect
   - Requires local HTTP server to receive callback
   - **Use case**: Web apps, desktop apps with browser
   - **Status**: Future enhancement

### LLM Integration

#### Events Triggered

1. **`oidc_discovered`**: Provider configuration discovered
   - Issuer URL, endpoints, supported scopes
   - LLM can decide which flow to use

2. **`oidc_token_received`**: Tokens received
   - Access token, ID token, refresh token
   - LLM can fetch user info or make API calls

3. **`oidc_userinfo_received`**: User information retrieved
   - Subject (user ID), claims (name, email, etc.)
   - LLM can process user data

#### Actions Available

**Async (User-Triggered)**:
- `discover_configuration`: Discover provider metadata
- `start_device_flow`: Begin device code authentication
- `exchange_password`: Authenticate with username/password
- `exchange_client_credentials`: Get service account token
- `refresh_token`: Refresh access token
- `fetch_userinfo`: Get user information
- `disconnect`: Close connection

**Sync (Response Actions)**:
- `fetch_userinfo`: Automatically fetch user info after tokens
- `refresh_token`: Refresh expired tokens

## Implementation Details

### Discovery Process

```rust
// Automatic discovery on connection
let issuer_url = IssuerUrl::new("https://accounts.google.com")?;
let provider_metadata = CoreProviderMetadata::discover_async(
    issuer_url,
    async_http_client,
).await?;

// Discovered endpoints stored:
// - authorization_endpoint
// - token_endpoint
// - userinfo_endpoint
// - supported_scopes
```

### Token Exchange (Password Flow)

```rust
let client = CoreClient::from_provider_metadata(
    provider_metadata,
    OidcClientId::new(client_id),
    Some(ClientSecret::new(client_secret)),
);

let token_response = client
    .exchange_password(
        &ResourceOwnerUsername::new(username),
        &ResourceOwnerPassword::new(password),
    )
    .add_scope(Scope::new("openid".to_string()))
    .add_scope(Scope::new("profile".to_string()))
    .request_async(async_http_client)
    .await?;

// Extract tokens
let access_token = token_response.access_token().secret();
let id_token = token_response.extra_fields().id_token();
let refresh_token = token_response.refresh_token();
```

### UserInfo Retrieval

```rust
let userinfo: CoreUserInfoClaims = client
    .user_info(AccessToken::new(access_token), None)?
    .request_async(async_http_client)
    .await?;

// Access user claims
let subject = userinfo.subject(); // User ID
let email = userinfo.email();     // Optional
let name = userinfo.name();       // Optional
```

## Limitations

1. **Device Code Flow**: Partially implemented
   - Requires polling mechanism for completion
   - Device authorization endpoint not always available
   - Full implementation would need background polling task

2. **Authorization Code Flow**: Not implemented
   - Requires local HTTP server for callback
   - Browser automation complexity
   - Better suited for desktop/web apps than CLI

3. **Token Expiration**: Manual refresh required
   - No automatic token refresh on expiry
   - LLM must explicitly call `refresh_token` action
   - Future enhancement: Automatic refresh before expiry

4. **PKCE Support**: Not explicitly configured
   - PKCE (Proof Key for Code Exchange) enhances security
   - `openidconnect` crate supports it but not enabled

5. **Certificate Validation**: Uses system defaults
   - Custom CA certificates not configurable
   - Self-signed certificates will fail validation

6. **ID Token Validation**: Basic validation only
   - JWT signature validation handled by library
   - Additional claims validation (audience, expiry) could be enhanced

## Security Considerations

1. **Credential Storage**: Tokens stored in memory only
   - Not persisted to disk
   - Lost when client disconnected
   - Refresh tokens allow re-authentication without credentials

2. **Client Secret**: Stored in plaintext in memory
   - Should only be used for confidential clients
   - Public clients (mobile/SPA) should not use client secret

3. **Password Flow**: Less secure than other flows
   - User credentials visible to NetGet
   - Only use with trusted providers
   - Prefer device code or authorization code flows when possible

4. **TLS Enforcement**: All OIDC communications over HTTPS
   - HTTP provider URLs will be rejected by most providers
   - `openidconnect` crate enforces HTTPS for security

## Testing Strategy

### Unit Tests
- Provider metadata parsing
- Token response parsing
- Action execution logic

### E2E Tests
- Use public OIDC test providers (Auth0, Keycloak)
- Test password flow with test credentials
- Test client credentials flow
- Verify token refresh
- Verify UserInfo retrieval

### Test Providers
- **Local Keycloak**: Full OIDC implementation for testing
- **Auth0**: Public test instances
- **Google**: Real provider (requires test account)

## Example Prompts

### Password Flow
```
Connect to https://login.microsoftonline.com/common/v2.0 as OpenID Connect client
Exchange username user@example.com and password mypass123 for tokens
Fetch user information
```

### Client Credentials
```
Connect to https://auth.example.com as OpenID Connect client
Use client credentials to get service account token
```

### Device Code Flow
```
Connect to https://accounts.google.com as OpenID Connect client
Start device code flow for user authentication
```

## Future Enhancements

1. **Full Device Code Flow**: Implement polling with timeout
2. **Authorization Code Flow**: Add local HTTP server for callbacks
3. **Automatic Token Refresh**: Background task to refresh before expiry
4. **PKCE**: Enable for authorization code flow security
5. **Multiple Providers**: Support connecting to multiple providers simultaneously
6. **Token Introspection**: Validate tokens via introspection endpoint
7. **Revocation**: Revoke tokens on disconnect
8. **Session Management**: Track multiple sessions with different users

## References

- OpenID Connect Specification: https://openid.net/specs/openid-connect-core-1_0.html
- OAuth2 RFC 6749: https://tools.ietf.org/html/rfc6749
- `openidconnect` crate docs: https://docs.rs/openidconnect/
- OAuth2 flows explained: https://oauth.net/2/grant-types/
