//! State management module
//!
//! Manages application state including mode, protocol, and connection information

pub mod app_state;
pub mod client;
pub mod easy;
pub mod machine;
pub mod server;
pub mod task;

pub use app_state::AppState;
pub use client::{ClientConnectionState, ClientId, ClientInstance, ClientStatus};
pub use easy::{EasyId, EasyInstance, EasyStatus};
pub use machine::StateMachine;
pub use server::{
    ConnectionState, ProtocolConnectionInfo, ProtocolState, ServerId, ServerInstance, ServerStatus,
};
pub use task::{ScheduledTask, TaskExecutionResult, TaskId, TaskScope, TaskStatus, TaskType};
