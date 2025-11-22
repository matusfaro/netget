# Ollama Model Testing Framework

A specialized E2E testing framework for evaluating Ollama models and prompts. This framework allows you to test how well a model interprets prompts and generates appropriate actions without requiring actual server/client execution.

## Overview

The framework consists of:

1. **`OllamaTestBuilder`** (`tests/helpers/ollama_test_builder.rs`) - Builder pattern for creating tests
2. **Example tests** (`tests/ollama_model_test.rs`) - Comprehensive test scenarios demonstrating usage
3. **Model selection** - Configurable via environment variables
4. **Script execution testing** - Actually runs generated scripts and validates output

## Quick Start

### Running Tests

```bash
# Use default model (qwen2.5-coder:7b)
cargo test --test ollama_model_test

# Use specific model
OLLAMA_MODEL=qwen3-coder:30b cargo test --test ollama_model_test

# Compare multiple models
OLLAMA_MODEL=model1 cargo test --test ollama_model_test
OLLAMA_MODEL=model2 cargo test --test ollama_model_test

# Run single test
cargo test --test ollama_model_test test_open_http_server

# Run with verbose output
cargo test --test ollama_model_test -- --nocapture
```

### Environment Variables

- **`OLLAMA_MODEL`** - Override default model (default: `qwen2.5-coder:7b`)
- **`OLLAMA_BASE_URL`** - Ollama API endpoint (default: `http://localhost:11434`)
- **`OLLAMA_ALT_MODEL`** - Alternative model for comparison tests (optional)

## Builder Pattern API

### Context Setup

```rust
// User input context (for global actions)
OllamaTestBuilder::new()
    .with_user_input("open http server")
    // ...

// Network request context (for protocol-level actions)
OllamaTestBuilder::new()
    .with_network_request(event, instruction, available_actions)
    // ...

// Model selection (optional, defaults to env var or default model)
OllamaTestBuilder::new()
    .with_model("qwen3-coder:30b")
    // ...
```

### Expectations (Assertions)

#### Exact Match Assertions

```rust
// Expect specific action type
.expect_action_type("open_server")

// Expect exact protocol
.expect_protocol("http")

// Expect exact field value
.expect_field_exact("port", json!(8080))
```

#### Flexible Assertions

```rust
// Expect field contains substring
.expect_field_contains("instruction", "hello world")

// Expect field matches regex pattern
.expect_field_matches("instruction", r"(?i)(localhost|127\.0\.0\.1)")
```

#### Handler Assertions

```rust
// Expect static handler with specific value
.expect_static_handler(json!({
    "type": "send_dns_a_response",
    "query_id": 0,
    "domain": "any",
    "ip": "1.2.3.4"
}))

// Expect script handler (any language)
.expect_script_handler()

// Expect script with specific language
.expect_script_with_language("python")

// Test script execution with input/output
// This actually RUNS the generated script and validates output!
.expect_script_execution(input_event, expected_actions)
```

#### Custom Assertions

```rust
// Custom validation with closure
.expect_custom("port in valid range", |action| {
    let port = action["port"].as_u64()
        .ok_or_else(|| anyhow::anyhow!("Port is not a number"))?;
    if port < 1024 || port > 65535 {
        anyhow::bail!("Port {} is outside valid range", port);
    }
    Ok(())
})
```

### Running the Test

```rust
// Run and assert success (returns error if any expectation fails)
.run()
.await?
.assert_success()

// Or get detailed results
let result = test.run().await?;
println!("Model: {}", result.model);
println!("Response: {}", result.response);
println!("Actions: {}", serde_json::to_string_pretty(&result.actions)?);
println!("Passed: {:?}", result.passed);
println!("Failed: {:?}", result.failed);
```

## Example Test Cases

### User Input Tests

```rust
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
```

### Network Request Tests

```rust
#[tokio::test]
async fn test_http_request_with_instruction() -> Result<()> {
    let event = Event::new(
        &EventType::new("http_request", "HTTP request received")
            .with_parameters(vec![
                ("method".to_string(), "GET".to_string()),
                ("path".to_string(), "/hello".to_string()),
            ]),
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
                "body": "Response body"
            }
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
```

### Script Handler Tests

#### Basic Script Validation

```rust
#[tokio::test]
async fn test_http_script_handler() -> Result<()> {
    OllamaTestBuilder::new()
        .with_user_input(
            "create an http server that sums query params x and y. \
             write this as a script."
        )
        .expect_action_type("open_server")
        .expect_protocol("http")
        .expect_script_handler()
        .expect_field_contains("instruction", "sum")
        .run()
        .await?
        .assert_success()
}
```

#### Script Execution Testing (IMPORTANT!)

**The framework can actually execute generated scripts and validate their output:**

```rust
#[tokio::test]
async fn test_http_script_sum_query_params() -> Result<()> {
    // Create test event (HTTP request with query params)
    let test_event = Event::new(
        &EventType::new("http_request", "HTTP request received")
            .with_parameters(vec![
                ("method".to_string(), "GET".to_string()),
                ("path".to_string(), "/?x=5&y=3".to_string()),
            ]),
        json!({
            "method": "GET",
            "path": "/?x=5&y=3",
            "query": {"x": "5", "y": "3"}
        }),
    );

    // Expected actions from script (should return sum = 8)
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
        .expect_script_execution(test_event, expected_actions)  // <- Actually runs script!
        .run()
        .await?
        .assert_success()
}
```

**How it works:**
1. LLM generates a script in the action's `handler.script` field
2. Framework extracts the script code and language
3. Creates a `ScriptInput` with your test event data
4. **Executes the script** using NetGet's script executor
5. Compares the script's output actions with your expected actions
6. Fails the test if they don't match exactly

**Benefits:**
- Validates that generated scripts are **syntactically correct**
- Validates that scripts **produce correct output**
- Tests **actual code execution**, not just structure
- Catches bugs in LLM-generated logic

### Model Comparison Tests

```rust
#[tokio::test]
async fn test_model_comparison() -> Result<()> {
    let prompt = "open http server";

    // Test with default model
    let result1 = OllamaTestBuilder::new()
        .with_user_input(prompt)
        .expect_action_type("open_server")
        .expect_protocol("http")
        .run()
        .await?;

    // Test with alternative model
    if let Ok(alt_model) = std::env::var("OLLAMA_ALT_MODEL") {
        let result2 = OllamaTestBuilder::new()
            .with_model(&alt_model)
            .with_user_input(prompt)
            .expect_action_type("open_server")
            .expect_protocol("http")
            .run()
            .await?;

        println!("Model 1: {} - {:?}", result1.model, result1.passed);
        println!("Model 2: {} - {:?}", result2.model, result2.passed);
    }

    Ok(())
}
```

## Use Cases

### 1. Model Evaluation

Compare different Ollama models to see which performs better:

```bash
# Test model A (default)
OLLAMA_MODEL=qwen3-coder:30b cargo test --test ollama_model_test

# Test model B (alternative)
OLLAMA_MODEL=qwen2.5-coder:7b cargo test --test ollama_model_test

# Compare results
```

### 2. Prompt Engineering

Test how changes to prompts affect model behavior:

```rust
// Version 1
.with_user_input("open http server")

// Version 2
.with_user_input("create a new http server listening on a port")

// Compare which produces better results
```

### 3. Instruction Quality

Validate that models correctly interpret complex instructions:

```rust
.with_user_input("create an http server that responds with the sum of query parameters x and y")
.expect_script_handler()  // Should recognize this needs a script
```

### 4. Regression Testing

Ensure model updates don't break existing functionality:

```bash
# Add tests for known-good behaviors
# Run tests when upgrading models
OLLAMA_MODEL=new-model cargo test --test ollama_model_test
```

## Test Organization

The test file (`tests/ollama_model_test.rs`) is organized into sections:

1. **User Input Tests** - Testing global action generation
   - `test_open_http_server()` - Basic server creation
   - `test_open_tcp_server_with_port()` - Port specification
   - `test_open_server_with_instruction()` - Detailed instructions
   - `test_dns_server_with_static_response()` - Static handlers
   - `test_open_client()` - Client connections
   - `test_close_server()` - Server management

2. **Script Handler Tests** - Testing script generation
   - `test_http_script_sum_query_params()` - Custom logic scripts
   - `test_tcp_echo_script()` - Simple echo scripts
   - `test_http_conditional_script()` - Conditional logic

3. **Network Request Tests** - Testing protocol-level actions
   - `test_http_request_with_instruction()` - HTTP responses
   - `test_dns_query_response()` - DNS responses
   - `test_tcp_hex_response()` - Binary data handling

4. **Custom Validation Tests** - Testing flexible assertions
   - `test_custom_validation()` - Custom logic
   - `test_regex_pattern_matching()` - Pattern matching

5. **Model Comparison Tests** - Testing across models
   - `test_model_comparison()` - Side-by-side comparison

6. **Complex Scenario Tests** - Testing multi-step logic
   - `test_server_with_scheduled_tasks()` - Scheduled tasks
   - `test_multiple_actions()` - Multiple actions

## Adding New Tests

1. Choose appropriate section or create new one
2. Use descriptive test name (`test_<scenario>`)
3. Add comprehensive docstring explaining what's being tested
4. Use builder pattern with clear expectations
5. Call `.assert_success()` to validate

Example template:

```rust
/// Test: <Brief description>
///
/// Validates that the model <what you're testing>.
#[tokio::test]
async fn test_<scenario>() -> Result<()> {
    OllamaTestBuilder::new()
        .with_user_input("<prompt>")
        .expect_action_type("<action>")
        .expect_protocol("<protocol>")
        // Add more expectations...
        .run()
        .await?
        .assert_success()
}
```

## Implementation Details

### How It Works

1. **Prompt Construction** - Builder creates appropriate prompt based on context
2. **Ollama API Call** - Sends prompt to configured model
3. **Response Parsing** - Extracts JSON actions from LLM response
4. **Validation** - Runs all expectations against parsed actions
5. **Results** - Returns detailed pass/fail results

### Response Parsing

The framework handles various response formats:

- Direct JSON array: `[{"type": "open_server", ...}]`
- Markdown code blocks: ` ```json [...] ``` `
- Text with embedded JSON: `Here are the actions: [...]`

### Error Handling

- **Compilation errors** - Shows which expectation failed
- **LLM errors** - Shows raw response if parsing fails
- **Detailed context** - Includes model, prompt, and response in errors

## Tips & Best Practices

### 1. Start Simple

Begin with basic tests and add complexity:

```rust
// Start here
.expect_action_type("open_server")
.expect_protocol("http")

// Then add
.expect_field_contains("instruction", "hello")

// Finally add custom validation
.expect_custom("complex validation", |action| { ... })
```

### 2. Use Meaningful Prompts

Test realistic user inputs, not contrived examples:

```rust
// ✓ Good - realistic
.with_user_input("open http server on port 8080")

// ✗ Bad - too formal
.with_user_input("Please instantiate an HTTP server instance on TCP port 8080")
```

### 3. Test Edge Cases

Include tests for ambiguous or complex scenarios:

```rust
.with_user_input("open http server that returns sum of x and y")
.expect_script_handler()  // Should recognize this needs custom logic
```

### 4. Document Expectations

Explain why you expect certain behavior:

```rust
// Model should recognize that summing requires a script handler
// because it involves dynamic computation
.expect_script_handler()
```

### 5. Use Custom Validation Wisely

Reserve custom validation for complex logic that can't be expressed with built-in expectations:

```rust
// ✓ Good - complex validation
.expect_custom("valid port range", |action| {
    let port = action["port"].as_u64()?;
    if port < 1024 || port > 65535 {
        bail!("Invalid port");
    }
    Ok(())
})

// ✗ Bad - use built-in instead
.expect_custom("has http protocol", |action| {
    if action["protocol"] != "http" {
        bail!("Wrong protocol");
    }
    Ok(())
})
// Better:
.expect_protocol("http")
```

## Future Enhancements

Potential additions to the framework:

1. ✅ **Script Execution Testing** - Actually run generated scripts and validate output (IMPLEMENTED!)
2. **Multi-Turn Conversations** - Test follow-up prompts and context retention
3. **Performance Metrics** - Track response time and token usage
4. **Failure Analysis** - Categorize and analyze common failure patterns
5. **Benchmark Suite** - Standard set of tests for model comparison
6. **Script Output Assertions** - More flexible assertions on script output (regex, contains, custom)

## Troubleshooting

### Tests Fail with "Could not parse actions"

Check that your model is correctly formatted responses. Use `--nocapture` to see raw output:

```bash
cargo test --test ollama_model_test test_name -- --nocapture
```

### Tests Timeout

Increase timeout or check Ollama is running:

```bash
curl http://localhost:11434/api/tags
```

### Inconsistent Results

Some models may be non-deterministic. Run tests multiple times or use temperature=0 in Ollama config.

## See Also

- `tests/ollama_model_test.rs` - Example test implementations
- `tests/helpers/ollama_test_builder.rs` - Framework implementation
- NetGet E2E testing docs - `tests/server/*/CLAUDE.md`
