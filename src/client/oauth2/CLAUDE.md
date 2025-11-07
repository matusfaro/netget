# OAuth2 Client Implementation

## Overview

OAuth2 client implementation for NetGet that supports multiple OAuth2 authentication flows with LLM control. This client enables secure authentication with OAuth2 providers using industry-standard flows.

## Library Choices

**Primary Library**: `oauth2` v4.4 - Comprehensive OAuth2 client library
- Supports all major OAuth2 flows (password, device code, client credentials, authorization code)
- Built-in PKCE support for security
- Async HTTP client integration with reqwest
- Type-safe token handling
- Refresh token support

## Architecture

### Connection Model

The OAuth2 client is **HTTP-based** and uses reqwest under the hood via the `oauth2` crate. Unlike traditional TCP-based clients, there is no persistent connection - instead, the client makes HTTP requests to token endpoints as needed.

### OAuth2 Flows Supported

1. **Resource Owner Password Credentials Flow**
   - Direct username/password authentication
   - Simple, no browser redirect required
   - Less secure (user trusts NetGet with credentials)
   - Action: `exchange_password`

2. **Device Code Flow**
   - User authenticates via browser on another device
   - Ideal for CLI applications
   - Displays verification URL and user code
   - Polls for completion automatically
   - Action: `start_device_code_flow`

3. **Client Credentials Flow**
   - Service-to-service authentication
   - No user context
   - Simplest for machine-to-machine scenarios
   - Action: `exchange_client_credentials`

4. **Authorization Code Flow**
   - Traditional web flow with redirect
   - Most secure (user never shares password)
   - Requires callback server or manual code paste
   - PKCE protection included
   - Actions: `generate_auth_url`, `exchange_code`

### State Management

OAuth2 client stores the following in protocol_data:
- `client_id` - OAuth2 client identifier
- `client_secret` - OAuth2 client secret (optional)
- `auth_url` - Authorization endpoint URL (optional, for auth code flow)
- `token_url` - Token endpoint URL (required)
- `device_auth_url` - Device authorization URL (optional, for device code flow)
- `access_token` - Current access token (after successful auth)
- `refresh_token` - Refresh token for token renewal
- `token_type` - Token type (usually "Bearer")
- `expires_in` - Token expiration time in seconds
- `scopes` - Granted scopes
- `pkce_verifier` - PKCE verifier for auth code flow
- `csrf_token` - CSRF protection token
- `device_code` - Device code for polling
- `polling_interval` - Device code polling interval

### LLM Integration

**Event Flow**:
1. Client initialized → `oauth2_connected` event
2. LLM chooses authentication flow based on instruction
3. Token obtained → `oauth2_token_obtained` event (tokens redacted for security)
4. Device code flow → `oauth2_device_code_started` event (displays URL and code)
5. Errors → `oauth2_error` event

**Action Execution**:
- Async actions: User-triggered (e.g., "authenticate with password")
- Sync actions: Response-triggered (e.g., refresh token on expiration)
- Custom action results for each flow type

**State Machine**: Idle → Processing → Completed (no data accumulation, request-response pattern)

## Implementation Details

### Token Security

Access tokens and refresh tokens are stored in protocol_data but **redacted in LLM events** for security. The LLM sees `"[REDACTED]"` instead of actual token values.

### Device Code Flow Polling

When device code flow is initiated:
1. Client displays verification URL and user code
2. Spawns background task to poll every N seconds (server-specified interval)
3. Polls up to 60 times (5 minutes with 5-second interval)
4. Automatically fires `oauth2_token_obtained` event on success
5. Stops polling if token obtained or timeout reached

### PKCE (Proof Key for Code Exchange)

Authorization code flow automatically uses PKCE (SHA-256) for enhanced security:
1. Generates random code verifier
2. Creates SHA-256 code challenge
3. Stores verifier in protocol_data
4. Sends challenge with auth URL
5. Uses verifier when exchanging code for token

### Token Refresh

When a refresh token is available, the LLM can trigger token refresh:
```json
{
  "type": "refresh_token"
}
```

The client automatically handles refresh and fires a new `oauth2_token_obtained` event.

## Startup Parameters

Required:
- `client_id` - OAuth2 client ID from provider
- `token_url` - Token endpoint URL

Optional:
- `client_secret` - Client secret (required for some flows)
- `auth_url` - Authorization endpoint (required for auth code flow)
- `scopes` - Default scopes to request
- `device_auth_url` - Device authorization endpoint (defaults to `{remote_addr}/device/code`)

## Example Usage

### Password Flow
```
open_client oauth2 https://provider.com/oauth --client_id=my-client --client_secret=secret --token_url=https://provider.com/oauth/token "Authenticate with username 'user@example.com' and password 'secret123'"
```

### Device Code Flow
```
open_client oauth2 https://provider.com/oauth --client_id=my-client --token_url=https://provider.com/oauth/token --device_auth_url=https://provider.com/oauth/device/code "Use device code flow for authentication"
```

### Client Credentials Flow
```
open_client oauth2 https://provider.com/oauth --client_id=my-client --client_secret=secret --token_url=https://provider.com/oauth/token "Get service account token"
```

### Authorization Code Flow
```
open_client oauth2 https://provider.com/oauth --client_id=my-client --auth_url=https://provider.com/oauth/authorize --token_url=https://provider.com/oauth/token "Generate authorization URL for user authentication"
```

## Limitations

1. **No Token Revocation**: Currently does not support token revocation endpoints
2. **No Introspection**: Does not support OAuth2 introspection (RFC 7662)
3. **Callback Server**: Authorization code flow requires manual code paste (no embedded callback server)
4. **Single Provider**: One client instance per provider (cannot share tokens across multiple clients)
5. **No Dynamic Registration**: Does not support OAuth2 Dynamic Client Registration (RFC 7591)
6. **No JWT Validation**: Does not validate JWT tokens (use OpenID Connect for ID token validation)

## Protocol Information

- **Stack**: ETH>IP>TCP>HTTP>OAuth2
- **RFCs**:
  - RFC 6749 (OAuth 2.0 Authorization Framework)
  - RFC 7636 (PKCE)
  - RFC 8628 (Device Authorization Grant)
- **Port**: N/A (HTTP-based, uses provider's endpoints)
- **Security**: Supports PKCE, redacts tokens in logs and LLM events

## Dependencies

- `oauth2` crate: OAuth2 client implementation
- `reqwest`: HTTP client (via oauth2 crate)
- Built-in types for type-safe token handling

## Testing Considerations

E2E testing requires:
1. Mock OAuth2 server or public test provider
2. Test accounts with known credentials
3. Validation of token exchange flows
4. Refresh token testing

See `tests/client/oauth2/CLAUDE.md` for test strategy.
