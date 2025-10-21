//! LLM integration module
//!
//! Provides integration with Ollama for LLM-driven protocol handling

pub mod actions;
pub mod ollama_client;
pub mod prompt;

// Keep the old client module for reference but use ollama_client
pub mod client;

pub use actions::Action;
pub use ollama_client::{CommandAction, CommandInterpretation, HttpLlmResponse, LlmResponse, OllamaClient};
pub use prompt::PromptBuilder;
