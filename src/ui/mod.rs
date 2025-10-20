//! Terminal user interface module
//!
//! Provides a full-screen TUI with multiple panels for user interaction,
//! LLM responses, connection information, and status updates.

pub mod app;
pub mod layout;
pub mod events;

pub use app::App;
pub use events::UiEvent;
