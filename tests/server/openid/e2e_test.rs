//! E2E tests for OpenID Connect server
//!
//! Tests the full OIDC flow using HTTP requests.

#![cfg(all(test, feature = "openid"))]

use crate::helpers::*;
use reqwest;
use serde_json::{json, Value};
use std::time::Duration;

/// Test OpenID Connect discovery endpoint and token flow
#[tokio::test]
async fn test_openid_connect_flow() -> E2EResult<()> {
    println!("\n=== E2E Test: OpenID Connect Flow ===");

    let instruction = r#"OpenID Connect server with issuer http://localhost:{AVAILABLE_PORT}.

When receiving requests:

1. For /.well-known/openid-configuration (discovery):
   - Return discovery document with all endpoints
   - Set authorization_endpoint, token_endpoint, userinfo_endpoint, jwks_uri
   - Supported scopes: ["openid", "profile", "email"]
   - Supported response types: ["code", "id_token", "token id_token"]

2. For /authorize (authorization):
   - Accept any client_id
   - Redirect to redirect_uri with code=AUTH_CODE_123 and preserve state parameter

3. For /token (token endpoint):
   - Accept any authorization code
   - Return access_token, token_type: "Bearer", id_token (JWT), expires_in: 3600, scope

4. For /userinfo (user info):
   - Return user claims: sub, name, email, email_verified

5. For /jwks.json (public keys):
   - Return JWKS with one RSA key
"#;

    let server_config = NetGetConfig::new(format!(
        "Start OpenID Connect server on port {{AVAILABLE_PORT}}. {}",
        instruction
    ))
    .with_mock(|mock| {
        mock
            // Mock 1: Server startup
            .on_instruction_containing("OpenID Connect server")
            .respond_with_actions(serde_json::json!([
                {
                    "type": "open_server",
                    "port": 0,
                    "base_stack": "http",
                    "instruction": instruction
                }
            ]))
            .expect_calls(1)
            .and()
            // Mock 2: Discovery endpoint
            .on_event("http_request_received")
            .and_event_data_contains("path", "/.well-known/openid-configuration")
            .respond_with_actions(serde_json::json!([
                {
                    "type": "send_http_response",
                    "status": 200,
                    "headers": {"Content-Type": "application/json"},
                    "body": serde_json::json!({
                        "issuer": "http://localhost:{AVAILABLE_PORT}",
                        "authorization_endpoint": "http://localhost:{AVAILABLE_PORT}/authorize",
                        "token_endpoint": "http://localhost:{AVAILABLE_PORT}/token",
                        "userinfo_endpoint": "http://localhost:{AVAILABLE_PORT}/userinfo",
                        "jwks_uri": "http://localhost:{AVAILABLE_PORT}/jwks.json",
                        "scopes_supported": ["openid", "profile", "email"],
                        "response_types_supported": ["code", "id_token", "token id_token"]
                    }).to_string()
                }
            ]))
            .expect_calls(1)
            .and()
            // Mock 3: Authorization endpoint
            .on_event("http_request_received")
            .and_event_data_contains("path", "/authorize")
            .respond_with_actions(serde_json::json!([
                {
                    "type": "send_http_response",
                    "status": 302,
                    "headers": {
                        "Location": "http://localhost:9999/callback?code=AUTH_CODE_123&state=random_state_123"
                    },
                    "body": ""
                }
            ]))
            .expect_calls(1)
            .and()
            // Mock 4: Token endpoint
            .on_event("http_request_received")
            .and_event_data_contains("path", "/token")
            .respond_with_actions(serde_json::json!([
                {
                    "type": "send_http_response",
                    "status": 200,
                    "headers": {"Content-Type": "application/json"},
                    "body": serde_json::json!({
                        "access_token": "ACCESS_TOKEN_XYZ",
                        "token_type": "Bearer",
                        "id_token": "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJ1c2VyMTIzIiwibmFtZSI6IkpvaG4gRG9lIiwiZW1haWwiOiJqb2huQGV4YW1wbGUuY29tIiwiaWF0IjoxNjAwMDAwMDAwLCJleHAiOjE2MDAwMDM2MDB9.signature",
                        "expires_in": 3600,
                        "scope": "openid profile email"
                    }).to_string()
                }
            ]))
            .expect_calls(1)
            .and()
            // Mock 5: UserInfo endpoint
            .on_event("http_request_received")
            .and_event_data_contains("path", "/userinfo")
            .respond_with_actions(serde_json::json!([
                {
                    "type": "send_http_response",
                    "status": 200,
                    "headers": {"Content-Type": "application/json"},
                    "body": serde_json::json!({
                        "sub": "user123",
                        "name": "John Doe",
                        "email": "john@example.com",
                        "email_verified": true
                    }).to_string()
                }
            ]))
            .expect_calls(1)
            .and()
            // Mock 6: JWKS endpoint
            .on_event("http_request_received")
            .and_event_data_contains("path", "/jwks.json")
            .respond_with_actions(serde_json::json!([
                {
                    "type": "send_http_response",
                    "status": 200,
                    "headers": {"Content-Type": "application/json"},
                    "body": serde_json::json!({
                        "keys": [
                            {
                                "kty": "RSA",
                                "use": "sig",
                                "kid": "key1",
                                "alg": "RS256",
                                "n": "0vx7agoebGcQve...",
                                "e": "AQAB"
                            }
                        ]
                    }).to_string()
                }
            ]))
            .expect_calls(1)
            .and()
    });

    let mut server = start_netget_server(server_config).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_millis(500)).await;

    let client = reqwest::Client::new();

    // Test 1: Discovery endpoint
    println!("Testing discovery endpoint...");
    let discovery_resp = client
        .get(format!(
            "http://localhost:{}/.well-known/openid-configuration",
            server.port
        ))
        .send()
        .await?;

    assert_eq!(discovery_resp.status(), 200, "Discovery should return 200");
    let discovery: Value = discovery_resp.json().await?;

    assert!(
        discovery["issuer"].as_str().unwrap().contains("localhost"),
        "Issuer should contain localhost"
    );
    assert!(
        discovery["authorization_endpoint"].as_str().is_some(),
        "Authorization endpoint should be present"
    );
    assert!(
        discovery["token_endpoint"].as_str().is_some(),
        "Token endpoint should be present"
    );
    println!("✓ Discovery endpoint works");

    // Test 2: Authorization endpoint (redirect flow)
    println!("Testing authorization endpoint...");
    let auth_resp = client
        .get(format!("http://localhost:{}/authorize", server.port))
        .query(&[
            ("response_type", "code"),
            ("client_id", "test_client"),
            ("redirect_uri", "http://localhost:9999/callback"),
            ("scope", "openid profile email"),
            ("state", "random_state_123"),
        ])
        .send()
        .await?;

    assert_eq!(
        auth_resp.status(),
        302,
        "Authorization should return 302 redirect"
    );

    let location = auth_resp
        .headers()
        .get("location")
        .expect("Missing Location header")
        .to_str()?;

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
        .post(format!("http://localhost:{}/token", server.port))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", "AUTH_CODE_123"),
            ("redirect_uri", "http://localhost:9999/callback"),
            ("client_id", "test_client"),
        ])
        .send()
        .await?;

    assert_eq!(token_resp.status(), 200, "Token should return 200");
    let token: Value = token_resp.json().await?;

    assert_eq!(token["access_token"], "ACCESS_TOKEN_XYZ");
    assert_eq!(token["token_type"], "Bearer");
    assert!(token["id_token"].is_string(), "ID token should be present");
    assert_eq!(token["expires_in"], 3600);
    assert_eq!(token["scope"], "openid profile email");
    println!("✓ Token endpoint works");

    // Test 4: UserInfo endpoint
    println!("Testing userinfo endpoint...");
    let userinfo_resp = client
        .get(format!("http://localhost:{}/userinfo", server.port))
        .header("Authorization", "Bearer ACCESS_TOKEN_XYZ")
        .send()
        .await?;

    assert_eq!(userinfo_resp.status(), 200, "UserInfo should return 200");
    let userinfo: Value = userinfo_resp.json().await?;

    assert_eq!(userinfo["sub"], "user123");
    assert_eq!(userinfo["name"], "John Doe");
    assert_eq!(userinfo["email"], "john@example.com");
    assert_eq!(userinfo["email_verified"], true);
    println!("✓ UserInfo endpoint works");

    // Test 5: JWKS endpoint
    println!("Testing JWKS endpoint...");
    let jwks_resp = client
        .get(format!("http://localhost:{}/jwks.json", server.port))
        .send()
        .await?;

    assert_eq!(jwks_resp.status(), 200, "JWKS should return 200");
    let jwks: Value = jwks_resp.json().await?;

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

    // Verify mock expectations
    server.verify_mocks().await?;

    // Cleanup
    server.stop().await?;

    Ok(())
}

/// Test OpenID Connect error handling
#[tokio::test]
async fn test_openid_error_handling() -> E2EResult<()> {
    println!("\n=== E2E Test: OpenID Connect Error Handling ===");

    let instruction = r#"OpenID Connect server with error handling.

For any request to /authorize with missing required parameters (client_id or redirect_uri):
- Respond with 400 Bad Request
- Return JSON error: {"error": "invalid_request", "error_description": "Missing required parameter"}

For any request to /token with invalid grant_type:
- Respond with 400 Bad Request
- Return JSON error: {"error": "unsupported_grant_type", "error_description": "Only authorization_code is supported"}
"#;

    let server_config = NetGetConfig::new(format!(
        "Start OpenID Connect server on port {{AVAILABLE_PORT}}. {}",
        instruction
    ))
    .with_mock(|mock| {
        mock
            // Mock 1: Server startup
            .on_instruction_containing("OpenID Connect server")
            .respond_with_actions(serde_json::json!([
                {
                    "type": "open_server",
                    "port": 0,
                    "base_stack": "http",
                    "instruction": instruction
                }
            ]))
            .expect_calls(1)
            .and()
            // Mock 2: Invalid authorization request
            .on_event("http_request_received")
            .and_event_data_contains("path", "/authorize")
            .respond_with_actions(serde_json::json!([
                {
                    "type": "send_http_response",
                    "status": 400,
                    "headers": {"Content-Type": "application/json"},
                    "body": serde_json::json!({
                        "error": "invalid_request",
                        "error_description": "Missing required parameter"
                    }).to_string()
                }
            ]))
            .expect_calls(1)
            .and()
            // Mock 3: Invalid token request
            .on_event("http_request_received")
            .and_event_data_contains("path", "/token")
            .respond_with_actions(serde_json::json!([
                {
                    "type": "send_http_response",
                    "status": 400,
                    "headers": {"Content-Type": "application/json"},
                    "body": serde_json::json!({
                        "error": "unsupported_grant_type",
                        "error_description": "Only authorization_code is supported"
                    }).to_string()
                }
            ]))
            .expect_calls(1)
            .and()
    });

    let mut server = start_netget_server(server_config).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_millis(500)).await;

    let client = reqwest::Client::new();

    // Test invalid authorization request (missing client_id)
    println!("Testing invalid authorization request...");
    let resp = client
        .get(format!("http://localhost:{}/authorize", server.port))
        .query(&[("response_type", "code")])
        .send()
        .await?;

    assert_eq!(resp.status(), 400, "Should return 400 for invalid request");
    let error: Value = resp.json().await?;
    assert_eq!(error["error"], "invalid_request");
    println!("✓ Invalid authorization request handled correctly");

    // Test invalid token request (unsupported grant type)
    println!("Testing invalid token request...");
    let resp = client
        .post(format!("http://localhost:{}/token", server.port))
        .form(&[("grant_type", "client_credentials")])
        .send()
        .await?;

    assert_eq!(resp.status(), 400, "Should return 400 for invalid grant");
    let error: Value = resp.json().await?;
    assert_eq!(error["error"], "unsupported_grant_type");
    println!("✓ Invalid token request handled correctly");

    println!("\n✅ Error handling tests passed!");

    // Verify mock expectations
    server.verify_mocks().await?;

    // Cleanup
    server.stop().await?;

    Ok(())
}
