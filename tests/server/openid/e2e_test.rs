//! E2E tests for OpenID Connect server
//!
//! Tests the full OIDC flow using HTTP requests.

#![cfg(all(test, feature = "openid"))]

use helpers::{
    netget_with_instruction, wait_for_log_match, OllamaGuard, PORT_OPENID_E2E,
};
use reqwest;
use serde_json::{json, Value};

#[path = "../helpers.rs"]
mod helpers;

/// Test OpenID Connect discovery endpoint and token flow
#[tokio::test]
#[ignore] // Only run with `cargo test --ignored` or in CI
async fn test_openid_connect_flow() {
    let _guard = OllamaGuard::new().await;

    // Start OpenID server with instruction
    let mut app = netget_with_instruction(&format!(
        r#"Start an OpenID Connect server on port {PORT_OPENID_E2E} with issuer http://localhost:{PORT_OPENID_E2E}.

When receiving requests:

1. For /.well-known/openid-configuration (discovery):
   - Return discovery document with all endpoints
   - Set authorization_endpoint: http://localhost:{PORT_OPENID_E2E}/authorize
   - Set token_endpoint: http://localhost:{PORT_OPENID_E2E}/token
   - Set userinfo_endpoint: http://localhost:{PORT_OPENID_E2E}/userinfo
   - Set jwks_uri: http://localhost:{PORT_OPENID_E2E}/jwks.json
   - Supported scopes: ["openid", "profile", "email"]
   - Supported response types: ["code", "id_token", "token id_token"]

2. For /authorize (authorization):
   - Accept any client_id
   - Redirect to redirect_uri with code=AUTH_CODE_123 and preserve state parameter

3. For /token (token endpoint):
   - Accept any authorization code
   - Return:
     - access_token: "ACCESS_TOKEN_XYZ"
     - token_type: "Bearer"
     - id_token: "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJ1c2VyMTIzIiwibmFtZSI6IkpvaG4gRG9lIiwiZW1haWwiOiJqb2huQGV4YW1wbGUuY29tIiwiaWF0IjoxNjAwMDAwMDAwLCJleHAiOjE2MDAwMDM2MDB9.signature"
     - expires_in: 3600
     - scope: "openid profile email"

4. For /userinfo (user info):
   - Return user claims:
     - sub: "user123"
     - name: "John Doe"
     - email: "john@example.com"
     - email_verified: true

5. For /jwks.json (public keys):
   - Return JWKS with one RSA key:
     - kty: "RSA"
     - use: "sig"
     - kid: "key1"
     - alg: "RS256"
     - n: "0vx7agoebGcQve..."
     - e: "AQAB"
"#
    ))
    .await;

    // Wait for server to start
    wait_for_log_match(&mut app, r"OpenID server listening on 127\.0\.0\.1:\d+", 30).await;

    let client = reqwest::Client::new();

    // Test 1: Discovery endpoint
    println!("Testing discovery endpoint...");
    let discovery_resp = client
        .get(format!(
            "http://localhost:{}/.well-known/openid-configuration",
            PORT_OPENID_E2E
        ))
        .send()
        .await
        .expect("Failed to send discovery request");

    assert_eq!(discovery_resp.status(), 200, "Discovery should return 200");
    let discovery: Value = discovery_resp
        .json()
        .await
        .expect("Failed to parse discovery JSON");

    assert_eq!(
        discovery["issuer"],
        format!("http://localhost:{}", PORT_OPENID_E2E)
    );
    assert_eq!(
        discovery["authorization_endpoint"],
        format!("http://localhost:{}/authorize", PORT_OPENID_E2E)
    );
    assert_eq!(
        discovery["token_endpoint"],
        format!("http://localhost:{}/token", PORT_OPENID_E2E)
    );
    assert_eq!(
        discovery["userinfo_endpoint"],
        format!("http://localhost:{}/userinfo", PORT_OPENID_E2E)
    );
    assert_eq!(
        discovery["jwks_uri"],
        format!("http://localhost:{}/jwks.json", PORT_OPENID_E2E)
    );

    println!("✓ Discovery endpoint works");

    // Test 2: Authorization endpoint (redirect flow)
    println!("Testing authorization endpoint...");
    let auth_resp = client
        .get(format!("http://localhost:{}/authorize", PORT_OPENID_E2E))
        .query(&[
            ("response_type", "code"),
            ("client_id", "test_client"),
            ("redirect_uri", "http://localhost:9999/callback"),
            ("scope", "openid profile email"),
            ("state", "random_state_123"),
        ])
        .send()
        .await
        .expect("Failed to send authorization request");

    // Should get a redirect (302)
    assert_eq!(
        auth_resp.status(),
        302,
        "Authorization should return 302 redirect"
    );

    let location = auth_resp
        .headers()
        .get("location")
        .expect("Missing Location header")
        .to_str()
        .expect("Invalid Location header");

    assert!(
        location.contains("code=AUTH_CODE_123"),
        "Redirect should contain authorization code"
    );
    assert!(
        location.contains("state=random_state_123"),
        "Redirect should preserve state parameter"
    );

    println!("✓ Authorization endpoint works");

    // Test 3: Token endpoint
    println!("Testing token endpoint...");
    let token_resp = client
        .post(format!("http://localhost:{}/token", PORT_OPENID_E2E))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", "AUTH_CODE_123"),
            ("redirect_uri", "http://localhost:9999/callback"),
            ("client_id", "test_client"),
        ])
        .send()
        .await
        .expect("Failed to send token request");

    assert_eq!(token_resp.status(), 200, "Token should return 200");
    let token: Value = token_resp.json().await.expect("Failed to parse token JSON");

    assert_eq!(token["access_token"], "ACCESS_TOKEN_XYZ");
    assert_eq!(token["token_type"], "Bearer");
    assert!(token["id_token"].is_string(), "ID token should be present");
    assert_eq!(token["expires_in"], 3600);
    assert_eq!(token["scope"], "openid profile email");

    println!("✓ Token endpoint works");

    // Test 4: UserInfo endpoint
    println!("Testing userinfo endpoint...");
    let userinfo_resp = client
        .get(format!("http://localhost:{}/userinfo", PORT_OPENID_E2E))
        .header("Authorization", "Bearer ACCESS_TOKEN_XYZ")
        .send()
        .await
        .expect("Failed to send userinfo request");

    assert_eq!(userinfo_resp.status(), 200, "UserInfo should return 200");
    let userinfo: Value = userinfo_resp
        .json()
        .await
        .expect("Failed to parse userinfo JSON");

    assert_eq!(userinfo["sub"], "user123");
    assert_eq!(userinfo["name"], "John Doe");
    assert_eq!(userinfo["email"], "john@example.com");
    assert_eq!(userinfo["email_verified"], true);

    println!("✓ UserInfo endpoint works");

    // Test 5: JWKS endpoint
    println!("Testing JWKS endpoint...");
    let jwks_resp = client
        .get(format!("http://localhost:{}/jwks.json", PORT_OPENID_E2E))
        .send()
        .await
        .expect("Failed to send JWKS request");

    assert_eq!(jwks_resp.status(), 200, "JWKS should return 200");
    let jwks: Value = jwks_resp.json().await.expect("Failed to parse JWKS JSON");

    assert!(jwks["keys"].is_array(), "JWKS should have keys array");
    let keys = jwks["keys"].as_array().expect("keys should be array");
    assert!(!keys.is_empty(), "JWKS should have at least one key");

    let key = &keys[0];
    assert_eq!(key["kty"], "RSA");
    assert_eq!(key["use"], "sig");
    assert_eq!(key["alg"], "RS256");
    assert!(key["n"].is_string(), "RSA modulus (n) should be present");
    assert!(key["e"].is_string(), "RSA exponent (e) should be present");

    println!("✓ JWKS endpoint works");

    println!("\n✅ All OpenID Connect endpoints tested successfully!");
}

/// Test OpenID Connect error handling
#[tokio::test]
#[ignore]
async fn test_openid_error_handling() {
    let _guard = OllamaGuard::new().await;

    // Start OpenID server with error handling instruction
    let mut app = netget_with_instruction(&format!(
        r#"Start an OpenID Connect server on port {}.

For any request to /authorize with missing required parameters (client_id or redirect_uri):
- Respond with 400 Bad Request
- Return JSON error: {{"error": "invalid_request", "error_description": "Missing required parameter"}}

For any request to /token with invalid grant_type:
- Respond with 400 Bad Request
- Return JSON error: {{"error": "unsupported_grant_type", "error_description": "Only authorization_code is supported"}}
"#,
        PORT_OPENID_E2E + 1
    ))
    .await;

    wait_for_log_match(&mut app, r"OpenID server listening on 127\.0\.0\.1:\d+", 30).await;

    let client = reqwest::Client::new();

    // Test invalid authorization request (missing client_id)
    println!("Testing invalid authorization request...");
    let resp = client
        .get(format!(
            "http://localhost:{}/authorize",
            PORT_OPENID_E2E + 1
        ))
        .query(&[("response_type", "code")])
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(resp.status(), 400, "Should return 400 for invalid request");
    let error: Value = resp.json().await.expect("Failed to parse error JSON");
    assert_eq!(error["error"], "invalid_request");

    println!("✓ Invalid authorization request handled correctly");

    // Test invalid token request (unsupported grant type)
    println!("Testing invalid token request...");
    let resp = client
        .post(format!("http://localhost:{}/token", PORT_OPENID_E2E + 1))
        .form(&[("grant_type", "client_credentials")])
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(resp.status(), 400, "Should return 400 for invalid grant");
    let error: Value = resp.json().await.expect("Failed to parse error JSON");
    assert_eq!(error["error"], "unsupported_grant_type");

    println!("✓ Invalid token request handled correctly");

    println!("\n✅ Error handling tests passed!");
}
