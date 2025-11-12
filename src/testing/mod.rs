//! Testing utilities for NetGet
//!
//! This module provides mock LLM infrastructure for E2E testing without requiring
//! a real Ollama instance. Tests configure expected LLM responses using a builder
//! pattern and verify that expectations are met.

pub mod mock_builder;
pub mod mock_config;
pub mod mock_matcher;

// Re-export main types
pub use mock_builder::{MockLlmBuilder, MockResponseBuilder, MockRuleBuilder};
pub use mock_config::{MockLlmConfig, MockResponse, MockRule};
pub use mock_matcher::{LlmContext, MockMatcher};
