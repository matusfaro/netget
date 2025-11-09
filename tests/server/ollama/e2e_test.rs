//! End-to-end Ollama API tests for NetGet
//!
//! These tests spawn the actual NetGet binary with Ollama API prompts
//! and validate the responses using HTTP clients.

#![cfg(all(test, feature = "ollama"))]

use crate::server::helpers::{self, ServerConfig, E2EResult};
use serde_json::Value;
use std::time::Duration;

#[tokio::test]
async fn test_ollama_list_models() -> E2EResult<()> {
    println!("\n=== E2E Test: Ollama List Models ===");

    // Start Ollama-compatible server
    let prompt = "Open Ollama on port {AVAILABLE_PORT}. This is an Ollama-compatible API server. \
        When clients request GET /api/tags, list available Ollama models from the backend.";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // Wait a bit for server to be ready
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send GET /api/tags request
    println!("Sending GET /api/tags request...");

    let client = reqwest::Client::new();
    let response = match tokio::time::timeout(
        Duration::from_secs(15),
        client
            .get(format!("http://127.0.0.1:{}/api/tags", server.port))
            .send()
    ).await {
        Ok(Ok(resp)) => {
            println!("✓ Received HTTP response: {}", resp.status());
            resp
        }
        Ok(Err(e)) => {
            println!("✗ HTTP request error: {}", e);
            return Err(e.into());
        }
        Err(_) => {
            println!("✗ HTTP request timeout");
            return Err("Request timeout".into());
        }
    };

    assert_eq!(response.status(), 200, "Expected HTTP 200 OK");

    // Parse JSON response
    let json: Value = response.json().await?;
    println!("Response JSON: {}", serde_json::to_string_pretty(&json)?);

    // Validate Ollama models list format
    assert!(json.get("models").and_then(|v| v.as_array()).is_some(),
            "Expected 'models' field to be an array");

    let models = json["models"].as_array().unwrap();
    println!("✓ Found {} models", models.len());

    // Verify at least one model exists
    if !models.is_empty() {
        let first_model = &models[0];
        assert!(first_model.get("name").is_some(), "Model should have 'name' field");
        println!("✓ First model: {}", first_model.get("name").unwrap());
    }

    println!("✓ Ollama List Models test completed\n");
    Ok(())
}

#[tokio::test]
async fn test_ollama_generate() -> E2EResult<()> {
    println!("\n=== E2E Test: Ollama Generate ===");

    let prompt = "Open Ollama on port {AVAILABLE_PORT}. This is an Ollama-compatible API server. \
        When clients send POST /api/generate requests, use the backend Ollama to generate responses.";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // Wait for server to be ready
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send generate request
    println!("Sending POST /api/generate request...");

    let client = reqwest::Client::new();
    let request_body = serde_json::json!({
        "model": "qwen2.5-coder:0.5b",
        "prompt": "Say 'Hello from NetGet Ollama' and nothing else.",
        "stream": false
    });

    let response = match tokio::time::timeout(
        Duration::from_secs(30),
        client
            .post(format!("http://127.0.0.1:{}/api/generate", server.port))
            .json(&request_body)
            .send()
    ).await {
        Ok(Ok(resp)) => {
            println!("✓ Received HTTP response: {}", resp.status());
            resp
        }
        Ok(Err(e)) => {
            println!("✗ HTTP request error: {}", e);
            return Err(e.into());
        }
        Err(_) => {
            println!("✗ HTTP request timeout");
            return Err("Request timeout".into());
        }
    };

    assert_eq!(response.status(), 200, "Expected HTTP 200 OK");

    // Parse JSON response
    let json: Value = response.json().await?;
    println!("Response JSON: {}", serde_json::to_string_pretty(&json)?);

    // Validate Ollama generate response format
    assert!(json.get("model").is_some(), "Expected 'model' field");
    assert!(json.get("response").is_some(), "Expected 'response' field");
    assert_eq!(json.get("done").and_then(|v| v.as_bool()), Some(true),
               "Expected 'done' to be true");

    let response_text = json["response"].as_str().unwrap();
    println!("✓ Generated response: {}", response_text);
    assert!(!response_text.is_empty(), "Response should not be empty");

    println!("✓ Ollama Generate test completed\n");
    Ok(())
}

#[tokio::test]
async fn test_ollama_chat() -> E2EResult<()> {
    println!("\n=== E2E Test: Ollama Chat ===");

    let prompt = "Open Ollama on port {AVAILABLE_PORT}. This is an Ollama-compatible API server. \
        When clients send POST /api/chat requests, use the backend Ollama to generate chat responses.";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // Wait for server to be ready
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send chat request
    println!("Sending POST /api/chat request...");

    let client = reqwest::Client::new();
    let request_body = serde_json::json!({
        "model": "qwen2.5-coder:0.5b",
        "messages": [
            {"role": "user", "content": "Say 'NetGet Ollama Chat' and nothing else."}
        ],
        "stream": false
    });

    let response = match tokio::time::timeout(
        Duration::from_secs(30),
        client
            .post(format!("http://127.0.0.1:{}/api/chat", server.port))
            .json(&request_body)
            .send()
    ).await {
        Ok(Ok(resp)) => {
            println!("✓ Received HTTP response: {}", resp.status());
            resp
        }
        Ok(Err(e)) => {
            println!("✗ HTTP request error: {}", e);
            return Err(e.into());
        }
        Err(_) => {
            println!("✗ HTTP request timeout");
            return Err("Request timeout".into());
        }
    };

    assert_eq!(response.status(), 200, "Expected HTTP 200 OK");

    // Parse JSON response
    let json: Value = response.json().await?;
    println!("Response JSON: {}", serde_json::to_string_pretty(&json)?);

    // Validate Ollama chat response format
    assert!(json.get("model").is_some(), "Expected 'model' field");
    assert!(json.get("message").is_some(), "Expected 'message' field");
    assert_eq!(json.get("done").and_then(|v| v.as_bool()), Some(true),
               "Expected 'done' to be true");

    let message = json["message"].as_object().unwrap();
    assert_eq!(message.get("role").and_then(|v| v.as_str()), Some("assistant"),
               "Expected message role to be 'assistant'");

    let content = message.get("content").and_then(|v| v.as_str()).unwrap();
    println!("✓ Chat response: {}", content);
    assert!(!content.is_empty(), "Response content should not be empty");

    println!("✓ Ollama Chat test completed\n");
    Ok(())
}

#[tokio::test]
async fn test_ollama_invalid_endpoint() -> E2EResult<()> {
    println!("\n=== E2E Test: Ollama Invalid Endpoint ===");

    let prompt = "Open Ollama on port {AVAILABLE_PORT}. This is an Ollama-compatible API server.";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_millis(500)).await;

    // Request non-existent endpoint
    println!("Sending GET /api/nonexistent request...");

    let client = reqwest::Client::new();
    let response = client
        .get(format!("http://127.0.0.1:{}/api/nonexistent", server.port))
        .send()
        .await?;

    println!("✓ Received HTTP response: {}", response.status());

    // Should return 404 Not Found
    assert_eq!(response.status(), 404, "Expected HTTP 404 Not Found");

    let json: Value = response.json().await?;
    println!("Response JSON: {}", serde_json::to_string_pretty(&json)?);

    assert!(json.get("error").is_some(), "Expected 'error' field in response");

    println!("✓ Ollama Invalid Endpoint test completed\n");
    Ok(())
}
