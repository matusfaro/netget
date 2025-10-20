//! State management module
//!
//! Manages application state including mode, protocol, and connection information

pub mod app_state;
pub mod machine;

pub use app_state::AppState;
pub use machine::StateMachine;
