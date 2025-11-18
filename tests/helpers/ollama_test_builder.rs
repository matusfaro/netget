use anyhow::{Context, Result, anyhow, bail};
use serde_json::{json, Value};
use std::sync::Arc;

use netget::events::{Event, EventType};
use netget::llm::OllamaClient;

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
        prompt: String,
        available_actions: Vec<Value>,
    },
    /// Network request event
    NetworkRequest {
        event: Event,
        instruction: String,
        available_actions: Vec<Value>,
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
    pub fn with_user_input(mut self, prompt: impl Into<String>) -> Self {
        // Get global actions for user commands
        let available_actions = vec![
            json!({
                "type": "open_server",
                "description": "Open a new server on a specified protocol",
                "parameters": {
                    "protocol": "The protocol to use (e.g., 'tcp', 'http', 'dns')",
                    "port": "Port number (optional, will auto-assign if not provided)",
                    "instruction": "Instructions for how the server should behave",
                    "handler": "Handler configuration (optional): 'static' (JSON value) or 'script' (Lua code)",
                    "scheduled_tasks": "Array of scheduled tasks (optional)"
                }
            }),
            json!({
                "type": "open_client",
                "description": "Open a new client connection to a remote server",
                "parameters": {
                    "protocol": "The protocol to use (e.g., 'tcp', 'http', 'redis')",
                    "remote_addr": "Remote address (host:port)",
                    "instruction": "Instructions for how the client should behave"
                }
            }),
            json!({
                "type": "close_server",
                "description": "Close a running server",
                "parameters": {
                    "server_id": "ID of the server to close"
                }
            }),
            json!({
                "type": "web_search",
                "description": "Search the web for information",
                "parameters": {
                    "query": "Search query"
                }
            }),
        ];

        self.prompt_context = Some(PromptContext::UserInput {
            prompt: prompt.into(),
            available_actions,
        });
        self
    }

    /// Set network request context
    pub fn with_network_request(
        mut self,
        event: Event,
        instruction: impl Into<String>,
        available_actions: Vec<Value>,
    ) -> Self {
        self.prompt_context = Some(PromptContext::NetworkRequest {
            event,
            instruction: instruction.into(),
            available_actions,
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
        // Get model from env var or use default
        let model = self.model
            .or_else(|| std::env::var("OLLAMA_MODEL").ok())
            .unwrap_or_else(|| "qwen2.5-coder:7b".to_string());

        println!("Testing with model: {}", model);

        // Build prompt based on context
        let prompt_context = self.prompt_context
            .ok_or_else(|| anyhow!("No prompt context set"))?;

        let (prompt, available_actions) = match prompt_context {
            PromptContext::UserInput { prompt, available_actions } => {
                let full_prompt = format!(
                    "You are a network protocol assistant. The user said: \"{}\"\n\n\
                     Analyze the user's request and respond with ONE action from the available actions.\n\n\
                     Available actions:\n{}\n\n\
                     Respond with a JSON array containing exactly one action object.",
                    prompt,
                    serde_json::to_string_pretty(&available_actions)?
                );
                (full_prompt, available_actions)
            }
            PromptContext::NetworkRequest { event, instruction, available_actions } => {
                let full_prompt = format!(
                    "You are handling a network request for a server with this instruction: \"{}\"\n\n\
                     Network event received:\n{}\n\n\
                     Available actions:\n{}\n\n\
                     Respond with a JSON array of action objects to handle this request.",
                    instruction,
                    serde_json::to_string_pretty(&event.to_json()?)?,
                    serde_json::to_string_pretty(&available_actions)?
                );
                (full_prompt, available_actions)
            }
        };

        // Call Ollama
        let ollama_client = OllamaClient::new(
            std::env::var("OLLAMA_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:11434".to_string()),
            model.clone(),
        )?;

        let response = ollama_client.chat(&prompt).await
            .context("Failed to call Ollama API")?;

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
                    println!("✓ PASS: {}", desc);
                    passed.push(desc);
                }
                Err(e) => {
                    let desc = expectation_description(&expectation);
                    println!("✗ FAIL: {}: {}", desc, e);
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
fn parse_actions_from_response(response: &str) -> Result<Vec<Value>> {
    // Try to find JSON array in response
    let trimmed = response.trim();

    // Try parsing as direct JSON array
    if let Ok(actions) = serde_json::from_str::<Vec<Value>>(trimmed) {
        return Ok(actions);
    }

    // Try to extract JSON from markdown code block
    if let Some(json_str) = extract_json_from_markdown(trimmed) {
        if let Ok(actions) = serde_json::from_str::<Vec<Value>>(&json_str) {
            return Ok(actions);
        }
    }

    // Try to find any JSON array in the text
    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            let json_str = &trimmed[start..=end];
            if let Ok(actions) = serde_json::from_str::<Vec<Value>>(json_str) {
                return Ok(actions);
            }
        }
    }

    bail!("Could not parse actions from response: {}", response)
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
                bail!("Expected action type '{}', got '{}'", expected_type, actual_type);
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
                bail!(
                    "Expected field '{}' to be {}, got {}",
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
                bail!(
                    "Expected field '{}' to contain '{}', got '{}'",
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
                    bail!(
                        "Expected static handler value {}, got {}",
                        expected_value,
                        static_value
                    );
                }
                return Ok(());
            }

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
                bail!("Action does not have a script handler");
            }

            // Check if language matches
            if let Some(language) = handler.get("language") {
                let actual_lang = language.as_str()
                    .ok_or_else(|| anyhow!("Language is not a string"))?;
                if actual_lang.to_lowercase() != expected_lang.to_lowercase() {
                    bail!(
                        "Expected script language '{}', got '{}'",
                        expected_lang,
                        actual_lang
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
        Expectation::Custom { name, .. } => format!("Custom: {}", name),
    }
}
