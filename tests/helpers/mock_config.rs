//! Mock LLM configuration types
//!
//! Defines the data structures for configuring mock LLM responses in tests.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

use super::mock_matcher::{LlmContext, MockMatcher};

/// Function that generates mock actions dynamically from event data
///
/// This enables protocol-correct responses for correlation IDs (DNS query_id, STUN transaction_id, etc.)
/// The closure receives event data extracted from the LLM prompt and returns actions.
pub type ResponseGenerator = Arc<dyn Fn(&serde_json::Value) -> Vec<serde_json::Value> + Send + Sync>;

/// Complete mock LLM configuration
#[derive(Serialize, Deserialize)]
pub struct MockLlmConfig {
    /// Rules to match against LLM calls (evaluated in order)
    #[serde(skip)]
    pub rules: Vec<MockRule>,

    /// Serialized rules (for passing via environment variable)
    #[serde(default)]
    pub serialized_rules: Vec<SerializedMockRule>,

    /// Whether verify() was called
    #[serde(skip)]
    pub was_verified: Arc<AtomicBool>,

    /// History of all LLM calls (for debugging)
    #[serde(skip)]
    pub call_history: Arc<Mutex<Vec<MockCallRecord>>>,
}

impl Clone for MockLlmConfig {
    fn clone(&self) -> Self {
        Self {
            rules: self.rules.iter().map(|r| r.shallow_clone()).collect(),
            serialized_rules: self.serialized_rules.clone(),
            was_verified: Arc::clone(&self.was_verified),
            call_history: Arc::clone(&self.call_history),
        }
    }
}

impl MockLlmConfig {
    /// Create a new mock configuration
    pub fn new(rules: Vec<MockRule>) -> Self {
        // Serialize rules for environment variable passing
        let serialized_rules = rules
            .iter()
            .map(|r| r.to_serialized())
            .collect();

        Self {
            rules,
            serialized_rules,
            was_verified: Arc::new(AtomicBool::new(false)),
            call_history: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Create from serialized format (from environment variable)
    pub fn from_serialized(serialized: Vec<SerializedMockRule>) -> Self {
        let rules = serialized
            .iter()
            .map(|s| MockRule::from_serialized(s.clone()))
            .collect();

        Self {
            rules,
            serialized_rules: serialized,
            was_verified: Arc::new(AtomicBool::new(false)),
            call_history: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Find matching response for the given context
    pub async fn find_match(&self, context: &LlmContext) -> Option<(usize, MockResponse)> {
        for (idx, rule) in self.rules.iter().enumerate() {
            if rule.matches(context) {
                // Increment call count
                rule.actual_calls.fetch_add(1, Ordering::SeqCst);

                // Record call
                let mut history = self.call_history.lock().await;
                history.push(MockCallRecord {
                    context: context.clone(),
                    matched_rule_idx: idx,
                    rule_description: rule.describe(),
                });

                return Some((idx, rule.response.clone()));
            }
        }

        None
    }

    /// Mark as verified
    pub fn mark_verified(&self) {
        self.was_verified.store(true, Ordering::SeqCst);
    }

    /// Check if verified
    pub fn is_verified(&self) -> bool {
        self.was_verified.load(Ordering::SeqCst)
    }
}

/// A single mock rule with matching criteria and response
pub struct MockRule {
    /// Matcher for this rule (trait object, not serializable)
    #[allow(dead_code)]
    matcher: Option<Box<dyn MockMatcher>>,

    /// Serialized matcher data
    serialized_matcher: SerializedMatcher,

    /// Response to return
    pub response: MockResponse,

    /// Expected exact number of invocations (None = any number)
    pub expected_calls: Option<usize>,

    /// Minimum number of invocations
    pub min_calls: Option<usize>,

    /// Maximum number of invocations
    pub max_calls: Option<usize>,

    /// Actual number of invocations (tracked at runtime)
    pub actual_calls: Arc<AtomicUsize>,
}

impl MockRule {
    /// Create a new mock rule
    pub fn new(
        matcher: Box<dyn MockMatcher>,
        serialized_matcher: SerializedMatcher,
        response: MockResponse,
    ) -> Self {
        Self {
            matcher: Some(matcher),
            serialized_matcher,
            response,
            expected_calls: None,
            min_calls: None,
            max_calls: None,
            actual_calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Check if context matches this rule
    pub fn matches(&self, context: &LlmContext) -> bool {
        // Use serialized matcher for matching (works across process boundaries)
        self.serialized_matcher.matches(context)
    }

    /// Get description of this rule
    pub fn describe(&self) -> String {
        self.serialized_matcher.describe()
    }

    /// Convert to serialized format
    pub fn to_serialized(&self) -> SerializedMockRule {
        SerializedMockRule {
            matcher: self.serialized_matcher.clone(),
            response: self.response.clone(),
            expected_calls: self.expected_calls,
            min_calls: self.min_calls,
            max_calls: self.max_calls,
        }
    }

    /// Create from serialized format
    pub fn from_serialized(s: SerializedMockRule) -> Self {
        Self {
            matcher: None, // Matcher not needed for deserialized rules
            serialized_matcher: s.matcher,
            response: s.response,
            expected_calls: s.expected_calls,
            min_calls: s.min_calls,
            max_calls: s.max_calls,
            actual_calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Shallow clone (clones Arc references, not underlying data)
    pub fn shallow_clone(&self) -> Self {
        Self {
            matcher: None, // Don't clone the matcher (not needed after serialization)
            serialized_matcher: self.serialized_matcher.clone(),
            response: self.response.clone(),
            expected_calls: self.expected_calls,
            min_calls: self.min_calls,
            max_calls: self.max_calls,
            actual_calls: Arc::clone(&self.actual_calls),
        }
    }
}

/// Serializable mock rule (for environment variable passing)
#[derive(Clone, Serialize, Deserialize)]
pub struct SerializedMockRule {
    pub matcher: SerializedMatcher,
    pub response: MockResponse,
    pub expected_calls: Option<usize>,
    pub min_calls: Option<usize>,
    pub max_calls: Option<usize>,
}

/// Serializable matcher (combines all matching criteria)
#[derive(Clone, Serialize, Deserialize)]
pub struct SerializedMatcher {
    /// Event type exact match
    pub event_type: Option<String>,

    /// Instruction substring match
    pub instruction_contains: Vec<String>,

    /// Instruction regex match
    pub instruction_regex: Option<String>,

    /// Event data field matches (key -> value substring)
    pub event_data_contains: Vec<(String, String)>,

    /// Iteration number
    pub iteration: Option<usize>,

    /// Message role
    pub message_role: Option<String>,

    /// Prompt substring match
    pub prompt_contains: Vec<String>,

    /// Match any (fallback)
    pub match_any: bool,
}

impl SerializedMatcher {
    /// Create a new serialized matcher
    pub fn new() -> Self {
        Self {
            event_type: None,
            instruction_contains: Vec::new(),
            instruction_regex: None,
            event_data_contains: Vec::new(),
            iteration: None,
            message_role: None,
            prompt_contains: Vec::new(),
            match_any: false,
        }
    }

    /// Check if context matches all criteria
    pub fn matches(&self, context: &LlmContext) -> bool {
        // Match any (fallback)
        if self.match_any {
            return true;
        }

        // Event type
        if let Some(ref event_type) = self.event_type {
            if context.event_type.as_ref() != Some(event_type) {
                return false;
            }
        }

        // CRITICAL FIX: Instruction-based matching should only match user input, not network events
        // If this rule uses instruction matching but the context has an event_type,
        // it means this is a network event (not user input), so skip instruction matching
        let has_instruction_criteria = !self.instruction_contains.is_empty() || self.instruction_regex.is_some();
        let is_network_event = context.event_type.is_some();

        if has_instruction_criteria && is_network_event && self.event_type.is_none() {
            // This rule is instruction-based (meant for user input) but context is a network event
            // Don't match unless the rule explicitly specifies an event_type
            return false;
        }

        // Instruction contains (all must match)
        for substring in &self.instruction_contains {
            if !context.instruction.contains(substring) {
                return false;
            }
        }

        // Instruction regex
        if let Some(ref regex_str) = self.instruction_regex {
            if let Ok(regex) = regex::Regex::new(regex_str) {
                if !regex.is_match(&context.instruction) {
                    return false;
                }
            } else {
                return false;
            }
        }

        // Event data contains
        for (key, value) in &self.event_data_contains {
            if let Some(field_value) = context.event_data.get(key) {
                // Handle both string and numeric values
                let field_str = if let Some(s) = field_value.as_str() {
                    s.to_string()
                } else if let Some(n) = field_value.as_i64() {
                    n.to_string()
                } else if let Some(n) = field_value.as_u64() {
                    n.to_string()
                } else if let Some(n) = field_value.as_f64() {
                    n.to_string()
                } else {
                    // For other types (bool, null, array, object), use JSON representation
                    field_value.to_string()
                };
                if !field_str.contains(value) {
                    return false;
                }
            } else {
                return false;
            }
        }

        // Iteration
        if let Some(expected_iter) = self.iteration {
            if context.iteration != expected_iter {
                return false;
            }
        }

        // Message role
        if let Some(ref role) = self.message_role {
            if context.message_role.as_ref() != Some(role) {
                return false;
            }
        }

        // Prompt contains
        for substring in &self.prompt_contains {
            if !context.prompt.contains(substring) {
                return false;
            }
        }

        true
    }

    /// Get description of matching criteria
    pub fn describe(&self) -> String {
        if self.match_any {
            return "match any".to_string();
        }

        let mut parts = Vec::new();

        if let Some(ref event_type) = self.event_type {
            parts.push(format!("event={}", event_type));
        }

        if !self.instruction_contains.is_empty() {
            parts.push(format!("instruction contains {:?}", self.instruction_contains));
        }

        if let Some(ref regex) = self.instruction_regex {
            parts.push(format!("instruction matches /{}/", regex));
        }

        if !self.event_data_contains.is_empty() {
            parts.push(format!("event_data {:?}", self.event_data_contains));
        }

        if let Some(iter) = self.iteration {
            parts.push(format!("iteration={}", iter));
        }

        if let Some(ref role) = self.message_role {
            parts.push(format!("role={}", role));
        }

        if !self.prompt_contains.is_empty() {
            parts.push(format!("prompt contains {:?}", self.prompt_contains));
        }

        if parts.is_empty() {
            "no criteria".to_string()
        } else {
            parts.join(", ")
        }
    }
}

impl Default for SerializedMatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Response types
pub enum MockResponse {
    /// Valid action response (parsed as JSON)
    Actions { actions: Vec<serde_json::Value> },

    /// Dynamic actions generated from event data (not serializable)
    ///
    /// This variant allows mock responses to access event data at runtime,
    /// enabling protocol-correct responses for correlation IDs.
    /// Cannot be serialized to environment variables.
    DynamicActions { generator: ResponseGenerator },

    /// Raw string (for testing LLM failures/malformed responses)
    Raw { content: String },
}

impl MockResponse {
    /// Convert to string for LLM response
    ///
    /// # Arguments
    /// * `event_data` - Event data from LLM context (for dynamic responses)
    pub fn to_response_string(&self, event_data: Option<&serde_json::Value>) -> String {
        match self {
            MockResponse::Actions { actions } => {
                // Wrap actions in ActionResponse format
                serde_json::json!({
                    "actions": actions
                })
                .to_string()
            }

            MockResponse::DynamicActions { generator } => {
                let empty_obj = serde_json::json!({});
                let event_data = event_data.unwrap_or(&empty_obj);
                let actions = generator(event_data);
                serde_json::json!({
                    "actions": actions
                })
                .to_string()
            }

            MockResponse::Raw { content } => content.clone(),
        }
    }
}

// Manual Clone implementation (can't derive due to DynamicActions)
impl Clone for MockResponse {
    fn clone(&self) -> Self {
        match self {
            MockResponse::Actions { actions } => {
                MockResponse::Actions { actions: actions.clone() }
            }
            MockResponse::DynamicActions { generator } => {
                MockResponse::DynamicActions { generator: Arc::clone(generator) }
            }
            MockResponse::Raw { content } => {
                MockResponse::Raw { content: content.clone() }
            }
        }
    }
}

// Manual Serialize implementation (DynamicActions cannot be serialized)
impl Serialize for MockResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        match self {
            MockResponse::Actions { actions } => {
                let mut state = serializer.serialize_struct("MockResponse", 2)?;
                state.serialize_field("type", "actions")?;
                state.serialize_field("actions", actions)?;
                state.end()
            }
            MockResponse::DynamicActions { .. } => {
                Err(serde::ser::Error::custom(
                    "DynamicActions cannot be serialized. \
                     Dynamic mocks require MOCK_OLLAMA_BASE_URL (in-process mock server), \
                     not NETGET_MOCK_LLM_CONFIG (environment variable serialization)."
                ))
            }
            MockResponse::Raw { content } => {
                let mut state = serializer.serialize_struct("MockResponse", 2)?;
                state.serialize_field("type", "raw")?;
                state.serialize_field("content", content)?;
                state.end()
            }
        }
    }
}

// Manual Deserialize implementation (DynamicActions intentionally not deserializable)
impl<'de> Deserialize<'de> for MockResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum MockResponseHelper {
            Actions { actions: Vec<serde_json::Value> },
            Raw { content: String },
            // DynamicActions intentionally not deserializable
        }

        let helper = MockResponseHelper::deserialize(deserializer)?;
        Ok(match helper {
            MockResponseHelper::Actions { actions } => MockResponse::Actions { actions },
            MockResponseHelper::Raw { content } => MockResponse::Raw { content },
        })
    }
}

/// Record of a mock LLM call
#[derive(Clone)]
pub struct MockCallRecord {
    /// Context of the call
    pub context: LlmContext,
    /// Index of matched rule
    pub matched_rule_idx: usize,
    /// Description of matched rule
    pub rule_description: String,
}
