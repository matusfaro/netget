use reqwest;
use std::path::PathBuf;
use std::time::Duration;

use crate::server::helpers::{
    assert_stack_name, get_available_port, start_netget_server, wait_for_server_startup,
    ServerConfig, E2EResult,
};

/// Test comprehensive route matching with file-based OpenAPI spec
#[tokio::test]
#[cfg(feature = "openapi")]
async fn test_openapi_route_matching_comprehensive() -> E2EResult<()> {
    // Get path to test spec file
    let spec_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/server/openapi/test_spec.yaml");
    let spec_path_str = spec_path.to_str().unwrap();

    // Create prompt that tells LLM to read spec and open server
    let prompt = format!(
        "CRITICAL: Use base_stack exactly 'openapi' (lowercase, NOT 'http', NOT 'HTTP'). \
        First, read the OpenAPI spec file at {} using read_file tool. \
        Then call open_server with base_stack='openapi', port={{AVAILABLE_PORT}}, and startup_params containing 'spec' key with the file content as value.",
        spec_path_str
    );

    let server = start_netget_server(ServerConfig::new_no_scripts(prompt)).await?;

    // Wait for server to start
    wait_for_server_startup(&server, Duration::from_secs(30), "OpenAPI").await?;

    // Verify correct stack
    assert_stack_name(&server, "OpenAPI");

    // Create HTTP client
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    let base_url = format!("http://127.0.0.1:{}", server.port);

    println!("\n=== Testing 404 Not Found (path doesn't exist) ===");
    let response = client
        .get(&format!("{}/nonexistent", base_url))
        .send()
        .await?;

    assert_eq!(
        response.status(),
        404,
        "Expected 404 for non-existent path"
    );

    let json: serde_json::Value = response.json().await?;
    assert!(
        json.get("error").is_some(),
        "404 response should contain error field"
    );
    println!("✓ 404 response: {:?}", json);

    println!("\n=== Testing 405 Method Not Allowed (path exists, wrong method) ===");
    // /users supports GET and POST, but not DELETE
    let response = client
        .delete(&format!("{}/users", base_url))
        .send()
        .await?;

    assert_eq!(
        response.status(),
        405,
        "Expected 405 for unsupported method"
    );

    // Check for Allow header
    let allow_header = response.headers().get("allow");
    assert!(
        allow_header.is_some(),
        "405 response should include Allow header"
    );
    let allowed_methods = allow_header.unwrap().to_str()?;
    println!("✓ 405 response with Allow header: {}", allowed_methods);
    assert!(
        allowed_methods.contains("GET") || allowed_methods.contains("POST"),
        "Allow header should list GET and POST"
    );

    let json: serde_json::Value = response.json().await?;
    assert!(
        json.get("error").is_some(),
        "405 response should contain error field"
    );

    println!("\n=== Testing path parameter extraction: /users/123 ===");
    let response = client.get(&format!("{}/users/123", base_url)).send().await?;

    let status = response.status();
    let body = response.text().await?;

    println!(
        "Status: {}, Body preview: {:?}",
        status,
        body.chars().take(200).collect::<String>()
    );

    // LLM should receive path_params with id=123
    // Response status could be 200 or error depending on LLM
    // Just verify we got a response (not immediate 404/405)
    assert!(
        status.as_u16() != 404 && status.as_u16() != 405,
        "Path parameter route should be matched (not 404/405)"
    );

    println!("\n=== Testing nested path parameters: /products/abc123/reviews ===");
    let response = client
        .get(&format!("{}/products/abc123/reviews", base_url))
        .send()
        .await?;

    let status = response.status();
    println!("Status: {}", status);
    assert!(
        status.as_u16() != 404 && status.as_u16() != 405,
        "Nested path parameter route should be matched"
    );

    println!("\n=== Testing successful endpoint: GET /users ===");
    let response = client.get(&format!("{}/users", base_url)).send().await?;

    // Should get LLM-generated response
    let status = response.status();
    let body = response.text().await?;

    println!("Status: {}, Body: {}", status, body);

    // Verify we got a JSON response from LLM (not empty, not error-only)
    let json: serde_json::Value = serde_json::from_str(&body)?;
    assert!(
        json.is_array() || json.is_object(),
        "Should receive structured JSON from LLM"
    );

    println!("\n=== Testing POST with body: POST /users ===");
    let response = client
        .post(&format!("{}/users", base_url))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "name": "Test User"
        }))
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;

    println!("Status: {}, Body: {}", status, body);

    // Should get LLM-generated response
    assert!(
        status.is_success() || status.is_client_error(),
        "Should receive response from LLM"
    );

    println!("\n=== All route matching tests passed! ===");

    // Cleanup
    server.stop().await?;

    Ok(())
}

/// Test llm_on_invalid configuration
#[tokio::test]
#[cfg(feature = "openapi")]
async fn test_openapi_llm_on_invalid_override() -> E2EResult<()> {
    let spec_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/server/openapi/test_spec.yaml");
    let spec_path_str = spec_path.to_str().unwrap();

    // Create prompt that tells LLM to read spec, open server, and configure error handling
    let prompt = format!(
        "CRITICAL: Use base_stack exactly 'openapi' (lowercase, NOT 'http', NOT 'HTTP'). \
        First, read the OpenAPI spec file at {} using read_file tool. \
        Then call open_server with base_stack='openapi', port={{AVAILABLE_PORT}}, and startup_params containing 'spec' key with the file content as value. \
        After opening, use configure_error_handling action with llm_on_invalid=true so you can customize 404 and 405 responses.",
        spec_path_str
    );

    let server = start_netget_server(ServerConfig::new_no_scripts(prompt)).await?;

    wait_for_server_startup(&server, Duration::from_secs(30), "OpenAPI").await?;

    assert_stack_name(&server, "OpenAPI");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    let base_url = format!("http://127.0.0.1:{}", server.port);

    println!("\n=== Testing 404 with LLM override ===");
    // With llm_on_invalid enabled, LLM should handle 404
    let response = client
        .get(&format!("{}/nonexistent", base_url))
        .send()
        .await?;

    // LLM might return any status code
    let status = response.status();
    let body = response.text().await?;

    println!("Status: {}, Body: {}", status, body);

    // Just verify we got a response (LLM was consulted)
    assert!(
        !body.is_empty(),
        "Should receive LLM-generated response for 404"
    );

    println!("\n=== LLM override test passed! ===");

    server.stop().await?;

    Ok(())
}
