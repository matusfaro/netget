//! End-to-end HTTP/2 tests for NetGet
//!
//! These tests spawn the actual NetGet binary with HTTP/2 prompts
//! and validate the responses using real HTTP/2 clients (reqwest).

#![cfg(feature = "http2")]

use super::super::helpers::{self, E2EResult, ServerConfig};
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_http2_basic_get_requests() -> E2EResult<()> {
    println!("\n=== E2E Test: HTTP/2 Basic GET Requests ===");

    // PROMPT: HTTP/2 server with multiple routes
    let prompt = r#"Start an HTTP/2 server on port {AVAILABLE_PORT}.
For GET /, return 200 with body: "Welcome to HTTP/2"
For GET /api/users, return 200 with JSON: {"users": ["Alice", "Bob"]}
For GET /api/status, return 200 with JSON: {"status": "ok", "protocol": "HTTP/2"}
For any other path, return 404 with body: "Not Found"
Set Content-Type header appropriately (text/plain for /, application/json for /api/*)."#;

    // Start the server
    let server =
        helpers::start_netget_server(ServerConfig::new_no_scripts(prompt.to_string())).await?;
    println!(
        "Server started: {} stack on port {}",
        server.stack, server.port
    );

    // Verify it's actually an HTTP/2 server
    assert!(
        server.stack.to_uppercase().contains("HTTP") && server.stack.contains("2"),
        "Expected HTTP/2 server but got {}",
        server.stack
    );

    // Give server time to initialize
    sleep(Duration::from_secs(2)).await;

    // Create HTTP/2 client (with prior knowledge - no TLS, direct HTTP/2)
    let client = reqwest::Client::builder().http2_prior_knowledge().build()?;

    // Test 1: GET /
    println!("Test 1: GET /");
    let response = client
        .get(format!("http://127.0.0.1:{}/", server.port))
        .send()
        .await?;

    assert_eq!(response.status(), 200, "Expected 200 OK for /");
    assert_eq!(
        response.version(),
        reqwest::Version::HTTP_2,
        "Expected HTTP/2"
    );
    let body = response.text().await?;
    assert!(
        body.contains("Welcome to HTTP/2"),
        "Expected welcome message, got: {}",
        body
    );
    println!("✓ GET / returned 200 with welcome message");

    // Test 2: GET /api/users
    println!("Test 2: GET /api/users");
    let response = client
        .get(format!("http://127.0.0.1:{}/api/users", server.port))
        .send()
        .await?;

    assert_eq!(response.status(), 200, "Expected 200 OK for /api/users");
    assert_eq!(
        response.version(),
        reqwest::Version::HTTP_2,
        "Expected HTTP/2"
    );
    let body = response.text().await?;
    let json: serde_json::Value = serde_json::from_str(&body)?;
    assert!(
        json.get("users").is_some(),
        "Expected 'users' field in response"
    );
    println!("✓ GET /api/users returned JSON with users");

    // Test 3: GET /api/status
    println!("Test 3: GET /api/status");
    let response = client
        .get(format!("http://127.0.0.1:{}/api/status", server.port))
        .send()
        .await?;

    assert_eq!(response.status(), 200, "Expected 200 OK for /api/status");
    assert_eq!(
        response.version(),
        reqwest::Version::HTTP_2,
        "Expected HTTP/2"
    );
    let body = response.text().await?;
    let json: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(json.get("status").and_then(|v| v.as_str()), Some("ok"));
    println!("✓ GET /api/status returned status: ok");

    // Test 4: GET /nonexistent (404)
    println!("Test 4: GET /nonexistent (404)");
    let response = client
        .get(format!("http://127.0.0.1:{}/nonexistent", server.port))
        .send()
        .await?;

    assert_eq!(response.status(), 404, "Expected 404 for /nonexistent");
    assert_eq!(
        response.version(),
        reqwest::Version::HTTP_2,
        "Expected HTTP/2"
    );
    let body = response.text().await?;
    assert!(body.contains("Not Found"), "Expected 'Not Found' message");
    println!("✓ GET /nonexistent returned 404");

    // Stop the server
    server.stop().await?;
    println!("✓ All tests passed!");

    Ok(())
}

#[tokio::test]
async fn test_http2_post_with_body() -> E2EResult<()> {
    println!("\n=== E2E Test: HTTP/2 POST with Body ===");

    // PROMPT: HTTP/2 server that echoes POST bodies
    let prompt = r#"Start an HTTP/2 server on port {AVAILABLE_PORT}.
For POST /echo, return 200 with JSON containing the request body and method.
For POST /api/users, parse the JSON body and return 201 with a success message including the name from the request.
Set Content-Type: application/json for all responses."#;

    // Start the server
    let server =
        helpers::start_netget_server(ServerConfig::new_no_scripts(prompt.to_string())).await?;
    println!("Server started on port {}", server.port);

    // Give server time to initialize
    sleep(Duration::from_secs(2)).await;

    // Create HTTP/2 client
    let client = reqwest::Client::builder().http2_prior_knowledge().build()?;

    // Test 1: POST /echo with text body
    println!("Test 1: POST /echo with text body");
    let response = client
        .post(format!("http://127.0.0.1:{}/echo", server.port))
        .body("Hello HTTP/2")
        .send()
        .await?;

    assert_eq!(response.status(), 200, "Expected 200 OK for POST /echo");
    assert_eq!(
        response.version(),
        reqwest::Version::HTTP_2,
        "Expected HTTP/2"
    );
    let body = response.text().await?;
    assert!(
        body.contains("Hello HTTP/2") || body.contains("POST"),
        "Expected echo response containing request data"
    );
    println!("✓ POST /echo returned response with request data");

    // Test 2: POST /api/users with JSON
    println!("Test 2: POST /api/users with JSON");
    let user_data = serde_json::json!({
        "name": "Charlie",
        "email": "charlie@example.com"
    });

    let response = client
        .post(format!("http://127.0.0.1:{}/api/users", server.port))
        .json(&user_data)
        .send()
        .await?;

    assert_eq!(
        response.status(),
        201,
        "Expected 201 Created for POST /api/users"
    );
    assert_eq!(
        response.version(),
        reqwest::Version::HTTP_2,
        "Expected HTTP/2"
    );
    let body = response.text().await?;
    // LLM should include the name "Charlie" in the response
    assert!(
        body.contains("Charlie") || body.to_lowercase().contains("success"),
        "Expected response containing user name or success message, got: {}",
        body
    );
    println!("✓ POST /api/users returned 201 with success message");

    // Stop the server
    server.stop().await?;
    println!("✓ All tests passed!");

    Ok(())
}

#[tokio::test]
async fn test_http2_multiplexing() -> E2EResult<()> {
    println!("\n=== E2E Test: HTTP/2 Multiplexing (Concurrent Requests) ===");

    // PROMPT: HTTP/2 server with simple response
    let prompt = r#"Start an HTTP/2 server on port {AVAILABLE_PORT}.
For GET /data, return 200 with JSON: {"data": "test", "timestamp": "2025-01-01T00:00:00Z"}
Set Content-Type: application/json."#;

    // Start the server
    let server =
        helpers::start_netget_server(ServerConfig::new_no_scripts(prompt.to_string())).await?;
    println!("Server started on port {}", server.port);

    // Give server time to initialize
    sleep(Duration::from_secs(2)).await;

    // Create HTTP/2 client (reuses connection for multiplexing)
    let client = reqwest::Client::builder().http2_prior_knowledge().build()?;

    // Send 3 concurrent requests over the same connection
    println!("Sending 3 concurrent requests...");
    let url = format!("http://127.0.0.1:{}/data", server.port);

    let (resp1, resp2, resp3) = tokio::join!(
        client.get(&url).send(),
        client.get(&url).send(),
        client.get(&url).send(),
    );

    // All requests should succeed
    let resp1 = resp1?;
    let resp2 = resp2?;
    let resp3 = resp3?;

    assert_eq!(resp1.status(), 200, "Request 1 should succeed");
    assert_eq!(resp2.status(), 200, "Request 2 should succeed");
    assert_eq!(resp3.status(), 200, "Request 3 should succeed");

    // All should be HTTP/2
    assert_eq!(resp1.version(), reqwest::Version::HTTP_2);
    assert_eq!(resp2.version(), reqwest::Version::HTTP_2);
    assert_eq!(resp3.version(), reqwest::Version::HTTP_2);

    println!("✓ All 3 concurrent requests succeeded via HTTP/2 multiplexing");

    // Stop the server
    server.stop().await?;
    println!("✓ All tests passed!");

    Ok(())
}
