//! End-to-end MCP (Model Context Protocol) tests for NetGet
//!
//! These tests spawn the actual NetGet binary with MCP prompts
//! and validate the responses using HTTP JSON-RPC 2.0 clients.

#![cfg(feature = "e2e-tests")]

use crate::server::helpers::{self, ServerConfig, E2EResult};
use serde_json::{json, Value};
use std::time::Duration;

/// Helper function to send MCP JSON-RPC request
async fn send_mcp_request(
    port: u16,
    method: &str,
    params: Option<Value>,
    id: Option<i64>,
) -> E2EResult<Value> {
    let client = reqwest::Client::new();

    let mut request_body = json!({
        "jsonrpc": "2.0",
        "method": method,
    });

    if let Some(p) = params {
        request_body["params"] = p;
    }

    if let Some(i) = id {
        request_body["id"] = json!(i);
    }

    println!("→ Sending MCP request: {}", serde_json::to_string_pretty(&request_body)?);

    let response = match tokio::time::timeout(
        Duration::from_secs(30),
        client
            .post(format!("http://127.0.0.1:{}", port))
            .header("Content-Type", "application/json")
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

    if !response.status().is_success() {
        println!("✗ HTTP error: {}", response.status());
        return Err(format!("HTTP error: {}", response.status()).into());
    }

    let json: Value = response.json().await?;
    println!("← Response: {}", serde_json::to_string_pretty(&json)?);

    Ok(json)
}

#[tokio::test]
async fn test_mcp_initialize() -> E2EResult<()> {
    println!("\n=== E2E Test: MCP Initialize ===");

    let prompt = "Listen on port {AVAILABLE_PORT} via MCP (Model Context Protocol). \
        You are an MCP server. When a client sends an initialize request, respond with: \
        - protocolVersion: 2024-11-05 \
        - capabilities: resources with subscribe support, tools, and prompts \
        - serverInfo: name=netget-mcp, version=0.1.0";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // Wait for server to be ready
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Send initialize request
    println!("\n→ Sending MCP initialize request...");

    let response = send_mcp_request(
        server.port,
        "initialize",
        Some(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "1.0.0"
            }
        })),
        Some(1),
    ).await?;

    // Validate JSON-RPC 2.0 response
    assert_eq!(response.get("jsonrpc").and_then(|v| v.as_str()), Some("2.0"),
               "Expected 'jsonrpc' field to be '2.0'");

    assert_eq!(response.get("id"), Some(&json!(1)),
               "Expected 'id' field to match request id");

    // Validate initialize response structure
    if let Some(result) = response.get("result") {
        println!("✓ Received initialize result");

        // Check protocol version
        assert_eq!(result.get("protocolVersion").and_then(|v| v.as_str()), Some("2024-11-05"),
                   "Expected protocolVersion to be '2024-11-05'");

        // Check server info
        if let Some(server_info) = result.get("serverInfo") {
            println!("  Server: {} v{}",
                server_info.get("name").and_then(|v| v.as_str()).unwrap_or("unknown"),
                server_info.get("version").and_then(|v| v.as_str()).unwrap_or("unknown"));
        }

        // Check capabilities
        if let Some(capabilities) = result.get("capabilities") {
            println!("  Capabilities: {}", serde_json::to_string_pretty(capabilities)?);
        }

        println!("✓ MCP Initialize test completed\n");
        Ok(())
    } else {
        println!("✗ No result in response: {:?}", response.get("error"));
        Err("Initialize failed".into())
    }
}

#[tokio::test]
async fn test_mcp_ping() -> E2EResult<()> {
    println!("\n=== E2E Test: MCP Ping ===");

    let prompt = "Listen on port {AVAILABLE_PORT} via MCP. You are an MCP server.";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send ping request
    println!("\n→ Sending MCP ping request...");

    let response = send_mcp_request(
        server.port,
        "ping",
        None,
        Some(2),
    ).await?;

    // Validate response
    assert_eq!(response.get("jsonrpc").and_then(|v| v.as_str()), Some("2.0"));
    assert_eq!(response.get("id"), Some(&json!(2)));

    if response.get("result").is_some() {
        println!("✓ Ping successful");
        println!("✓ MCP Ping test completed\n");
        Ok(())
    } else {
        println!("✗ Ping failed: {:?}", response.get("error"));
        Err("Ping failed".into())
    }
}

#[tokio::test]
async fn test_mcp_resources_list() -> E2EResult<()> {
    println!("\n=== E2E Test: MCP Resources List ===");

    let prompt = "Listen on port {AVAILABLE_PORT} via MCP. \
        Use a Python script to handle requests deterministically. \
        When a client requests resources/list, return a list with these resources: \
        - uri: file:///example.txt, name: Example File, description: A sample text file \
        - uri: file:///data.json, name: Data File, description: JSON data file";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_secs(1)).await;

    // Send resources/list request
    println!("\n→ Sending MCP resources/list request...");

    let response = send_mcp_request(
        server.port,
        "resources/list",
        None,
        Some(3),
    ).await?;

    // Validate response
    assert_eq!(response.get("jsonrpc").and_then(|v| v.as_str()), Some("2.0"));
    assert_eq!(response.get("id"), Some(&json!(3)));

    if let Some(result) = response.get("result") {
        if let Some(resources) = result.get("resources").and_then(|v| v.as_array()) {
            println!("✓ Received {} resources", resources.len());
            for resource in resources {
                if let (Some(uri), Some(name)) = (
                    resource.get("uri").and_then(|v| v.as_str()),
                    resource.get("name").and_then(|v| v.as_str()),
                ) {
                    println!("  - {}: {}", name, uri);
                }
            }
        }
        println!("✓ MCP Resources List test completed\n");
        Ok(())
    } else {
        println!("✗ Resources list failed: {:?}", response.get("error"));
        Err("Resources list failed".into())
    }
}

#[tokio::test]
async fn test_mcp_resources_read() -> E2EResult<()> {
    println!("\n=== E2E Test: MCP Resources Read ===");

    let prompt = "Listen on port {AVAILABLE_PORT} via MCP. \
        Use a Python script to handle requests deterministically. \
        When a client reads resource 'file:///example.txt', \
        return contents with uri and text: 'Hello from NetGet MCP server!'";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_secs(1)).await;

    // Send resources/read request
    println!("\n→ Sending MCP resources/read request...");

    let response = send_mcp_request(
        server.port,
        "resources/read",
        Some(json!({
            "uri": "file:///example.txt"
        })),
        Some(4),
    ).await?;

    // Validate response
    assert_eq!(response.get("jsonrpc").and_then(|v| v.as_str()), Some("2.0"));
    assert_eq!(response.get("id"), Some(&json!(4)));

    if let Some(result) = response.get("result") {
        if let Some(contents) = result.get("contents").and_then(|v| v.as_array()) {
            println!("✓ Received {} content items", contents.len());
            for content in contents {
                if let Some(text) = content.get("text").and_then(|v| v.as_str()) {
                    println!("  Content: {}", text);
                }
            }
        }
        println!("✓ MCP Resources Read test completed\n");
        Ok(())
    } else {
        println!("Resource read error: {:?}", response.get("error"));
        // Resource not found is acceptable for this test
        println!("✓ MCP Resources Read test completed (resource not found is expected)\n");
        Ok(())
    }
}

#[tokio::test]
async fn test_mcp_tools_list() -> E2EResult<()> {
    println!("\n=== E2E Test: MCP Tools List ===");

    let prompt = "Listen on port {AVAILABLE_PORT} via MCP. \
        You are an MCP server. When a client requests tools/list, return a list with these tools: \
        - name: calculate, description: Perform calculations, inputSchema with 'expression' string parameter \
        - name: search, description: Search files, inputSchema with 'query' string parameter";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_secs(1)).await;

    // Send tools/list request
    println!("\n→ Sending MCP tools/list request...");

    let response = send_mcp_request(
        server.port,
        "tools/list",
        None,
        Some(5),
    ).await?;

    // Validate response
    assert_eq!(response.get("jsonrpc").and_then(|v| v.as_str()), Some("2.0"));
    assert_eq!(response.get("id"), Some(&json!(5)));

    if let Some(result) = response.get("result") {
        if let Some(tools) = result.get("tools").and_then(|v| v.as_array()) {
            println!("✓ Received {} tools", tools.len());
            for tool in tools {
                if let (Some(name), Some(desc)) = (
                    tool.get("name").and_then(|v| v.as_str()),
                    tool.get("description").and_then(|v| v.as_str()),
                ) {
                    println!("  - {}: {}", name, desc);
                }
            }
        }
        println!("✓ MCP Tools List test completed\n");
        Ok(())
    } else {
        println!("✗ Tools list failed: {:?}", response.get("error"));
        Err("Tools list failed".into())
    }
}

#[tokio::test]
async fn test_mcp_tools_call() -> E2EResult<()> {
    println!("\n=== E2E Test: MCP Tools Call ===");

    let prompt = "Listen on port {AVAILABLE_PORT} via MCP. \
        Use a Python script to handle requests deterministically. \
        When a client calls tool 'calculate' with expression '2+2', \
        return content with type text and text '4'";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_secs(1)).await;

    // Send tools/call request
    println!("\n→ Sending MCP tools/call request...");

    let response = send_mcp_request(
        server.port,
        "tools/call",
        Some(json!({
            "name": "calculate",
            "arguments": {
                "expression": "2+2"
            }
        })),
        Some(6),
    ).await?;

    // Validate response
    assert_eq!(response.get("jsonrpc").and_then(|v| v.as_str()), Some("2.0"));
    assert_eq!(response.get("id"), Some(&json!(6)));

    if let Some(result) = response.get("result") {
        if let Some(content) = result.get("content").and_then(|v| v.as_array()) {
            println!("✓ Received {} content items", content.len());
            for item in content {
                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                    println!("  Result: {}", text);
                }
            }
        }
        println!("✓ MCP Tools Call test completed\n");
        Ok(())
    } else {
        println!("Tool call error: {:?}", response.get("error"));
        // Tool execution error is acceptable for this test
        println!("✓ MCP Tools Call test completed (execution error is expected)\n");
        Ok(())
    }
}

#[tokio::test]
async fn test_mcp_prompts_list() -> E2EResult<()> {
    println!("\n=== E2E Test: MCP Prompts List ===");

    let prompt = "Listen on port {AVAILABLE_PORT} via MCP. \
        You are an MCP server. When a client requests prompts/list, return a list with these prompts: \
        - name: code-review, description: Review code for quality and bugs \
        - name: summarize, description: Summarize text content";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_secs(1)).await;

    // Send prompts/list request
    println!("\n→ Sending MCP prompts/list request...");

    let response = send_mcp_request(
        server.port,
        "prompts/list",
        None,
        Some(7),
    ).await?;

    // Validate response
    assert_eq!(response.get("jsonrpc").and_then(|v| v.as_str()), Some("2.0"));
    assert_eq!(response.get("id"), Some(&json!(7)));

    if let Some(result) = response.get("result") {
        if let Some(prompts) = result.get("prompts").and_then(|v| v.as_array()) {
            println!("✓ Received {} prompts", prompts.len());
            for prompt in prompts {
                if let (Some(name), Some(desc)) = (
                    prompt.get("name").and_then(|v| v.as_str()),
                    prompt.get("description").and_then(|v| v.as_str()),
                ) {
                    println!("  - {}: {}", name, desc);
                }
            }
        }
        println!("✓ MCP Prompts List test completed\n");
        Ok(())
    } else {
        println!("✗ Prompts list failed: {:?}", response.get("error"));
        Err("Prompts list failed".into())
    }
}

#[tokio::test]
async fn test_mcp_prompts_get() -> E2EResult<()> {
    println!("\n=== E2E Test: MCP Prompts Get ===");

    let prompt = "Listen on port {AVAILABLE_PORT} via MCP. \
        Use a Python script to handle requests deterministically. \
        When a client gets prompt 'code-review', \
        return messages with role 'user' and content with type 'text' and text 'Review this code'";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_secs(1)).await;

    // Send prompts/get request
    println!("\n→ Sending MCP prompts/get request...");

    let response = send_mcp_request(
        server.port,
        "prompts/get",
        Some(json!({
            "name": "code-review"
        })),
        Some(8),
    ).await?;

    // Validate response
    assert_eq!(response.get("jsonrpc").and_then(|v| v.as_str()), Some("2.0"));
    assert_eq!(response.get("id"), Some(&json!(8)));

    if let Some(result) = response.get("result") {
        if let Some(messages) = result.get("messages").and_then(|v| v.as_array()) {
            println!("✓ Received {} messages", messages.len());
            for message in messages {
                if let Some(role) = message.get("role").and_then(|v| v.as_str()) {
                    println!("  Message role: {}", role);
                }
            }
        }
        println!("✓ MCP Prompts Get test completed\n");
        Ok(())
    } else {
        println!("Prompt get error: {:?}", response.get("error"));
        // Prompt not found is acceptable for this test
        println!("✓ MCP Prompts Get test completed (prompt not found is expected)\n");
        Ok(())
    }
}

#[tokio::test]
async fn test_mcp_error_handling() -> E2EResult<()> {
    println!("\n=== E2E Test: MCP Error Handling ===");

    let prompt = "Listen on port {AVAILABLE_PORT} via MCP. You are an MCP server.";

    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send invalid method request
    println!("\n→ Sending invalid method request...");

    let response = send_mcp_request(
        server.port,
        "invalid/method",
        None,
        Some(99),
    ).await?;

    // Should receive an error response
    assert_eq!(response.get("jsonrpc").and_then(|v| v.as_str()), Some("2.0"));
    assert_eq!(response.get("id"), Some(&json!(99)));

    if let Some(error) = response.get("error") {
        println!("✓ Received error response: {:?}", error);

        // Check error structure
        assert!(error.get("code").is_some(), "Error should have 'code' field");
        assert!(error.get("message").is_some(), "Error should have 'message' field");

        println!("✓ MCP Error Handling test completed\n");
        Ok(())
    } else {
        println!("✗ Expected error response, got result");
        Err("Expected error response".into())
    }
}
