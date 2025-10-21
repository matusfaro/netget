//! HTTP Integration Tests
//!
//! Black-box tests that use prompts to configure the LLM-controlled HTTP server.
//! Each test provides a prompt and validates the behavior using a real HTTP client.

mod common;

use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_http_get_html() {
    println!("\n=== Testing GET HTML via HTTP/LLM ===");

    // PROMPT: Tell the LLM to return HTML
    let prompt = "listen on port 0 via http. For any GET request, return an HTML page with status 200, Content-Type: text/html, and body: <html><body><h1>Hello from LLM!</h1></body></html>";

    // Start server
    let (_state, port, _handle) = common::start_server_with_prompt(prompt).await;

    println!("Server started on port {}", port);
    sleep(Duration::from_millis(500)).await;

    // VALIDATION: Make GET request and verify HTML response
    let url = format!("http://127.0.0.1:{}/", port);
    let client = reqwest::Client::new();

    println!("Making GET request to {}", url);
    let response = client
        .get(&url)
        .send()
        .await
        .expect("Failed to make request");

    // Verify status code
    assert_eq!(response.status(), 200, "Expected status 200");
    println!("✓ Status: 200");

    // Verify Content-Type
    let content_type = response
        .headers()
        .get("content-type")
        .expect("Missing Content-Type header")
        .to_str()
        .unwrap();
    assert!(
        content_type.contains("text/html"),
        "Expected HTML content type, got: {}",
        content_type
    );
    println!("✓ Content-Type: {}", content_type);

    // Verify body
    let body = response.text().await.expect("Failed to read body");
    println!("Body: {}", body);
    assert!(
        body.contains("<h1>Hello from LLM!</h1>"),
        "Expected HTML content"
    );
    println!("✓ HTML content verified");

    println!("=== GET HTML test passed ===\n");
}

#[tokio::test]
async fn test_http_post_json() {
    println!("\n=== Testing POST JSON via HTTP/LLM ===");

    // PROMPT: Tell the LLM to return JSON
    let prompt = "listen on port 0 via http. For any POST request, return a JSON response with status 200, Content-Type: application/json, and body: {\"status\": \"success\", \"message\": \"Data received\"}";

    // Start server
    let (_state, port, _handle) = common::start_server_with_prompt(prompt).await;

    println!("Server started on port {}", port);
    sleep(Duration::from_millis(500)).await;

    // VALIDATION: Make POST request and verify JSON response
    let url = format!("http://127.0.0.1:{}/api/data", port);
    let client = reqwest::Client::new();

    println!("Making POST request to {}", url);
    let response = client
        .post(&url)
        .json(&serde_json::json!({
            "key": "value",
            "number": 42
        }))
        .send()
        .await
        .expect("Failed to make request");

    // Verify status code
    assert_eq!(response.status(), 200, "Expected status 200");
    println!("✓ Status: 200");

    // Verify Content-Type
    let content_type = response
        .headers()
        .get("content-type")
        .expect("Missing Content-Type header")
        .to_str()
        .unwrap();
    assert!(
        content_type.contains("application/json"),
        "Expected JSON content type, got: {}",
        content_type
    );
    println!("✓ Content-Type: {}", content_type);

    // Verify body
    let body = response.text().await.expect("Failed to read body");
    println!("Body: {}", body);

    let json: serde_json::Value = serde_json::from_str(&body).expect("Invalid JSON response");
    assert_eq!(json["status"], "success");
    assert_eq!(json["message"], "Data received");
    println!("✓ JSON content verified");

    println!("=== POST JSON test passed ===\n");
}

#[tokio::test]
async fn test_http_custom_headers() {
    println!("\n=== Testing Custom Headers via HTTP/LLM ===");

    // PROMPT: Tell the LLM to return custom headers
    let prompt = "listen on port 0 via http. For any request to /custom, return status 201, with headers: X-Custom-Header: test-value and X-Request-ID: 12345, and body: Custom response";

    // Start server
    let (_state, port, _handle) = common::start_server_with_prompt(prompt).await;

    println!("Server started on port {}", port);
    sleep(Duration::from_millis(500)).await;

    // VALIDATION: Verify custom headers
    let url = format!("http://127.0.0.1:{}/custom", port);
    let client = reqwest::Client::new();

    println!("Making GET request to {}", url);
    let response = client
        .get(&url)
        .send()
        .await
        .expect("Failed to make request");

    // Verify status code
    assert_eq!(response.status(), 201, "Expected status 201");
    println!("✓ Status: 201");

    // Verify custom headers
    let custom_header = response
        .headers()
        .get("x-custom-header")
        .expect("Missing X-Custom-Header")
        .to_str()
        .unwrap();
    assert_eq!(custom_header, "test-value");
    println!("✓ X-Custom-Header: {}", custom_header);

    let request_id = response
        .headers()
        .get("x-request-id")
        .expect("Missing X-Request-ID")
        .to_str()
        .unwrap();
    assert_eq!(request_id, "12345");
    println!("✓ X-Request-ID: {}", request_id);

    // Verify body
    let body = response.text().await.expect("Failed to read body");
    println!("Body: {}", body);
    assert_eq!(body, "Custom response");
    println!("✓ Body verified");

    println!("=== Custom Headers test passed ===\n");
}

#[tokio::test]
async fn test_http_404() {
    println!("\n=== Testing 404 Response via HTTP/LLM ===");

    // PROMPT: Tell the LLM to return 404 for specific path
    let prompt =
        "listen on port 0 via http. For any request to /notfound, return status 404 with body: Page not found";

    // Start server
    let (_state, port, _handle) = common::start_server_with_prompt(prompt).await;

    println!("Server started on port {}", port);
    sleep(Duration::from_millis(500)).await;

    // VALIDATION: Verify 404 response
    let url = format!("http://127.0.0.1:{}/notfound", port);
    let client = reqwest::Client::new();

    println!("Making GET request to {}", url);
    let response = client
        .get(&url)
        .send()
        .await
        .expect("Failed to make request");

    // Verify status code
    assert_eq!(response.status(), 404, "Expected status 404");
    println!("✓ Status: 404");

    // Verify body
    let body = response.text().await.expect("Failed to read body");
    println!("Body: {}", body);
    assert_eq!(body, "Page not found");
    println!("✓ Body verified");

    println!("=== 404 test passed ===\n");
}

#[tokio::test]
async fn test_http_routing() {
    println!("\n=== Testing Route-based Responses via HTTP/LLM ===");

    // PROMPT: Tell the LLM to handle different routes
    let prompt = "listen on port 0 via http. For GET /home, return 'Home Page'. For GET /about, return 'About Page'. For anything else, return 404 with 'Not Found'";

    // Start server
    let (_state, port, _handle) = common::start_server_with_prompt(prompt).await;

    println!("Server started on port {}", port);
    sleep(Duration::from_millis(500)).await;

    let client = reqwest::Client::new();

    // Test /home route
    println!("Testing /home route...");
    let response = client
        .get(&format!("http://127.0.0.1:{}/home", port))
        .send()
        .await
        .expect("Failed to make request");
    assert_eq!(response.status(), 200);
    let body = response.text().await.unwrap();
    assert!(body.contains("Home"), "Expected 'Home Page', got: {}", body);
    println!("✓ /home route works");

    // Test /about route
    println!("Testing /about route...");
    let response = client
        .get(&format!("http://127.0.0.1:{}/about", port))
        .send()
        .await
        .expect("Failed to make request");
    assert_eq!(response.status(), 200);
    let body = response.text().await.unwrap();
    assert!(body.contains("About"), "Expected 'About Page', got: {}", body);
    println!("✓ /about route works");

    // Test unknown route
    println!("Testing unknown route...");
    let response = client
        .get(&format!("http://127.0.0.1:{}/unknown", port))
        .send()
        .await
        .expect("Failed to make request");
    assert_eq!(response.status(), 404);
    let body = response.text().await.unwrap();
    assert!(
        body.contains("Not Found"),
        "Expected 'Not Found', got: {}",
        body
    );
    println!("✓ Unknown route returns 404");

    println!("=== Routing test passed ===\n");
}
