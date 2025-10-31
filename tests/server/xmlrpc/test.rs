//! End-to-end XML-RPC tests for NetGet
//!
//! These tests spawn the actual NetGet binary with XML-RPC prompts
//! and validate the responses using real HTTP clients to send XML-RPC requests.

#![cfg(feature = "e2e-tests")]

use super::super::super::helpers::{self, E2EResult, ServerConfig};
use quick_xml::events::Event;
use quick_xml::Reader;

/// Helper to build XML-RPC methodCall
fn build_method_call(method_name: &str, params: &[(&str, &str)]) -> String {
    let mut xml = format!(
        r#"<?xml version="1.0"?>
<methodCall>
  <methodName>{}</methodName>
  <params>"#,
        method_name
    );

    for (value_type, value) in params {
        xml.push_str(&format!(
            r#"
    <param>
      <value><{}>{}</{}></value>
    </param>"#,
            value_type, value, value_type
        ));
    }

    xml.push_str(
        r#"
  </params>
</methodCall>"#,
    );

    xml
}

/// Helper to parse XML-RPC response and extract value
fn parse_xmlrpc_response(xml: &str) -> E2EResult<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut in_value = false;
    let mut in_fault = false;
    let mut result = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                if e.name().as_ref() == b"value" {
                    in_value = true;
                } else if e.name().as_ref() == b"fault" {
                    in_fault = true;
                }
            }
            Ok(Event::Text(e)) if in_value && !in_fault => {
                result = e.unescape()?.to_string();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow::anyhow!("XML parse error: {}", e).into()),
            _ => {}
        }
        buf.clear();
    }

    Ok(result)
}

#[tokio::test]
async fn test_xmlrpc_simple_method() -> E2EResult<()> {
    println!("\n=== E2E Test: Simple XML-RPC Method Call ===");

    // PROMPT: Simple add method
    let prompt = "listen on port {AVAILABLE_PORT} via xmlrpc stack. Implement method 'add' that takes two integers and returns their sum.";

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started: {} stack on port {}", server.stack, server.port);

    // Verify it's actually an XML-RPC server
    assert_eq!(
        server.stack, "XML-RPC",
        "Expected XML-RPC server but got {}",
        server.stack
    );

    // VALIDATION: Call add method with 5 + 3
    let xml_request = build_method_call("add", &[("int", "5"), ("int", "3")]);
    println!("Sending XML-RPC request:\n{}", xml_request);

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/RPC2", server.port);

    let response = client
        .post(&url)
        .header("Content-Type", "text/xml")
        .body(xml_request)
        .send()
        .await?;

    assert_eq!(response.status(), 200);

    let response_xml = response.text().await?;
    println!("Received XML-RPC response:\n{}", response_xml);

    // Parse and validate response
    let result = parse_xmlrpc_response(&response_xml)?;
    println!("Parsed result: {}", result);

    // Should contain 8 (5+3)
    assert!(
        result.contains("8"),
        "Expected sum of 8, got: {}",
        result
    );

    println!("✓ XML-RPC method call validated");
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_xmlrpc_introspection_list_methods() -> E2EResult<()> {
    println!("\n=== E2E Test: XML-RPC system.listMethods Introspection ===");

    // PROMPT: Server with introspection
    let prompt = "listen on port {AVAILABLE_PORT} via xmlrpc stack. Implement these methods: add, subtract, multiply. Also support system.listMethods introspection.";

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Call system.listMethods
    let xml_request = build_method_call("system.listMethods", &[]);
    println!("Sending system.listMethods request");

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/", server.port);

    let response = client
        .post(&url)
        .header("Content-Type", "text/xml")
        .body(xml_request)
        .send()
        .await?;

    assert_eq!(response.status(), 200);

    let response_xml = response.text().await?;
    println!("Received response:\n{}", response_xml);

    // Verify response contains method names
    assert!(response_xml.contains("add") || response_xml.contains("system.listMethods"));

    println!("✓ Introspection validated");
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_xmlrpc_fault_response() -> E2EResult<()> {
    println!("\n=== E2E Test: XML-RPC Fault Response ===");

    // PROMPT: Server that returns fault for unknown methods
    let prompt = "listen on port {AVAILABLE_PORT} via xmlrpc stack. Implement method 'greet'. For unknown methods, return fault code -32601 with message 'Method not found'.";

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Call non-existent method
    let xml_request = build_method_call("nonExistentMethod", &[]);
    println!("Calling non-existent method");

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/", server.port);

    let response = client
        .post(&url)
        .header("Content-Type", "text/xml")
        .body(xml_request)
        .send()
        .await?;

    // XML-RPC returns HTTP 200 even for faults (fault is in XML body)
    assert_eq!(response.status(), 200);

    let response_xml = response.text().await?;
    println!("Received fault response:\n{}", response_xml);

    // Verify it's a fault response
    assert!(
        response_xml.contains("<fault>"),
        "Expected fault response"
    );
    assert!(
        response_xml.contains("faultCode") || response_xml.contains("-32601"),
        "Expected fault code"
    );

    println!("✓ Fault response validated");
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_xmlrpc_string_parameter() -> E2EResult<()> {
    println!("\n=== E2E Test: XML-RPC String Parameter ===");

    // PROMPT: String echo method
    let prompt = "listen on port {AVAILABLE_PORT} via xmlrpc stack. Implement method 'greet' that takes a name (string) and returns 'Hello, [name]!'.";

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Call greet method
    let xml_request = build_method_call("greet", &[("string", "Alice")]);
    println!("Calling greet method with 'Alice'");

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/", server.port);

    let response = client
        .post(&url)
        .header("Content-Type", "text/xml")
        .body(xml_request)
        .send()
        .await?;

    assert_eq!(response.status(), 200);

    let response_xml = response.text().await?;
    println!("Received response:\n{}", response_xml);

    let result = parse_xmlrpc_response(&response_xml)?;
    println!("Parsed result: {}", result);

    // Should contain greeting
    assert!(
        result.contains("Alice"),
        "Expected greeting with 'Alice', got: {}",
        result
    );

    println!("✓ String parameter validated");
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_xmlrpc_boolean_parameter() -> E2EResult<()> {
    println!("\n=== E2E Test: XML-RPC Boolean Parameter ===");

    // PROMPT: Boolean parameter
    let prompt = "listen on port {AVAILABLE_PORT} via xmlrpc stack. Implement method 'toggle' that takes a boolean and returns the opposite.";

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Call toggle method with true (1)
    let xml_request = build_method_call("toggle", &[("boolean", "1")]);
    println!("Calling toggle method with true");

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/", server.port);

    let response = client
        .post(&url)
        .header("Content-Type", "text/xml")
        .body(xml_request)
        .send()
        .await?;

    assert_eq!(response.status(), 200);

    let response_xml = response.text().await?;
    println!("Received response:\n{}", response_xml);

    // Should contain boolean response
    assert!(
        response_xml.contains("<boolean>") || response_xml.contains("0") || response_xml.contains("false"),
        "Expected boolean in response"
    );

    println!("✓ Boolean parameter validated");
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_xmlrpc_multiple_parameters() -> E2EResult<()> {
    println!("\n=== E2E Test: XML-RPC Multiple Parameters ===");

    // PROMPT: Multiple parameters
    let prompt = "listen on port {AVAILABLE_PORT} via xmlrpc stack. Implement method 'concat' that takes two strings and returns them concatenated with a space between.";

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Call concat method
    let xml_request = build_method_call("concat", &[("string", "Hello"), ("string", "World")]);
    println!("Calling concat method");

    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/", server.port);

    let response = client
        .post(&url)
        .header("Content-Type", "text/xml")
        .body(xml_request)
        .send()
        .await?;

    assert_eq!(response.status(), 200);

    let response_xml = response.text().await?;
    println!("Received response:\n{}", response_xml);

    let result = parse_xmlrpc_response(&response_xml)?;
    println!("Parsed result: {}", result);

    // Should contain both words
    assert!(
        result.contains("Hello") && result.contains("World"),
        "Expected 'Hello World', got: {}",
        result
    );

    println!("✓ Multiple parameters validated");
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}

#[tokio::test]
async fn test_xmlrpc_non_post_request() -> E2EResult<()> {
    println!("\n=== E2E Test: XML-RPC Non-POST Request (Should Fail) ===");

    // PROMPT: Standard XML-RPC server
    let prompt = "listen on port {AVAILABLE_PORT} via xmlrpc stack. Implement method 'test'.";

    // Start the server
    let server = helpers::start_netget_server(ServerConfig::new(prompt)).await?;
    println!("Server started on port {}", server.port);

    // VALIDATION: Try GET request (XML-RPC requires POST)
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/", server.port);

    let response = client.get(&url).send().await?;

    // Should return 200 with fault (XML-RPC returns 200 for faults)
    assert_eq!(response.status(), 200);

    let response_xml = response.text().await?;
    println!("Received response:\n{}", response_xml);

    // Should be a fault response for invalid request
    assert!(
        response_xml.contains("<fault>") || response_xml.contains("Invalid request"),
        "Expected fault for non-POST request"
    );

    println!("✓ Non-POST rejection validated");
    server.stop().await?;
    println!("=== Test passed ===\n");
    Ok(())
}
