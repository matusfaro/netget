//! Terminal user interface module
//!
//! Provides a full-screen TUI with multiple panels for user interaction,
//! LLM responses, connection information, and status updates.

pub mod app;
pub mod events;
pub mod layout;

pub use app::{App, Focus};
pub use events::UiEvent;
