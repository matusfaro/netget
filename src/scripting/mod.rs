//! Script-based response handling system
//!
//! This module provides support for Python and JavaScript scripts to handle
//! protocol responses deterministically, with fallback to LLM for complex cases.

pub mod types;
pub mod environment;
pub mod executor;
pub mod manager;
pub mod highlight;
pub mod event_handler;

// Re-export commonly used types
pub use types::{ScriptConfig, ScriptLanguage, ScriptSource, ScriptInput, ScriptResponse};
pub use environment::ScriptingEnvironment;
pub use executor::{execute_script, SCRIPT_TIMEOUT_SECS};
pub use manager::ScriptManager;
pub use event_handler::{EventHandler, EventHandlerConfig, EventHandlerType, EventPattern};
