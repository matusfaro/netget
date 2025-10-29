//! State management module
//!
//! Manages application state including mode, protocol, and connection information

pub mod app_state;
pub mod machine;
pub mod server;

pub use app_state::AppState;
pub use machine::StateMachine;
pub use server::{
    ConnectionState, ProtocolConnectionInfo, ProtocolState, ServerId, ServerInstance, ServerStatus,
};
