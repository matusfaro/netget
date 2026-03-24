//! LLM integration module
//!
//! Provides integration with Ollama for LLM-driven protocol handling

// New action system
pub mod action_helper;
pub mod actions; // Centralized helper for LLM calls
pub mod conversation; // Conversation-based LLM interaction
pub mod conversation_state; // Conversation history management
pub mod default_instructions; // Default instructions registry
pub mod event_handler_executor; // Event handler execution (script/static/llm)
pub mod event_instructions; // Event-specific instructions
pub mod model_selection;
pub mod rate_limiter; // Rate limiting for LLM calls (concurrency + token throttling)
pub mod reference_parser; // XML reference parser for large content blocks
pub mod template_engine; // Handlebars template engine // Model selection utilities
pub mod ollama_client;
pub mod prompt;
pub mod response_handler; // Keep the old client module for reference

// Embedded LLM inference (feature-gated)
#[cfg(feature = "embedded-llm")]
pub mod embedded_inference;

// Configuration persistence
pub mod config;

// Hybrid LLM manager (Ollama + embedded fallback)
#[cfg(feature = "embedded-llm")]
pub mod hybrid_manager;

// Re-exports from new action system
pub use actions::{
    common::{
        get_all_common_actions, get_network_event_common_actions, get_user_input_common_actions,
        CommonAction,
    },
    executor::{execute_actions, ExecutionResult},
    protocol_trait::{ActionResult, Server},
    ActionDefinition, ActionResponse, Parameter,
};

// Re-export action helper functions
pub use action_helper::{
    call_llm_for_client, call_llm_with_actions, call_llm_with_custom_actions,
    call_llm_with_protocol, ClientLlmResult,
};

// Current ollama client exports
pub use ollama_client::{
    CommandAction, CommandInterpretation, GenerateResponse, HttpLlmResponse, LlmResponse,
    OllamaClient, TokenUsage,
};
pub use prompt::PromptBuilder;
pub use response_handler::{handle_llm_response, ProcessedResponse};

// Conversation handler
pub use conversation::ConversationHandler;

// Conversation state management
pub use conversation_state::{ConversationMessage, ConversationState, MessageRole, MessageType};

// Message type from ollama_client module
pub use ollama_client::Message;

// Event instructions
pub use default_instructions::{resolve_instructions, DEFAULT_INSTRUCTIONS};
pub use event_instructions::{
    EventInstructions, Example, InstructionSource, ServerInstructionConfig,
};

// Model selection
pub use model_selection::{
    check_ollama_availability, ensure_model_selected, select_or_validate_model, ModelInfo,
};

// Embedded LLM inference (feature-gated exports)
#[cfg(feature = "embedded-llm")]
pub use embedded_inference::{EmbeddedLLMBackend, InferenceConfig};

// Configuration exports
pub use config::{LlmBackendType, NetGetConfig, OllamaConfig, OpenAIConfig};
#[cfg(feature = "embedded-llm")]
pub use config::EmbeddedLlmConfig;

// Hybrid manager exports
#[cfg(feature = "embedded-llm")]
pub use hybrid_manager::{ActiveBackend, HybridLLMManager};

// Rate limiter
pub use rate_limiter::{
    RateLimiter, RateLimiterConfig, RateLimiterPermit, RateLimiterStats, RequestSource,
};

/// Format text with indentation and ANSI dim styling for log output.
///
/// Each line is prefixed with the specified number of spaces and wrapped
/// with ANSI dim codes (\x1b[2m ... \x1b[0m) for greyed out appearance.
///
/// # Arguments
/// * `text` - The text to format
/// * `indent_spaces` - Number of spaces to indent each line (default: 8)
///
/// # Returns
/// Vec of formatted lines, each indented and dimmed (ready to send via tx.send())
pub fn format_indented_dimmed_lines(text: &str, indent_spaces: usize) -> Vec<String> {
    let indent = " ".repeat(indent_spaces);
    text.lines()
        .map(|line| format!("{}\x1b[2m{}\x1b[0m", indent, line))
        .collect()
}

/// Format text with indentation and ANSI dim styling for log output.
/// Returns a single string with lines joined by newlines (for tracing/debug output).
///
/// # Arguments
/// * `text` - The text to format
/// * `indent_spaces` - Number of spaces to indent each line (default: 8)
///
/// # Returns
/// Formatted string with each line indented and dimmed, joined by \n
pub fn format_indented_dimmed(text: &str, indent_spaces: usize) -> String {
    format_indented_dimmed_lines(text, indent_spaces).join("\n")
}
