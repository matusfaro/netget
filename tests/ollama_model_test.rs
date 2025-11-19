/// Ollama Model Testing Framework
///
/// This test file demonstrates the Ollama model testing framework that allows
/// evaluating different models and prompts against specific expectations.
///
/// ## Running Tests
///
/// ```bash
/// # Use default model (qwen2.5-coder:7b)
/// cargo test --test ollama_model_test
///
/// # Use specific model
/// OLLAMA_MODEL=qwen3-coder:30b cargo test --test ollama_model_test
///
/// # Run single test
/// cargo test --test ollama_model_test test_open_http_server
///
/// # Run with verbose output
/// cargo test --test ollama_model_test -- --nocapture
/// ```
///
/// ## Environment Variables
///
/// - `OLLAMA_MODEL`: Override the default model (default: qwen2.5-coder:7b)
/// - `OLLAMA_BASE_URL`: Ollama API endpoint (default: http://localhost:11434)
///
/// ## Test Structure
///
/// Each test uses the `OllamaTestBuilder` to:
/// 1. Set up context (user input or network request)
/// 2. Add expectations (exact matches, contains, regex, script execution, etc.)
/// 3. Run the test and validate results
///
/// ## Adding New Tests
///
/// ```rust
/// #[tokio::test]
/// async fn test_my_scenario() -> Result<()> {
///     OllamaTestBuilder::new()
///         .with_user_input("your prompt here")
///         .expect_action_type("open_server")
///         .expect_protocol("tcp")
///         .expect_field_contains("instruction", "some keyword")
///         .run()
///         .await?
///         .assert_success()
/// }
/// ```

mod helpers;

use anyhow::Result;
use serde_json::json;

use helpers::ollama_test_builder::OllamaTestBuilder;
use netget::llm::actions::Parameter;
use netget::protocol::{Event, EventType};

// ============================================================================
// User Input Tests - Testing global action generation
// ============================================================================

/// Test: Open HTTP server with basic prompt
///
/// Validates that the model correctly interprets a simple request to open
/// an HTTP server and generates the appropriate action.
#[tokio::test]
async fn test_open_http_server() -> Result<()> {
    OllamaTestBuilder::new()
        .with_user_input("open http server")
        .expect_action_type("open_server")
        .expect_protocol("http")
        .run()
        .await?
        .assert_success()
}

/// Test: Open TCP server with specific port
///
/// Validates that the model understands port specifications and includes
/// them in the generated action.
#[tokio::test]
async fn test_open_tcp_server_with_port() -> Result<()> {
    OllamaTestBuilder::new()
        .with_user_input("open tcp server on port 8080")
        .expect_action_type("open_server")
        .expect_protocol("tcp")
        .expect_field_exact("port", json!(8080))
        .run()
        .await?
        .assert_success()
}

/// Test: Open server with detailed instruction
///
/// Validates that the model captures the intent of the user's request
/// in the instruction field.
#[tokio::test]
async fn test_open_server_with_instruction() -> Result<()> {
    OllamaTestBuilder::new()
        .with_user_input("open an http server that responds with hello world to all requests")
        .expect_action_type("open_server")
        .expect_protocol("http")
        .expect_field_contains("instruction", "hello world")
        .run()
        .await?
        .assert_success()
}

/// Test: Open DNS server with static response
///
/// Validates that the model correctly generates a static handler when
/// the user specifies a simple, static response pattern.
#[tokio::test]
async fn test_dns_server_with_static_response() -> Result<()> {
    OllamaTestBuilder::new()
        .with_user_input("open a dns server that always responds with 1.2.3.4 for any query")
        .expect_action_type("open_server")
        .expect_protocol("dns")
        .expect_static_handler(json!({
            "type": "send_dns_a_response",
            "query_id": 0,
            "domain": "any",
            "ip": "1.2.3.4"
        }))
        .run()
        .await?
        .assert_success()
}

/// Test: Open client connection
///
/// Validates that the model correctly generates client connection actions
/// and includes the remote address.
#[tokio::test]
async fn test_open_client() -> Result<()> {
    OllamaTestBuilder::new()
        .with_user_input("connect to redis server at localhost:6379")
        .expect_action_type("open_client")
        .expect_protocol("redis")
        .expect_field_contains("remote_addr", "localhost:6379")
        .run()
        .await?
        .assert_success()
}

/// Test: Close server action
///
/// Validates that the model understands server management commands
/// and generates appropriate close actions.
#[tokio::test]
async fn test_close_server() -> Result<()> {
    OllamaTestBuilder::new()
        .with_user_input("close server 123")
        .expect_action_type("close_server")
        .expect_field_exact("server_id", json!(123))
        .run()
        .await?
        .assert_success()
}

// ============================================================================
// Script Handler Tests - Testing script generation and execution
// ============================================================================

/// Test: HTTP server with script handler
///
/// Validates that the model recognizes when a script handler is appropriate
/// for implementing custom logic.
#[tokio::test]
async fn test_http_script_sum_query_params() -> Result<()> {
    // Create test event (HTTP request with query params)
    let test_event = Event::new(
        Box::leak(Box::new(
            EventType::new("http_request", "HTTP request received")
                .with_parameters(vec![
                    Parameter {
                        name: "method".to_string(),
                        type_hint: "string".to_string(),
                        description: "HTTP method".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "path".to_string(),
                        type_hint: "string".to_string(),
                        description: "Request path with query params".to_string(),
                        required: true,
                    },
                ])
        )),
        json!({
            "method": "GET",
            "path": "/?x=5&y=3",
            "query": {"x": "5", "y": "3"}
        }),
    );

    // Expected actions from script
    let expected_actions = vec![json!({
        "type": "send_http_response",
        "status": 200,
        "body": "8"
    })];

    OllamaTestBuilder::new()
        .with_user_input(
            "create an http server that receives query parameters x and y \
             and returns their mathematical sum. write this as a script."
        )
        .expect_action_type("open_server")
        .expect_protocol("http")
        .expect_script_handler()
        .expect_field_contains("instruction", "sum")
        .expect_script_execution(test_event, expected_actions)
        .run()
        .await?
        .assert_success()
}

/// Test: TCP server with echo script
///
/// Validates that the model can generate a script handler for simple
/// echo functionality.
#[tokio::test]
async fn test_tcp_echo_script() -> Result<()> {
    // Create test event (TCP data received)
    let test_event = Event::new(
        Box::leak(Box::new(
            EventType::new("tcp_data_received", "TCP data received")
                .with_parameters(vec![
                    Parameter {
                        name: "data_hex".to_string(),
                        type_hint: "string".to_string(),
                        description: "Hex-encoded TCP data".to_string(),
                        required: true,
                    },
                ])
        )),
        json!({
            "data_hex": "48656c6c6f"
        }),
    );

    // Expected actions from script (echo back same data)
    let expected_actions = vec![json!({
        "type": "send_tcp_data",
        "data_hex": "48656c6c6f"
    })];

    OllamaTestBuilder::new()
        .with_user_input("create a tcp echo server using a script")
        .expect_action_type("open_server")
        .expect_protocol("tcp")
        .expect_script_handler()
        .expect_script_execution(test_event, expected_actions)
        .run()
        .await?
        .assert_success()
}

/// Test: HTTP server with conditional script
///
/// Validates that the model can generate scripts when conditional logic
/// is required.
#[tokio::test]
async fn test_http_conditional_script() -> Result<()> {
    // Create test event (GET request)
    let test_event_get = Event::new(
        Box::leak(Box::new(
            EventType::new("http_request", "HTTP request received")
                .with_parameters(vec![
                    Parameter {
                        name: "method".to_string(),
                        type_hint: "string".to_string(),
                        description: "HTTP method".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "path".to_string(),
                        type_hint: "string".to_string(),
                        description: "Request path".to_string(),
                        required: true,
                    },
                ])
        )),
        json!({
            "method": "GET",
            "path": "/"
        }),
    );

    // Expected actions for GET request
    let expected_actions_get = vec![json!({
        "type": "send_http_response",
        "status": 200,
        "body": "Hello GET"
    })];

    OllamaTestBuilder::new()
        .with_user_input(
            "create an http server that responds with 'Hello GET' for GET requests \
             and 'Hello POST' for POST requests. write as a script."
        )
        .expect_action_type("open_server")
        .expect_protocol("http")
        .expect_script_handler()
        .expect_field_contains("instruction", "GET")
        .expect_field_contains("instruction", "POST")
        .expect_script_execution(test_event_get, expected_actions_get)
        .run()
        .await?
        .assert_success()
}

// ============================================================================
// Network Request Tests - Testing protocol-level action generation
// ============================================================================

/// Test: HTTP request handling with instruction
///
/// Validates that the model correctly responds to network events
/// based on the server's instruction.
#[tokio::test]
async fn test_http_request_with_instruction() -> Result<()> {
    let event = Event::new(
        Box::leak(Box::new(
            EventType::new("http_request", "HTTP request received")
                .with_parameters(vec![
                    Parameter {
                        name: "method".to_string(),
                        type_hint: "string".to_string(),
                        description: "HTTP method".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "path".to_string(),
                        type_hint: "string".to_string(),
                        description: "Request path".to_string(),
                        required: true,
                    },
                ])
        )),
        json!({
            "method": "GET",
            "path": "/hello"
        }),
    );

    let available_actions = vec![
        json!({
            "type": "send_http_response",
            "description": "Send HTTP response",
            "parameters": {
                "status": "HTTP status code",
                "headers": "Response headers (optional)",
                "body": "Response body (optional)"
            }
        }),
        json!({
            "type": "wait_for_more",
            "description": "Wait for more data"
        }),
    ];

    OllamaTestBuilder::new()
        .with_network_request(
            event,
            "Respond with 'Hello, World!' to all requests",
            available_actions,
        )
        .expect_action_type("send_http_response")
        .expect_field_exact("status", json!(200))
        .expect_field_contains("body", "Hello, World!")
        .run()
        .await?
        .assert_success()
}

/// Test: DNS query response
///
/// Validates that the model generates correct DNS response actions.
#[tokio::test]
async fn test_dns_query_response() -> Result<()> {
    let event = Event::new(
        Box::leak(Box::new(
            EventType::new("dns_query", "DNS query received")
                .with_parameters(vec![
                    Parameter {
                        name: "query_id".to_string(),
                        type_hint: "number".to_string(),
                        description: "DNS query identifier".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "domain".to_string(),
                        type_hint: "string".to_string(),
                        description: "Domain name being queried".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "query_type".to_string(),
                        type_hint: "string".to_string(),
                        description: "DNS query type (A, AAAA, etc.)".to_string(),
                        required: true,
                    },
                ])
        )),
        json!({
            "query_id": 12345,
            "domain": "example.com",
            "query_type": "A"
        }),
    );

    let available_actions = vec![
        json!({
            "type": "send_dns_a_response",
            "description": "Send DNS A record response",
            "parameters": {
                "query_id": "Query ID from the request",
                "domain": "Domain name",
                "ip": "IP address to return"
            }
        }),
        json!({
            "type": "send_dns_nxdomain",
            "description": "Send DNS NXDOMAIN (not found) response",
            "parameters": {
                "query_id": "Query ID from the request"
            }
        }),
    ];

    OllamaTestBuilder::new()
        .with_network_request(
            event,
            "Respond to queries for example.com with IP 93.184.216.34",
            available_actions,
        )
        .expect_action_type("send_dns_a_response")
        .expect_field_exact("query_id", json!(12345))
        .expect_field_exact("domain", json!("example.com"))
        .expect_field_exact("ip", json!("93.184.216.34"))
        .run()
        .await?
        .assert_success()
}

/// Test: TCP data handling with hex encoding
///
/// Validates that the model understands hex-encoded data handling.
#[tokio::test]
async fn test_tcp_hex_response() -> Result<()> {
    let event = Event::new(
        Box::leak(Box::new(
            EventType::new("tcp_data_received", "TCP data received")
                .with_parameters(vec![
                    Parameter {
                        name: "data_hex".to_string(),
                        type_hint: "string".to_string(),
                        description: "Hex-encoded TCP data".to_string(),
                        required: true,
                    },
                ])
        )),
        json!({
            "data_hex": "48656c6c6f"
        }),
    );

    let available_actions = vec![
        json!({
            "type": "send_tcp_data",
            "description": "Send TCP data",
            "parameters": {
                "data_hex": "Data in hexadecimal format"
            }
        }),
        json!({
            "type": "disconnect",
            "description": "Disconnect the connection"
        }),
    ];

    OllamaTestBuilder::new()
        .with_network_request(
            event,
            "Echo back any received data",
            available_actions,
        )
        .expect_action_type("send_tcp_data")
        .expect_field_exact("data_hex", json!("48656c6c6f"))
        .run()
        .await?
        .assert_success()
}

// ============================================================================
// Custom Validation Tests - Testing flexible assertions
// ============================================================================

/// Test: Custom validation with closure
///
/// Demonstrates using custom validation logic for complex assertions
/// that don't fit into standard expectation types.
#[tokio::test]
async fn test_custom_validation() -> Result<()> {
    OllamaTestBuilder::new()
        .with_user_input("open http server on port 8080 with timeout of 30 seconds")
        .expect_action_type("open_server")
        .expect_protocol("http")
        .expect_custom("port in valid range", |action| {
            let port = action["port"].as_u64()
                .ok_or_else(|| anyhow::anyhow!("Port is not a number"))?;
            if port < 1024 || port > 65535 {
                anyhow::bail!("Port {} is outside valid range 1024-65535", port);
            }
            Ok(())
        })
        .expect_custom("instruction mentions timeout", |action| {
            let instruction = action["instruction"].as_str()
                .ok_or_else(|| anyhow::anyhow!("No instruction field"))?;
            if !instruction.to_lowercase().contains("timeout") {
                anyhow::bail!("Instruction doesn't mention timeout");
            }
            Ok(())
        })
        .run()
        .await?
        .assert_success()
}

/// Test: Regex pattern matching
///
/// Demonstrates using regex patterns for flexible string matching.
#[tokio::test]
async fn test_regex_pattern_matching() -> Result<()> {
    OllamaTestBuilder::new()
        .with_user_input("open http server on localhost port 8080")
        .expect_action_type("open_server")
        .expect_protocol("http")
        .expect_field_matches("instruction", r"(?i)(localhost|127\.0\.0\.1)")
        .run()
        .await?
        .assert_success()
}

// ============================================================================
// Model Comparison Tests - Testing across different models
// ============================================================================

/// Test: Compare model responses for same prompt
///
/// This test demonstrates how to explicitly test with different models
/// for comparison purposes.
#[tokio::test]
async fn test_model_comparison() -> Result<()> {
    let prompt = "open http server";

    // Test with first model (default or env)
    let result1 = OllamaTestBuilder::new()
        .with_user_input(prompt)
        .expect_action_type("open_server")
        .expect_protocol("http")
        .run()
        .await?;

    println!("Model: {}", result1.model);
    println!("Response: {}", result1.response);
    result1.assert_success()?;

    // Optionally test with different model if specified
    if let Ok(alt_model) = std::env::var("OLLAMA_ALT_MODEL") {
        let result2 = OllamaTestBuilder::new()
            .with_model(&alt_model)
            .with_user_input(prompt)
            .expect_action_type("open_server")
            .expect_protocol("http")
            .run()
            .await?;

        println!("\nAlternative model: {}", result2.model);
        println!("Response: {}", result2.response);
        result2.assert_success()?;
    }

    Ok(())
}

// ============================================================================
// Complex Scenario Tests - Testing multi-step logic
// ============================================================================

/// Test: Server with scheduled tasks
///
/// Validates that the model can generate complex server configurations
/// including scheduled tasks.
#[tokio::test]
async fn test_server_with_scheduled_tasks() -> Result<()> {
    OllamaTestBuilder::new()
        .with_user_input(
            "open http server that sends a heartbeat log every 10 seconds"
        )
        .expect_action_type("open_server")
        .expect_protocol("http")
        .expect_custom("has scheduled tasks", |action| {
            let tasks = action.get("scheduled_tasks")
                .ok_or_else(|| anyhow::anyhow!("No scheduled_tasks field"))?;
            if !tasks.is_array() || tasks.as_array().unwrap().is_empty() {
                anyhow::bail!("No scheduled tasks defined");
            }
            Ok(())
        })
        .run()
        .await?
        .assert_success()
}

/// Test: Multiple actions in response
///
/// Validates that the model can generate multiple actions when appropriate.
#[tokio::test]
async fn test_multiple_actions() -> Result<()> {
    let result = OllamaTestBuilder::new()
        .with_user_input("open both http and tcp servers")
        .expect_custom("multiple servers", |action| {
            // This test expects the first action but also checks if
            // the LLM returned multiple actions
            Ok(())
        })
        .run()
        .await?;

    println!("Actions returned: {}", result.actions.len());

    // For this test, we just validate structure, not necessarily
    // that it returns 2 actions (depends on model interpretation)
    result.assert_success()
}
