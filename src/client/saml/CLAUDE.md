# SAML Client Implementation

## Overview

This implements a SAML 2.0 Service Provider (SP) client that can initiate authentication with a SAML Identity Provider (IdP). The client allows LLM-controlled Single Sign-On (SSO) operations including generating authentication requests and validating SAML assertions.

## Library Choices

### XML Processing
- **quick-xml** (v0.37) - Fast, lightweight XML parser and writer
  - Used for generating AuthnRequest XML
  - Used for parsing SAML Response and Assertion XML
  - Provides streaming parser for efficient memory usage
  - Already used in the codebase for XML-RPC

### Encoding/Compression
- **base64** (v0.22) - Base64 encoding/decoding for SAML messages
- **flate2** (v1.0) - DEFLATE compression for HTTP-Redirect binding
- **urlencoding** (v2.1) - URL encoding for redirect parameters

### Utilities
- **uuid** (v1.11) - Generate unique request IDs
- **chrono** (v0.4) - Timestamp generation for SAML requests

## Architecture

### SAML Protocol Flow

1. **Initialization**: Client connects to IdP URL, stores configuration
2. **SSO Initiation**:
   - Generate SAML AuthnRequest with unique ID
   - Encode request (deflate + base64 for HTTP-Redirect, or base64 for HTTP-POST)
   - Build SSO URL with encoded request
   - LLM receives SSO URL to direct user
3. **Assertion Validation**:
   - Receive base64-encoded SAML Response
   - Decode and parse XML response
   - Extract status code, subject, and attributes
   - LLM processes authentication result

### HTTP Bindings Supported

1. **HTTP-Redirect** (Primary)
   - AuthnRequest is DEFLATE-compressed, base64-encoded, URL-encoded
   - Sent as query parameter: `?SAMLRequest=...&RelayState=...`
   - Lightweight, suitable for browser redirects

2. **HTTP-POST** (Planned)
   - AuthnRequest is base64-encoded (no compression)
   - Sent as form POST parameter
   - Supports larger requests

### State Management

Client stores in `protocol_data`:
- `idp_url`: Identity Provider endpoint URL
- `entity_id`: Service Provider entity identifier (default: `urn:netget:sp`)
- `acs_url`: Assertion Consumer Service URL (where IdP sends response)
- `binding`: SAML binding type (`redirect` or `post`)
- `request_id`: Generated request ID for validation
- `sso_url`: Complete SSO URL for user redirection

## LLM Integration

### Connection Event
Triggered when client is initialized:
```json
{
  "event": "saml_connected",
  "data": {
    "idp_url": "https://idp.example.com/saml/sso",
    "sso_url": "https://idp.example.com/saml/sso?SAMLRequest=...",
    "request_id": "_12345678-1234-1234-1234-123456789012"
  }
}
```

### Response Event
Triggered when assertion is validated:
```json
{
  "event": "saml_response_received",
  "data": {
    "success": true,
    "status_code": "urn:oasis:names:tc:SAML:2.0:status:Success",
    "assertion": {
      "subject": "user@example.com",
      "status_code": "urn:oasis:names:tc:SAML:2.0:status:Success"
    },
    "attributes": {
      "email": "user@example.com",
      "firstName": "John",
      "lastName": "Doe"
    }
  }
}
```

### Actions

#### Async Actions (User-triggered)

1. **initiate_sso** - Start authentication flow
   ```json
   {
     "type": "initiate_sso",
     "relay_state": "/protected/resource",
     "force_authn": false
   }
   ```

2. **validate_assertion** - Validate SAML response from IdP
   ```json
   {
     "type": "validate_assertion",
     "saml_response": "PHNhbWxwOlJlc3BvbnNlLi4uPg=="
   }
   ```

3. **disconnect** - Close SAML client
   ```json
   {
     "type": "disconnect"
   }
   ```

#### Sync Actions (Response to events)

1. **parse_assertion** - Parse assertion from response
   ```json
   {
     "type": "parse_assertion",
     "response_xml": "<samlp:Response...>"
   }
   ```

## Limitations

### Security Considerations

1. **No Signature Verification** - Current implementation does NOT verify XML signatures
   - SAML responses should be signed by IdP
   - Signature verification requires `xmlsec` integration (complex)
   - **For testing/development only** - not production-ready without signature validation

2. **No Certificate Management** - No handling of X.509 certificates
   - Production SAML requires certificate-based trust
   - Would need certificate storage and validation

3. **Basic XML Parsing** - Simple parsing without full SAML compliance
   - Extracts essential fields (status, subject, attributes)
   - Does not validate all SAML specification requirements
   - May fail on complex SAML responses

4. **No Encryption** - SAML assertions are not encrypted
   - Some IdPs require encrypted assertions
   - Would need XML encryption support

### Protocol Support

1. **HTTP-Redirect Only** - HTTP-POST binding partially implemented
2. **SP-Initiated SSO Only** - IdP-initiated flow not supported
3. **No Logout** - Single Logout (SLO) not implemented
4. **No Metadata** - No support for SAML metadata exchange

### Known Issues

1. **Timestamp Validation** - Does not validate NotBefore/NotOnOrAfter conditions
2. **Audience Validation** - Does not validate audience restriction
3. **InResponseTo** - Does not validate InResponseTo matches request ID
4. **Replay Protection** - No mechanism to prevent assertion replay attacks

## Testing Strategy

### Manual Testing
1. Use a public SAML test IdP (e.g., samltest.id)
2. Configure client with IdP metadata
3. Initiate SSO and follow redirect
4. Validate response manually

### E2E Testing
- Requires running SAML IdP (e.g., SimpleSAMLphp)
- Test successful authentication flow
- Test failed authentication
- Test attribute extraction

## Example Usage

```rust
// Open SAML client
let client_id = open_client(
    "saml",
    "https://idp.example.com/saml/sso",
    "Authenticate user with SAML IdP",
    Some(json!({
        "entity_id": "https://myapp.com/saml/sp",
        "acs_url": "https://myapp.com/saml/acs"
    }))
).await?;

// Initiate SSO
execute_action(client_id, json!({
    "type": "initiate_sso",
    "relay_state": "/dashboard",
    "force_authn": false
})).await?;

// After user completes authentication and returns with SAML response:
execute_action(client_id, json!({
    "type": "validate_assertion",
    "saml_response": "PHNhbWxwOlJlc3BvbnNlLi4uPg=="
})).await?;
```

## Future Enhancements

1. **Signature Verification** - Integrate xmlsec for XML signature validation
2. **Certificate Management** - Handle X.509 certificates properly
3. **Full SAML Compliance** - Implement all required validations
4. **HTTP-POST Binding** - Complete POST binding implementation
5. **IdP-Initiated SSO** - Support unsolicited responses
6. **Single Logout** - Implement SLO protocol
7. **Metadata Support** - Generate and consume SAML metadata XML
8. **Assertion Encryption** - Support encrypted assertions

## References

- [SAML 2.0 Specification](https://docs.oasis-open.org/security/saml/v2.0/)
- [SAML 2.0 Bindings](https://docs.oasis-open.org/security/saml/v2.0/saml-bindings-2.0-os.pdf)
- [SAML 2.0 Profiles](https://docs.oasis-open.org/security/saml/v2.0/saml-profiles-2.0-os.pdf)
