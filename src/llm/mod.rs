//! LLM integration module
//!
//! Provides integration with Ollama for LLM-driven protocol handling

// New action system
pub mod action_helper;
pub mod actions; // Centralized helper for LLM calls
pub mod conversation; // Conversation-based LLM interaction
pub mod conversation_state; // Conversation history management
pub mod template_engine; // Handlebars template engine
pub mod event_instructions; // Event-specific instructions
pub mod default_instructions; // Default instructions registry

// Old modules
pub mod client;
pub mod old_actions; // Legacy action system, to be removed
pub mod ollama_client;
pub mod prompt;
pub mod response_handler; // Keep the old client module for reference

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
    call_llm_with_actions, call_llm_with_custom_actions, call_llm_with_protocol,
    call_llm_for_client, ClientLlmResult,
};

// Legacy re-exports (for backward compatibility during migration)
pub use old_actions::{Action as OldAction, CommandInterpretation as OldCommandInterpretation};

// Current ollama client exports
pub use ollama_client::{
    CommandAction, CommandInterpretation, HttpLlmResponse, LlmResponse, OllamaClient,
};
pub use prompt::PromptBuilder;
pub use response_handler::{handle_llm_response, ProcessedResponse};

// Conversation handler
pub use conversation::ConversationHandler;

// Conversation state management
pub use conversation_state::{ConversationState, ConversationMessage, MessageRole, MessageType};

// Message type from ollama_client module
pub use ollama_client::Message;

// Event instructions
pub use event_instructions::{EventInstructions, Example, ServerInstructionConfig, InstructionSource};
pub use default_instructions::{resolve_instructions, DEFAULT_INSTRUCTIONS};
