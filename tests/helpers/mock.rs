//! Mock LLM testing utilities
//!
//! Re-exports of mock types for convenient use in tests

// Re-export from main crate
pub use netget::testing::{
    LlmContext, MockLlmBuilder, MockLlmConfig, MockMatcher, MockResponse, MockResponseBuilder,
    MockRule, MockRuleBuilder,
};

// Re-export serde_json for convenience in tests
pub use serde_json::json;
