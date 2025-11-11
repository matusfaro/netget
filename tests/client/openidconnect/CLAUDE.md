# OpenID Connect Client E2E Tests

## Test Strategy

The OpenID Connect client tests follow a **limited black-box approach** due to the complexity of running a full OIDC
provider. Tests focus on:

1. **Client Initialization**: Verify the client can be created and configured
2. **Discovery**: Test OIDC provider discovery (`.well-known/openid-configuration`)
3. **LLM Interpretation**: Verify the LLM understands OIDC-specific instructions
4. **Error Handling**: Ensure graceful failure with invalid providers
5. **Lifecycle**: Test clean connection and disconnection

## LLM Call Budget

**Total Budget**: < 6 LLM calls

### Test Breakdown

1. `test_oidc_client_initialization` - **1 LLM call**
    - Client connection with discovery
    - Verifies discovery attempt (may fail on auth but discovery succeeds)

2. `test_oidc_client_with_parameters` - **1 LLM call**
    - Client connection with startup parameters
    - Verifies parameter parsing

3. `test_oidc_client_flow_interpretation` - **1 LLM call**
    - Client connection with flow instruction
    - Verifies LLM understands flow types (device code, password, etc.)

4. `test_oidc_client_invalid_provider` - **1 LLM call**
    - Client connection to invalid provider
    - Verifies error handling

5. `test_oidc_client_disconnect` - **1 LLM call**
    - Client connection and disconnect
    - Verifies lifecycle management

**Efficiency Strategy**:

- No server required (connects to public OIDC providers)
- Quick tests (1-2 seconds each)
- Focus on initialization and LLM interpretation, not full auth flows
- Use well-known providers (Google, Microsoft) for discovery tests

## Expected Runtime

**Total Runtime**: ~10-15 seconds

- Each test runs in 1-2 seconds
- No waiting for complex authentication flows
- Network latency for discovery (1-2 seconds per test)
- Minimal setup/teardown

## Testing Limitations

### What We Can Test

✅ Client initialization and configuration
✅ OIDC provider discovery (`.well-known/openid-configuration`)
✅ LLM instruction interpretation
✅ Error handling for invalid providers
✅ Client lifecycle (connect/disconnect)
✅ Parameter parsing and validation

### What We Cannot Test (Without Full Provider)

❌ **Token Exchange**: Requires valid credentials
❌ **Password Flow**: Needs real username/password
❌ **Device Code Flow**: Requires polling and user interaction
❌ **Client Credentials Flow**: Needs client secret
❌ **UserInfo Endpoint**: Requires valid access token
❌ **Token Refresh**: Requires valid refresh token

### Why These Limitations?

1. **No Test Provider**: Running a full OIDC provider (Keycloak, Auth0) is complex
2. **Credentials Required**: Real authentication needs valid credentials
3. **Interactive Flows**: Device code flow requires user browser interaction
4. **Network Dependency**: Tests rely on external providers (Google, etc.)

## Alternative Testing Approaches

### Option 1: Mock HTTP Server (Future)

Create a mock OIDC provider that:

- Serves `.well-known/openid-configuration`
- Returns fake tokens for testing
- Implements minimal OIDC endpoints

**Pros**: Full flow testing without real credentials
**Cons**: Significant implementation effort, doesn't test real providers

### Option 2: Local Keycloak (Future)

Run Keycloak in Docker for tests:

```bash
docker run -p 8080:8080 -e KEYCLOAK_USER=admin -e KEYCLOAK_PASSWORD=admin quay.io/keycloak/keycloak:latest
```

**Pros**: Real OIDC provider, full flow testing
**Cons**: Requires Docker, slower tests, complex setup

### Option 3: Public Test Providers (Current)

Use public providers for discovery only:

- Google: `https://accounts.google.com`
- Microsoft: `https://login.microsoftonline.com/common/v2.0`

**Pros**: No setup, real discovery testing
**Cons**: Cannot test full auth flows, network dependency

## Known Issues

1. **Network Dependency**: Tests require internet access for discovery
    - May fail in offline environments
    - Rate limiting possible with public providers

2. **Discovery Failures**: Public providers may change metadata
    - Tests verify client handles errors gracefully
    - Not testing exact discovery response content

3. **No Full Auth**: Cannot test complete authentication flows
    - Tests verify initialization only
    - Manual testing required for full flows

4. **LLM Interpretation Variance**: LLM may interpret instructions differently
    - Tests verify protocol recognition, not exact actions
    - Prompts designed to be clear and unambiguous

## Running the Tests

```bash
# Run all OpenID Connect client tests
./cargo-isolated.sh test --no-default-features --features openidconnect --test client::openidconnect::e2e_test

# Run with output
./cargo-isolated.sh test --no-default-features --features openidconnect --test client::openidconnect::e2e_test -- --nocapture

# Run specific test
./cargo-isolated.sh test --no-default-features --features openidconnect --test client::openidconnect::e2e_test test_oidc_client_initialization
```

## Test Requirements

- **Internet Connection**: Required for OIDC provider discovery
- **Ollama Running**: LLM must be available
- **No Credentials**: Tests don't require real OIDC credentials

## Future Enhancements

1. **Mock Provider**: Implement minimal OIDC mock server
2. **Integration Tests**: Add tests with local Keycloak
3. **Token Validation**: Test JWT parsing and validation
4. **Full Flows**: Test complete auth flows with test credentials
5. **Multiple Providers**: Test Google, Microsoft, Auth0 compatibility
6. **Offline Mode**: Add tests that don't require network

## Manual Testing

For full authentication flow testing, use NetGet interactively:

```bash
# Test password flow
./cargo-isolated.sh run --no-default-features --features openidconnect
# In NetGet:
# Connect to https://your-oidc-provider.com as OpenID Connect client with client_id=YOUR_CLIENT_ID
# Exchange username YOUR_USERNAME and password YOUR_PASSWORD for tokens
# Fetch user information

# Test client credentials flow
./cargo-isolated.sh run --no-default-features --features openidconnect
# In NetGet:
# Connect to https://your-oidc-provider.com as OpenID Connect client with client_id=YOUR_CLIENT_ID and client_secret=YOUR_SECRET
# Exchange client credentials for access token
```

## Success Criteria

Tests pass if:

1. Client initializes without panics
2. Discovery attempts are made (even if they fail on auth)
3. LLM recognizes OIDC-specific instructions
4. Errors are handled gracefully
5. No resource leaks (clean disconnection)

**Total LLM Budget**: < 6 calls ✅
**Total Runtime**: ~10-15 seconds ✅
**Network Required**: Yes (for discovery) ⚠️
**Credentials Required**: No ✅
