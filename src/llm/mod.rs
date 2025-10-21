//! LLM integration module
//!
//! Provides integration with Ollama for LLM-driven protocol handling

pub mod actions;
pub mod client;
pub mod prompt;

pub use actions::{Action, CommandInterpretation};
pub use client::{HttpLlmResponse, LlmResponse, OllamaClient};
pub use prompt::PromptBuilder;
