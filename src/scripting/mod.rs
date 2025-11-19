//! Script-based response handling system
//!
//! This module provides support for Python and JavaScript scripts to handle
//! protocol responses deterministically, with fallback to LLM for complex cases.

pub mod environment;
pub mod event_handler;
pub mod executor;
pub mod highlight;
pub mod manager;
pub mod types;

// Re-export commonly used types
pub use environment::ScriptingEnvironment;
pub use event_handler::{EventHandler, EventHandlerConfig, EventHandlerType, EventPattern};
pub use executor::{execute_script, SCRIPT_TIMEOUT_SECS};
pub use manager::ScriptManager;
pub use types::{
    ConnectionContext, ScriptConfig, ScriptInput, ScriptLanguage, ScriptResponse, ScriptSource,
    ServerContext,
};
