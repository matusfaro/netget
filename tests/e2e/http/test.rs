//! End-to-end HTTP tests for NetGet
//!
//! These tests spawn the actual NetGet binary with HTTP prompts
//! and validate the responses using real HTTP clients.

#![cfg(feature = "e2e-tests")]

// Helper module imported from parent

use super::super::helpers::{self, ServerConfig, E2EResult};

#[tokio::test]
async fn test_http_simple_get() -> E2EResult<()> {
    println!("\n=== E2E Test: Simple HTTP GET ===");

    // PROMPT: Simple HTML response
    // Get an available port first (since port 0 has issues in non-interactive mode)
    let port = helpers::get_available_port().await?;
    let prompt = format!("listen on port {} via http stack. For any GET request, return status 200 with body: <h1>Hello World</h1>", port);

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started: {} stack on port {}", server.stack, server.port);

    // Verify it's actually an HTTP server
    assert_eq!(server.stack, "HTTP", "Expected HTTP server but got {}", server.stack);

    // VALIDATION: Make request and check response
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/", server.port);

    let response = client.get(&url).send().await?;

    assert_eq!(response.status(), 200);
    let body = response.text().await?;
    assert!(body.contains("Hello World"));

    println!("✓ Response validated");
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_http_json_api() -> E2EResult<()> {
    println!("\n=== E2E Test: JSON API ===");

    // PROMPT: JSON API response
    let port = helpers::get_available_port().await?;
    let prompt = format!(r#"listen on port {} via http stack. For any POST to /api/data, return status 201 with Content-Type: application/json and body: {{"status": "created", "id": 123}}"#, port);

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Make POST request
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/api/data", server.port);

    let response = client
        .post(&url)
        .json(&serde_json::json!({"name": "test"}))
        .send()
        .await?;

    assert_eq!(response.status(), 201);

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(content_type.contains("json"));

    let json: serde_json::Value = response.json().await?;
    assert_eq!(json["status"], "created");
    assert_eq!(json["id"], 123);

    println!("✓ JSON response validated");
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_http_routing() -> E2EResult<()> {
    println!("\n=== E2E Test: HTTP Routing ===");

    // PROMPT: Route-based responses
    let port = helpers::get_available_port().await?;
    let prompt = format!("listen on port {} via http stack. For GET /home return 'Welcome Home'. For GET /about return 'About Us'. For other paths return 404 with 'Not Found'", port);

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    let client = reqwest::Client::new();

    // Test /home route
    let response = client
        .get(&format!("http://127.0.0.1:{}/home", server.port))
        .send()
        .await?;
    assert_eq!(response.status(), 200);
    let body = response.text().await?;
    assert!(body.contains("Welcome") || body.contains("Home"));
    println!("✓ /home route works");

    // Test /about route
    let response = client
        .get(&format!("http://127.0.0.1:{}/about", server.port))
        .send()
        .await?;
    assert_eq!(response.status(), 200);
    let body = response.text().await?;
    assert!(body.contains("About"));
    println!("✓ /about route works");

    // Test 404 for unknown route
    let response = client
        .get(&format!("http://127.0.0.1:{}/unknown", server.port))
        .send()
        .await?;
    assert_eq!(response.status(), 404);
    let body = response.text().await?;
    assert!(body.contains("Not Found") || body.contains("not found"));
    println!("✓ 404 response works");

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_http_headers() -> E2EResult<()> {
    println!("\n=== E2E Test: Custom Headers ===");

    // PROMPT: Custom headers in response
    let port = helpers::get_available_port().await?;
    let prompt = format!("listen on port {} via http stack. For GET /api return status 200 with headers: X-API-Version: 1.0, X-Custom: test-value, and body: API Response", port);

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Check headers
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/api", server.port);

    let response = client.get(&url).send().await?;

    assert_eq!(response.status(), 200);

    // Check custom headers (case-insensitive)
    let headers = response.headers();

    let api_version = headers
        .get("x-api-version")
        .and_then(|v| v.to_str().ok());
    assert_eq!(api_version, Some("1.0"));

    let custom = headers
        .get("x-custom")
        .and_then(|v| v.to_str().ok());
    assert_eq!(custom, Some("test-value"));

    let body = response.text().await?;
    assert!(body.contains("API Response") || body.contains("API"));

    println!("✓ Custom headers validated");
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_http_methods() -> E2EResult<()> {
    println!("\n=== E2E Test: HTTP Methods ===");

    // PROMPT: Different responses for different methods
    let port = helpers::get_available_port().await?;
    let prompt = format!("listen on port {} via http stack. For GET return 'GET Response'. For POST return 'POST Response'. For PUT return 'PUT Response'. For DELETE return 'DELETE Response'", port);

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/", server.port);

    // Test GET
    let response = client.get(&url).send().await?;
    assert_eq!(response.status(), 200);
    let body = response.text().await?;
    assert!(body.contains("GET"));
    println!("✓ GET method works");

    // Test POST
    let response = client.post(&url).send().await?;
    assert_eq!(response.status(), 200);
    let body = response.text().await?;
    assert!(body.contains("POST"));
    println!("✓ POST method works");

    // Test PUT
    let response = client.put(&url).send().await?;
    assert_eq!(response.status(), 200);
    let body = response.text().await?;
    assert!(body.contains("PUT"));
    println!("✓ PUT method works");

    // Test DELETE
    let response = client.delete(&url).send().await?;
    assert_eq!(response.status(), 200);
    let body = response.text().await?;
    assert!(body.contains("DELETE"));
    println!("✓ DELETE method works");

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_http_error_responses() -> E2EResult<()> {
    println!("\n=== E2E Test: Error Responses ===");

    // PROMPT: Various error codes
    let port = helpers::get_available_port().await?;
    let prompt = format!("listen on port {} via http stack. For GET /forbidden return 403 with 'Access Denied'. For GET /error return 500 with 'Server Error'. For GET /redirect return 301 with Location header: /home", port);

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // Don't follow redirects for this test
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    // Test 403 Forbidden
    let response = client
        .get(&format!("http://127.0.0.1:{}/forbidden", server.port))
        .send()
        .await?;
    assert_eq!(response.status(), 403);
    let body = response.text().await?;
    assert!(body.contains("Denied") || body.contains("denied") || body.contains("Forbidden"));
    println!("✓ 403 response works");

    // Test 500 Error
    let response = client
        .get(&format!("http://127.0.0.1:{}/error", server.port))
        .send()
        .await?;
    assert_eq!(response.status(), 500);
    let body = response.text().await?;
    assert!(body.contains("Error") || body.contains("error"));
    println!("✓ 500 response works");

    // Test 301 Redirect
    let response = client
        .get(&format!("http://127.0.0.1:{}/redirect", server.port))
        .send()
        .await?;
    assert_eq!(response.status(), 301);
    let location = response
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok());
    assert_eq!(location, Some("/home"));
    println!("✓ 301 redirect works");

    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

// Remove the ctor/dtor functions to avoid the panic issue
// Tests will handle their own cleanup