use anyhow::{Context, Result, anyhow, bail};
use serde_json::{json, Value};
use std::sync::Arc;

use netget::llm::{OllamaClient, prompt::PromptBuilder};
use netget::protocol::Event;
use netget::scripting::{
    ScriptConfig, ScriptInput, ScriptLanguage, ScriptSource, ServerContext,
    execute_script,
};
use netget::state::app_state::AppState;
use netget::state::ServerId;

/// Helper function to convert Event to JSON for serialization
fn event_to_json(event: &Event) -> Result<Value> {
    Ok(json!({
        "type": event.event_type.id,
        "data": event.data,
    }))
}

/// Builder for testing Ollama model responses
pub struct OllamaTestBuilder {
    prompt_context: Option<PromptContext>,
    expectations: Vec<Expectation>,
    model: Option<String>,
}

/// Context for the LLM prompt
pub enum PromptContext {
    /// User input from CLI
    UserInput {
        user_message: String,
    },
    /// Network request event
    NetworkRequest {
        event: Event,
        instruction: String,
        server_id: ServerId,
    },
}

/// Expectations/assertions for the test
pub enum Expectation {
    /// Expect specific action type (exact match)
    ActionType(String),

    /// Expect exact field value
    FieldExact { field: String, value: Value },

    /// Expect field contains substring (for strings)
    FieldContains { field: String, substring: String },

    /// Expect field matches regex
    FieldMatches { field: String, pattern: String },

    /// Expect protocol exact match
    Protocol(String),

    /// Expect static handler with specific value
    StaticHandler(Value),

    /// Expect script handler
    ScriptHandler,

    /// Expect script handler with language validation
    ScriptWithLanguage(String),

    /// Test script execution with input/output
    ScriptTest {
        input_event: Event,
        expected_actions: Vec<Value>,
    },

    /// Custom validation function
    Custom {
        name: String,
        validator: Arc<dyn Fn(&Value) -> Result<()> + Send + Sync>,
    },
}

impl OllamaTestBuilder {
    /// Create a new test builder
    pub fn new() -> Self {
        Self {
            prompt_context: None,
            expectations: Vec::new(),
            model: None,
        }
    }

    /// Set user input context
    pub fn with_user_input(mut self, user_message: impl Into<String>) -> Self {
        self.prompt_context = Some(PromptContext::UserInput {
            user_message: user_message.into(),
        });
        self
    }

    /// Set network request context
    pub fn with_network_request(
        mut self,
        event: Event,
        instruction: impl Into<String>,
        server_id: ServerId,
    ) -> Self {
        self.prompt_context = Some(PromptContext::NetworkRequest {
            event,
            instruction: instruction.into(),
            server_id,
        });
        self
    }

    /// Set the Ollama model to use
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Expect specific action type
    pub fn expect_action_type(mut self, action_type: impl Into<String>) -> Self {
        self.expectations.push(Expectation::ActionType(action_type.into()));
        self
    }

    /// Expect exact field value
    pub fn expect_field_exact(mut self, field: impl Into<String>, value: Value) -> Self {
        self.expectations.push(Expectation::FieldExact {
            field: field.into(),
            value,
        });
        self
    }

    /// Expect field contains substring
    pub fn expect_field_contains(mut self, field: impl Into<String>, substring: impl Into<String>) -> Self {
        self.expectations.push(Expectation::FieldContains {
            field: field.into(),
            substring: substring.into(),
        });
        self
    }

    /// Expect field matches regex
    pub fn expect_field_matches(mut self, field: impl Into<String>, pattern: impl Into<String>) -> Self {
        self.expectations.push(Expectation::FieldMatches {
            field: field.into(),
            pattern: pattern.into(),
        });
        self
    }

    /// Expect exact protocol match
    pub fn expect_protocol(mut self, protocol: impl Into<String>) -> Self {
        self.expectations.push(Expectation::Protocol(protocol.into()));
        self
    }

    /// Expect static handler with specific value
    pub fn expect_static_handler(mut self, value: Value) -> Self {
        self.expectations.push(Expectation::StaticHandler(value));
        self
    }

    /// Expect script handler
    pub fn expect_script_handler(mut self) -> Self {
        self.expectations.push(Expectation::ScriptHandler);
        self
    }

    /// Expect script handler with specific language
    pub fn expect_script_with_language(mut self, language: impl Into<String>) -> Self {
        self.expectations.push(Expectation::ScriptWithLanguage(language.into()));
        self
    }

    /// Test script execution with input/output
    ///
    /// This will:
    /// 1. Extract the script from the LLM's response
    /// 2. Execute it with the provided input event
    /// 3. Validate the output matches expected actions
    pub fn expect_script_execution(
        mut self,
        input_event: Event,
        expected_actions: Vec<Value>,
    ) -> Self {
        self.expectations.push(Expectation::ScriptTest {
            input_event,
            expected_actions,
        });
        self
    }

    /// Add custom validation
    pub fn expect_custom<F>(mut self, name: impl Into<String>, validator: F) -> Self
    where
        F: Fn(&Value) -> Result<()> + Send + Sync + 'static,
    {
        self.expectations.push(Expectation::Custom {
            name: name.into(),
            validator: Arc::new(validator),
        });
        self
    }

    /// Run the test
    pub async fn run(self) -> Result<TestResult> {
        // Initialize tracing subscriber (once) to capture RUST_LOG output
        use std::sync::Once;
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
                .with_test_writer()
                .try_init();
        });

        // Get model from env var or use default (7B balances speed and capability for tests)
        let model = self.model
            .or_else(|| std::env::var("OLLAMA_MODEL").ok())
            .unwrap_or_else(|| "qwen2.5-coder:7b".to_string());

        println!("Testing with model: {}", model);

        // Build prompt based on context using real prompt generation
        let prompt_context = self.prompt_context
            .ok_or_else(|| anyhow!("No prompt context set"))?;

        // Create a minimal AppState for testing (no servers, default settings)
        let state = AppState::new();

        let prompt = match prompt_context {
            PromptContext::UserInput { user_message } => {
                // Use real prompt generation for user input
                let system_prompt = PromptBuilder::build_user_input_system_prompt(
                    &state,
                    Vec::new(), // No protocol-specific actions for generic tests
                    None,       // No conversation history
                )
                .await;

                // Combine system prompt with user message
                format!("{}\n\n# User Input\n\n{}", system_prompt, user_message)
            }
            PromptContext::NetworkRequest { event, instruction, server_id } => {
                // Set the server instruction in state
                let _ = state.set_instruction(server_id, instruction.clone()).await;

                // Get protocol-specific actions from registry
                // For now, just use common actions - protocol actions would come from the server
                let actions = netget::llm::actions::get_network_event_common_actions();

                // Use real prompt generation for network events
                let system_prompt = PromptBuilder::build_network_event_action_prompt_for_server(
                    &state,
                    server_id,
                    actions,
                )
                .await;

                // Build event trigger message
                let event_message = PromptBuilder::build_event_trigger_message_with_id(
                    &event.event_type.id,
                    &event.event_type.description,
                    event.data.clone(),
                );

                // Combine system prompt with event message
                format!("{}\n\n# Network Event\n\n{}", system_prompt, event_message)
            }
        };

        // Call Ollama
        let ollama_client = OllamaClient::new(
            std::env::var("OLLAMA_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:11434".to_string()),
        );

        tracing::info!("=== FULL PROMPT ===\n{}\n=== END PROMPT ===", prompt);

        let response = ollama_client
            .generate_with_retry(&model, &prompt, "JSON response with actions array", 0)
            .await
            .context("Failed to call Ollama API")?;

        tracing::info!("=== LLM RESPONSE ===\n{}\n=== END RESPONSE ===", response);
        println!("LLM Response:\n{}", response);

        // Parse actions from response
        let actions = parse_actions_from_response(&response)
            .context("Failed to parse actions from LLM response")?;

        println!("Parsed actions: {}", serde_json::to_string_pretty(&actions)?);

        // Run expectations
        let mut passed = Vec::new();
        let mut failed = Vec::new();

        for expectation in self.expectations {
            match validate_expectation(&expectation, &actions).await {
                Ok(_) => {
                    let desc = expectation_description(&expectation);
                    println!("✅ PASS {}", desc);
                    passed.push(desc);
                }
                Err(e) => {
                    let desc = expectation_description(&expectation);
                    println!("❌ FAIL {}: {}", desc, e);
                    failed.push((desc, e.to_string()));
                }
            }
        }

        Ok(TestResult {
            model,
            prompt,
            response,
            actions,
            passed,
            failed,
        })
    }
}

/// Result of a test run
pub struct TestResult {
    pub model: String,
    pub prompt: String,
    pub response: String,
    pub actions: Vec<Value>,
    pub passed: Vec<String>,
    pub failed: Vec<(String, String)>,
}

impl TestResult {
    /// Check if all expectations passed
    pub fn is_success(&self) -> bool {
        self.failed.is_empty()
    }

    /// Assert that all expectations passed
    pub fn assert_success(&self) -> Result<()> {
        if !self.is_success() {
            bail!(
                "Test failed with {} failures:\n{}",
                self.failed.len(),
                self.failed
                    .iter()
                    .map(|(desc, err)| format!("  - {}: {}", desc, err))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
        Ok(())
    }
}

/// Parse actions from LLM response
/// The real prompt expects {"actions": [...]} format
fn parse_actions_from_response(response: &str) -> Result<Vec<Value>> {
    let trimmed = response.trim();

    // Try parsing as {"actions": [...]} format (real netget format)
    if let Ok(action_response) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(actions) = action_response.get("actions").and_then(|a| a.as_array()) {
            return Ok(actions.clone());
        }
    }

    // Try to extract JSON from markdown code block
    if let Some(json_str) = extract_json_from_markdown(trimmed) {
        if let Ok(action_response) = serde_json::from_str::<serde_json::Value>(&json_str) {
            if let Some(actions) = action_response.get("actions").and_then(|a| a.as_array()) {
                return Ok(actions.clone());
            }
        }
    }

    // Try to find JSON object with "actions" field
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            let json_str = &trimmed[start..=end];
            if let Ok(action_response) = serde_json::from_str::<serde_json::Value>(json_str) {
                if let Some(actions) = action_response.get("actions").and_then(|a| a.as_array()) {
                    return Ok(actions.clone());
                }
            }
        }
    }

    bail!("Could not parse actions from response. Expected {{\"actions\": [...]}} format.\nResponse: {}", response)
}

/// Extract JSON from markdown code block
fn extract_json_from_markdown(text: &str) -> Option<String> {
    // Look for ```json ... ``` or ``` ... ```
    let patterns = vec![
        (r"```json\s*\n([\s\S]*?)\n```", 1),
        (r"```\s*\n([\s\S]*?)\n```", 1),
    ];

    for (pattern, group) in patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            if let Some(captures) = re.captures(text) {
                if let Some(json) = captures.get(group) {
                    return Some(json.as_str().to_string());
                }
            }
        }
    }

    None
}

/// Validate an expectation against actions
async fn validate_expectation(expectation: &Expectation, actions: &[Value]) -> Result<()> {
    match expectation {
        Expectation::ActionType(expected_type) => {
            if actions.is_empty() {
                bail!("No actions returned");
            }
            let action = &actions[0];
            let actual_type = action["type"]
                .as_str()
                .ok_or_else(|| anyhow!("Action has no 'type' field"))?;
            if actual_type != expected_type {
                eprintln!("\n❌ Action Type Mismatch:");
                eprintln!("   Expected: '{}'", expected_type);
                eprintln!("   Actual:   '{}'", actual_type);
                eprintln!("   Full action:\n{}", serde_json::to_string_pretty(action).unwrap_or_default());
                bail!("Expected type '{}', got '{}'", expected_type, actual_type);
            }
            Ok(())
        }

        Expectation::FieldExact { field, value } => {
            if actions.is_empty() {
                bail!("No actions returned");
            }
            let action = &actions[0];
            let actual_value = action.get(field)
                .ok_or_else(|| anyhow!("Action has no '{}' field", field))?;
            if actual_value != value {
                eprintln!("\n❌ Field Exact Match:");
                eprintln!("   Field:    '{}'", field);
                eprintln!("   Expected:\n{}", serde_json::to_string_pretty(value).unwrap_or_default());
                eprintln!("   Actual:\n{}", serde_json::to_string_pretty(actual_value).unwrap_or_default());
                bail!(
                    "Field '{}': expected {}, got {}",
                    field,
                    value,
                    actual_value
                );
            }
            Ok(())
        }

        Expectation::FieldContains { field, substring } => {
            if actions.is_empty() {
                bail!("No actions returned");
            }
            let action = &actions[0];
            let actual_value = action.get(field)
                .ok_or_else(|| anyhow!("Action has no '{}' field", field))?
                .as_str()
                .ok_or_else(|| anyhow!("Field '{}' is not a string", field))?;
            if !actual_value.contains(substring) {
                eprintln!("\n❌ Field Contains:");
                eprintln!("   Field:           '{}'", field);
                eprintln!("   Expected substr: '{}'", substring);
                eprintln!("   Actual value:    '{}'", actual_value);
                bail!(
                    "Field '{}': expected to contain '{}', got '{}'",
                    field,
                    substring,
                    actual_value
                );
            }
            Ok(())
        }

        Expectation::FieldMatches { field, pattern } => {
            if actions.is_empty() {
                bail!("No actions returned");
            }
            let action = &actions[0];
            let actual_value = action.get(field)
                .ok_or_else(|| anyhow!("Action has no '{}' field", field))?
                .as_str()
                .ok_or_else(|| anyhow!("Field '{}' is not a string", field))?;
            let re = regex::Regex::new(pattern)?;
            if !re.is_match(actual_value) {
                eprintln!("\n❌ Field Regex Match Failed:");
                eprintln!("   Field:    '{}'", field);
                eprintln!("   Pattern:  '{}'", pattern);
                eprintln!("   Actual:   '{}'", actual_value);
                bail!(
                    "Expected field '{}' to match pattern '{}', got '{}'",
                    field,
                    pattern,
                    actual_value
                );
            }
            Ok(())
        }

        Expectation::Protocol(expected_protocol) => {
            if actions.is_empty() {
                bail!("No actions returned");
            }
            let action = &actions[0];
            let actual_protocol = action["protocol"]
                .as_str()
                .ok_or_else(|| anyhow!("Action has no 'protocol' field"))?;
            if actual_protocol != expected_protocol {
                eprintln!("\n❌ Protocol Mismatch:");
                eprintln!("   Expected: '{}'", expected_protocol);
                eprintln!("   Actual:   '{}'", actual_protocol);
                bail!(
                    "Expected protocol '{}', got '{}'",
                    expected_protocol,
                    actual_protocol
                );
            }
            Ok(())
        }

        Expectation::StaticHandler(expected_value) => {
            if actions.is_empty() {
                bail!("No actions returned");
            }
            let action = &actions[0];
            let handler = action.get("handler")
                .ok_or_else(|| anyhow!("Action has no 'handler' field"))?;

            // Check if handler is object with "static" field
            if let Some(static_value) = handler.get("static") {
                if static_value != expected_value {
                    eprintln!("\n❌ Static Handler Mismatch:");
                    eprintln!("   Expected: {}", serde_json::to_string_pretty(expected_value).unwrap_or_default());
                    eprintln!("   Actual:   {}", serde_json::to_string_pretty(static_value).unwrap_or_default());
                    bail!(
                        "Expected static handler value {}, got {}",
                        expected_value,
                        static_value
                    );
                }
                return Ok(());
            }

            eprintln!("\n❌ No Static Handler Found:");
            eprintln!("   Handler: {}", serde_json::to_string_pretty(handler).unwrap_or_default());
            bail!("Action does not have a static handler");
        }

        Expectation::ScriptHandler => {
            if actions.is_empty() {
                bail!("No actions returned");
            }
            let action = &actions[0];
            let handler = action.get("handler")
                .ok_or_else(|| anyhow!("Action has no 'handler' field"))?;

            // Check if handler has "script" field
            if handler.get("script").is_none() {
                eprintln!("\n❌ No Script Handler Found:");
                eprintln!("   Handler: {}", serde_json::to_string_pretty(handler).unwrap_or_default());
                bail!("Action does not have a script handler");
            }
            Ok(())
        }

        Expectation::ScriptWithLanguage(expected_lang) => {
            if actions.is_empty() {
                bail!("No actions returned");
            }
            let action = &actions[0];
            let handler = action.get("handler")
                .ok_or_else(|| anyhow!("Action has no 'handler' field"))?;

            // Check if handler has "script" field
            if handler.get("script").is_none() {
                eprintln!("\n❌ No Script Handler Found:");
                eprintln!("   Handler: {}", serde_json::to_string_pretty(handler).unwrap_or_default());
                bail!("Action does not have a script handler");
            }

            // Check if language matches
            if let Some(language) = handler.get("language") {
                let actual_lang = language.as_str()
                    .ok_or_else(|| anyhow!("Language is not a string"))?;
                if actual_lang.to_lowercase() != expected_lang.to_lowercase() {
                    eprintln!("\n❌ Script Language Mismatch:");
                    eprintln!("   Expected: '{}'", expected_lang);
                    eprintln!("   Actual:   '{}'", actual_lang);
                    bail!(
                        "Expected script language '{}', got '{}'",
                        expected_lang,
                        actual_lang
                    );
                }
            }

            Ok(())
        }

        Expectation::ScriptTest { input_event, expected_actions } => {
            if actions.is_empty() {
                bail!("No actions returned");
            }
            let action = &actions[0];
            let handler = action.get("handler")
                .ok_or_else(|| anyhow!("Action has no 'handler' field"))?;

            // Get script code
            let script_code = handler.get("script")
                .and_then(|s| s.as_str())
                .ok_or_else(|| anyhow!("Action does not have a script handler"))?;

            // Get script language (default to Python if not specified)
            let language_str = handler.get("language")
                .and_then(|l| l.as_str())
                .unwrap_or("python");
            let language = ScriptLanguage::parse(language_str)
                .ok_or_else(|| anyhow!("Invalid script language: {}", language_str))?;

            // Create script config
            let config = ScriptConfig {
                language,
                source: ScriptSource::Inline(script_code.to_string()),
                handles_contexts: vec!["all".to_string()],
            };

            // Create script input from event
            let input = ScriptInput {
                event_type_id: input_event.event_type.id.clone(),
                server: ServerContext {
                    id: 1,
                    port: 8080,
                    stack: "test".to_string(),
                    memory: String::new(),
                    instruction: "test".to_string(),
                },
                connection: None,
                event: event_to_json(&input_event)?,
            };

            // Execute the script
            let response = execute_script(&config, &input)
                .context("Failed to execute script")?;

            // Compare actions
            if response.actions.len() != expected_actions.len() {
                eprintln!("\n❌ Script Action Count Mismatch:");
                eprintln!("   Expected: {} actions", expected_actions.len());
                eprintln!("   Actual:   {} actions", response.actions.len());
                eprintln!("   Actual actions: {}", serde_json::to_string_pretty(&response.actions).unwrap_or_default());
                bail!(
                    "Expected {} actions from script, got {}",
                    expected_actions.len(),
                    response.actions.len()
                );
            }

            for (i, (expected, actual)) in expected_actions.iter().zip(response.actions.iter()).enumerate() {
                if expected != actual {
                    eprintln!("\n❌ Script Action {} Mismatch:", i);
                    eprintln!("   Expected: {}", serde_json::to_string_pretty(expected).unwrap_or_default());
                    eprintln!("   Actual:   {}", serde_json::to_string_pretty(actual).unwrap_or_default());
                    bail!(
                        "Script action {} mismatch:\nExpected: {}\nActual: {}",
                        i,
                        serde_json::to_string_pretty(expected)?,
                        serde_json::to_string_pretty(actual)?
                    );
                }
            }

            Ok(())
        }

        Expectation::Custom { name, validator } => {
            if actions.is_empty() {
                bail!("No actions returned");
            }
            validator(&actions[0])
                .with_context(|| format!("Custom validation '{}' failed", name))
        }
    }
}

/// Get description of an expectation
fn expectation_description(expectation: &Expectation) -> String {
    match expectation {
        Expectation::ActionType(t) => format!("Action type is '{}'", t),
        Expectation::FieldExact { field, value } => {
            format!("Field '{}' equals {}", field, value)
        }
        Expectation::FieldContains { field, substring } => {
            format!("Field '{}' contains '{}'", field, substring)
        }
        Expectation::FieldMatches { field, pattern } => {
            format!("Field '{}' matches pattern '{}'", field, pattern)
        }
        Expectation::Protocol(p) => format!("Protocol is '{}'", p),
        Expectation::StaticHandler(v) => format!("Static handler is {}", v),
        Expectation::ScriptHandler => "Has script handler".to_string(),
        Expectation::ScriptWithLanguage(lang) => {
            format!("Has script handler with language '{}'", lang)
        }
        Expectation::ScriptTest { .. } => "Script execution test".to_string(),
        Expectation::Custom { name, .. } => format!("Custom: {}", name),
    }
}
