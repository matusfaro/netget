//! Mock matcher trait and implementations
//!
//! Defines the MockMatcher trait for matching LLM call contexts against rules.

use serde::{Deserialize, Serialize};

/// Context passed to matchers when LLM is called
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct LlmContext {
    /// Event type (e.g., "tcp_connection_received", "http_request")
    pub event_type: Option<String>,

    /// Server/client instruction
    pub instruction: String,

    /// Event data (request details, structured JSON)
    pub event_data: serde_json::Value,

    /// Iteration number for this context (for multi-turn conversations)
    pub iteration: usize,

    /// Message role (user/assistant) for user commands
    pub message_role: Option<String>,

    /// Full prompt text sent to LLM
    pub prompt: String,
}

impl LlmContext {
    /// Create a new LLM context
    pub fn new(prompt: String) -> Self {
        Self {
            event_type: None,
            instruction: String::new(),
            event_data: serde_json::json!({}),
            iteration: 1,
            message_role: None,
            prompt,
        }
    }

    /// Set event type
    pub fn with_event_type(mut self, event_type: impl Into<String>) -> Self {
        self.event_type = Some(event_type.into());
        self
    }

    /// Set instruction
    pub fn with_instruction(mut self, instruction: impl Into<String>) -> Self {
        self.instruction = instruction.into();
        self
    }

    /// Set event data
    pub fn with_event_data(mut self, data: serde_json::Value) -> Self {
        self.event_data = data;
        self
    }

    /// Set iteration
    pub fn with_iteration(mut self, iteration: usize) -> Self {
        self.iteration = iteration;
        self
    }

    /// Set message role
    pub fn with_message_role(mut self, role: impl Into<String>) -> Self {
        self.message_role = Some(role.into());
        self
    }
}

/// Trait for matching LLM call contexts
pub trait MockMatcher: Send + Sync {
    /// Check if context matches this matcher's criteria
    fn matches(&self, context: &LlmContext) -> bool;

    /// Get human-readable description of matching criteria
    fn describe(&self) -> String;
}

/// Matcher for event type
pub struct EventTypeMatcher {
    event_type: String,
}

impl EventTypeMatcher {
    pub fn new(event_type: impl Into<String>) -> Self {
        Self {
            event_type: event_type.into(),
        }
    }
}

impl MockMatcher for EventTypeMatcher {
    fn matches(&self, context: &LlmContext) -> bool {
        context.event_type.as_ref() == Some(&self.event_type)
    }

    fn describe(&self) -> String {
        format!("event_type={}", self.event_type)
    }
}

/// Matcher for instruction substring
pub struct InstructionContainsMatcher {
    substring: String,
}

impl InstructionContainsMatcher {
    pub fn new(substring: impl Into<String>) -> Self {
        Self {
            substring: substring.into(),
        }
    }
}

impl MockMatcher for InstructionContainsMatcher {
    fn matches(&self, context: &LlmContext) -> bool {
        context.instruction.contains(&self.substring)
    }

    fn describe(&self) -> String {
        format!("instruction contains '{}'", self.substring)
    }
}

/// Matcher for instruction regex
pub struct InstructionRegexMatcher {
    regex: regex::Regex,
    pattern: String,
}

impl InstructionRegexMatcher {
    pub fn new(pattern: impl Into<String>) -> Self {
        let pattern = pattern.into();
        let regex = regex::Regex::new(&pattern).expect("Invalid regex pattern");
        Self { regex, pattern }
    }
}

impl MockMatcher for InstructionRegexMatcher {
    fn matches(&self, context: &LlmContext) -> bool {
        self.regex.is_match(&context.instruction)
    }

    fn describe(&self) -> String {
        format!("instruction matches /{}/", self.pattern)
    }
}

/// Matcher for full prompt substring
pub struct PromptContainsMatcher {
    substring: String,
}

impl PromptContainsMatcher {
    pub fn new(substring: impl Into<String>) -> Self {
        Self {
            substring: substring.into(),
        }
    }
}

impl MockMatcher for PromptContainsMatcher {
    fn matches(&self, context: &LlmContext) -> bool {
        context.prompt.contains(&self.substring)
    }

    fn describe(&self) -> String {
        format!("prompt contains '{}'", self.substring)
    }
}

/// Matcher for event data field
pub struct EventDataMatcher {
    key: String,
    value: String,
}

impl EventDataMatcher {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

impl MockMatcher for EventDataMatcher {
    fn matches(&self, context: &LlmContext) -> bool {
        if let Some(field_value) = context.event_data.get(&self.key) {
            let field_str = field_value.as_str().unwrap_or("");
            field_str.contains(&self.value)
        } else {
            false
        }
    }

    fn describe(&self) -> String {
        format!("event_data[{}] contains '{}'", self.key, self.value)
    }
}

/// Matcher for iteration number
pub struct IterationMatcher {
    iteration: usize,
}

impl IterationMatcher {
    pub fn new(iteration: usize) -> Self {
        Self { iteration }
    }
}

impl MockMatcher for IterationMatcher {
    fn matches(&self, context: &LlmContext) -> bool {
        context.iteration == self.iteration
    }

    fn describe(&self) -> String {
        format!("iteration={}", self.iteration)
    }
}

/// Matcher for message role
pub struct MessageRoleMatcher {
    role: String,
}

impl MessageRoleMatcher {
    pub fn new(role: impl Into<String>) -> Self {
        Self {
            role: role.into(),
        }
    }
}

impl MockMatcher for MessageRoleMatcher {
    fn matches(&self, context: &LlmContext) -> bool {
        context.message_role.as_ref() == Some(&self.role)
    }

    fn describe(&self) -> String {
        format!("message_role={}", self.role)
    }
}

/// Matcher for custom function
pub struct CustomMatcher<F>
where
    F: Fn(&LlmContext) -> bool + Send + Sync,
{
    matcher_fn: F,
}

impl<F> CustomMatcher<F>
where
    F: Fn(&LlmContext) -> bool + Send + Sync,
{
    pub fn new(matcher_fn: F) -> Self {
        Self { matcher_fn }
    }
}

impl<F> MockMatcher for CustomMatcher<F>
where
    F: Fn(&LlmContext) -> bool + Send + Sync,
{
    fn matches(&self, context: &LlmContext) -> bool {
        (self.matcher_fn)(context)
    }

    fn describe(&self) -> String {
        "custom matcher".to_string()
    }
}

/// Matcher that matches everything (fallback)
pub struct AnyMatcher;

impl MockMatcher for AnyMatcher {
    fn matches(&self, _context: &LlmContext) -> bool {
        true
    }

    fn describe(&self) -> String {
        "match any".to_string()
    }
}

/// Combined matcher (all must match)
pub struct CombinedMatcher {
    matchers: Vec<Box<dyn MockMatcher>>,
}

impl CombinedMatcher {
    pub fn new(matchers: Vec<Box<dyn MockMatcher>>) -> Self {
        Self { matchers }
    }

    pub fn add(&mut self, matcher: Box<dyn MockMatcher>) {
        self.matchers.push(matcher);
    }
}

impl MockMatcher for CombinedMatcher {
    fn matches(&self, context: &LlmContext) -> bool {
        self.matchers.iter().all(|m| m.matches(context))
    }

    fn describe(&self) -> String {
        let descriptions: Vec<String> = self.matchers.iter().map(|m| m.describe()).collect();
        descriptions.join(" AND ")
    }
}
