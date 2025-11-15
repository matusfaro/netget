//! Builder API for creating mock LLM configurations
//!
//! Provides a fluent interface for configuring mock LLM responses in tests.

use crate::helpers::mock_config::{MockLlmConfig, MockResponse, MockRule, SerializedMatcher};
use crate::helpers::mock_matcher::{
    AnyMatcher, CombinedMatcher, CustomMatcher, EventDataMatcher, EventTypeMatcher,
    InstructionContainsMatcher, InstructionRegexMatcher, IterationMatcher, MessageRoleMatcher,
    MockMatcher,
};

/// Builder for creating mock LLM configurations
pub struct MockLlmBuilder {
    rules: Vec<MockRule>,
}

impl MockLlmBuilder {
    /// Create a new mock LLM builder
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Match on event type
    pub fn on_event(self, event_type: impl Into<String>) -> MockRuleBuilder {
        let event_type = event_type.into();
        let mut serialized = SerializedMatcher::new();
        serialized.event_type = Some(event_type.clone());

        MockRuleBuilder::new(
            self,
            Box::new(EventTypeMatcher::new(event_type)),
            serialized,
        )
    }

    /// Match on instruction substring
    pub fn on_instruction_containing(self, substring: impl Into<String>) -> MockRuleBuilder {
        let substring = substring.into();
        let mut serialized = SerializedMatcher::new();
        serialized.instruction_contains.push(substring.clone());

        MockRuleBuilder::new(
            self,
            Box::new(InstructionContainsMatcher::new(substring)),
            serialized,
        )
    }

    /// Match on instruction regex
    pub fn on_instruction_regex(self, regex: impl Into<String>) -> MockRuleBuilder {
        let regex = regex.into();
        let mut serialized = SerializedMatcher::new();
        serialized.instruction_regex = Some(regex.clone());

        MockRuleBuilder::new(
            self,
            Box::new(InstructionRegexMatcher::new(regex)),
            serialized,
        )
    }

    /// Match on custom function
    pub fn on_custom<F>(self, matcher: F) -> MockRuleBuilder
    where
        F: Fn(&super::mock_matcher::LlmContext) -> bool + Send + Sync + 'static,
    {
        use crate::helpers::mock_matcher::CustomMatcher;

        // Custom matchers can't be serialized, so we use match_any
        let mut serialized = SerializedMatcher::new();
        serialized.match_any = true;

        MockRuleBuilder::new(self, Box::new(CustomMatcher::new(matcher)), serialized)
    }

    /// Default fallback (matches everything)
    pub fn on_any(self) -> MockRuleBuilder {
        let mut serialized = SerializedMatcher::new();
        serialized.match_any = true;

        MockRuleBuilder::new(self, Box::new(AnyMatcher), serialized)
    }

    /// Build the configuration
    pub fn build(self) -> MockLlmConfig {
        MockLlmConfig::new(self.rules)
    }
}

impl Default for MockLlmBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for a single rule
pub struct MockRuleBuilder {
    parent: MockLlmBuilder,
    matcher: Box<dyn MockMatcher>,
    serialized_matcher: SerializedMatcher,
}

impl MockRuleBuilder {
    fn new(
        parent: MockLlmBuilder,
        matcher: Box<dyn MockMatcher>,
        serialized_matcher: SerializedMatcher,
    ) -> Self {
        Self {
            parent,
            matcher,
            serialized_matcher,
        }
    }

    /// Add additional matching criteria (all must match)
    pub fn and_instruction_containing(mut self, substring: impl Into<String>) -> Self {
        let substring = substring.into();

        // Add to combined matcher
        let mut combined = CombinedMatcher::new(vec![self.matcher]);
        combined.add(Box::new(InstructionContainsMatcher::new(substring.clone())));
        self.matcher = Box::new(combined);

        // Add to serialized
        self.serialized_matcher.instruction_contains.push(substring);

        self
    }

    /// Add event data matching criteria
    pub fn and_event_data_contains(
        mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        let key = key.into();
        let value = value.into();

        // Add to combined matcher
        let mut combined = CombinedMatcher::new(vec![self.matcher]);
        combined.add(Box::new(EventDataMatcher::new(key.clone(), value.clone())));
        self.matcher = Box::new(combined);

        // Add to serialized
        self.serialized_matcher
            .event_data_contains
            .push((key, value));

        self
    }

    /// Add iteration matching criteria
    pub fn and_iteration(mut self, iteration: usize) -> Self {
        // Add to combined matcher
        let mut combined = CombinedMatcher::new(vec![self.matcher]);
        combined.add(Box::new(IterationMatcher::new(iteration)));
        self.matcher = Box::new(combined);

        // Add to serialized
        self.serialized_matcher.iteration = Some(iteration);

        self
    }

    /// Add message role matching criteria
    pub fn and_message_role(mut self, role: impl Into<String>) -> Self {
        let role = role.into();

        // Add to combined matcher
        let mut combined = CombinedMatcher::new(vec![self.matcher]);
        combined.add(Box::new(MessageRoleMatcher::new(role.clone())));
        self.matcher = Box::new(combined);

        // Add to serialized
        self.serialized_matcher.message_role = Some(role);

        self
    }

    /// Respond with action JSON
    pub fn respond_with_actions(self, actions: serde_json::Value) -> MockResponseBuilder {
        let actions_vec = if actions.is_array() {
            actions.as_array().unwrap().clone()
        } else {
            vec![actions]
        };

        MockResponseBuilder::new(
            self,
            MockResponse::Actions {
                actions: actions_vec,
            },
        )
    }

    /// Respond with actions generated dynamically from event data
    ///
    /// This allows mock responses to access event data at runtime, enabling
    /// protocol-correct responses for correlation IDs (DNS query_id, STUN transaction_id, etc.)
    ///
    /// # Example
    /// ```rust,ignore
    /// .respond_with_actions_from_event(|event_data| {
    ///     serde_json::json!([{
    ///         "type": "send_dns_a_response",
    ///         "query_id": event_data["query_id"],  // Dynamic from event
    ///         "domain": "example.com",
    ///         "ip": "93.184.216.34"
    ///     }])
    /// })
    /// ```
    ///
    /// # Arguments
    /// * `generator` - Closure that takes event data and returns action JSON (array or single object)
    ///
    /// # Returns
    /// MockResponseBuilder for chaining (.expect_calls(), etc.)
    ///
    /// # Note
    /// Dynamic mocks cannot be serialized to environment variables.
    /// Use with MOCK_OLLAMA_BASE_URL (in-process mock server) only.
    pub fn respond_with_actions_from_event<F>(self, generator: F) -> MockResponseBuilder
    where
        F: Fn(&serde_json::Value) -> serde_json::Value + Send + Sync + 'static,
    {
        use crate::helpers::mock_config::ResponseGenerator;
        use std::sync::Arc;

        // Wrap user's closure to handle array/single-action normalization
        let wrapped_generator: ResponseGenerator =
            Arc::new(move |event_data: &serde_json::Value| {
                let result = generator(event_data);
                if result.is_array() {
                    result.as_array().unwrap().clone()
                } else {
                    vec![result]
                }
            });

        MockResponseBuilder::new(
            self,
            MockResponse::DynamicActions {
                generator: wrapped_generator,
            },
        )
    }

    /// Respond with raw string (for testing failures)
    pub fn respond_with_raw(self, raw: impl Into<String>) -> MockResponseBuilder {
        MockResponseBuilder::new(
            self,
            MockResponse::Raw {
                content: raw.into(),
            },
        )
    }
}

/// Builder for response expectations
pub struct MockResponseBuilder {
    rule_builder: MockRuleBuilder,
    response: MockResponse,
    expected_calls: Option<usize>,
    min_calls: Option<usize>,
    max_calls: Option<usize>,
}

impl MockResponseBuilder {
    fn new(rule_builder: MockRuleBuilder, response: MockResponse) -> Self {
        Self {
            rule_builder,
            response,
            expected_calls: None,
            min_calls: None,
            max_calls: None,
        }
    }

    /// Set expected number of invocations (fails if not met)
    pub fn expect_calls(mut self, count: usize) -> Self {
        self.expected_calls = Some(count);
        self
    }

    /// Expect at least N calls
    pub fn expect_at_least(mut self, count: usize) -> Self {
        self.min_calls = Some(count);
        self
    }

    /// Expect at most N calls
    pub fn expect_at_most(mut self, count: usize) -> Self {
        self.max_calls = Some(count);
        self
    }

    /// Finish this rule and continue building
    pub fn and(self) -> MockLlmBuilder {
        // Create completed rule
        let mut rule = MockRule::new(
            self.rule_builder.matcher,
            self.rule_builder.serialized_matcher,
            self.response,
        );

        rule.expected_calls = self.expected_calls;
        rule.min_calls = self.min_calls;
        rule.max_calls = self.max_calls;

        // Add to parent
        let mut parent = self.rule_builder.parent;
        parent.rules.push(rule);
        parent
    }

    /// Finish this rule and build the configuration
    pub fn build(self) -> MockLlmConfig {
        self.and().build()
    }
}
