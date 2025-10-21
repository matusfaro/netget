//! LLM integration module
//!
//! Provides integration with Ollama for LLM-driven protocol handling

pub mod client;
pub mod prompt;

pub use client::{OllamaClient, LlmResponse};
pub use prompt::PromptBuilder;
