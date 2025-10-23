//! LLM integration module
//!
//! Provides integration with Ollama for LLM-driven protocol handling

// New action system
pub mod actions;

// Old modules
pub mod old_actions;  // Legacy action system, to be removed
pub mod ollama_client;
pub mod prompt;
pub mod response_handler;
pub mod client;  // Keep the old client module for reference

// Re-exports from new action system
pub use actions::{
    ActionDefinition, ActionResponse, Parameter,
    common::{CommonAction, get_all_common_actions, get_user_input_common_actions, get_network_event_common_actions},
    context::NetworkContext,
    protocol_trait::{ActionResult, ProtocolActions},
    executor::{execute_actions, ExecutionResult},
};

// Legacy re-exports (for backward compatibility during migration)
pub use old_actions::{Action as OldAction, CommandInterpretation as OldCommandInterpretation};

// Current ollama client exports
pub use ollama_client::{CommandAction, CommandInterpretation, HttpLlmResponse, LlmResponse, OllamaClient};
pub use prompt::PromptBuilder;
pub use response_handler::{handle_llm_response, ProcessedResponse};
