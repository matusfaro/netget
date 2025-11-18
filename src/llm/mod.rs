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
    CommandAction, CommandInterpretation, HttpLlmResponse, LlmResponse, OllamaClient,
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
pub use config::{LlmBackendType, NetGetConfig, OllamaConfig};
#[cfg(feature = "embedded-llm")]
pub use config::EmbeddedLlmConfig;

// Hybrid manager exports
#[cfg(feature = "embedded-llm")]
pub use hybrid_manager::{ActiveBackend, HybridLLMManager};
