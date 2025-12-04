//! Protocol type definitions
//!
//! The application supports multiple protocol implementations.
//! Protocol behavior is controlled by the LLM based on the chosen protocol and instructions.

pub mod binding_defaults;
pub mod client_registry;
pub mod connect_context;
pub mod dependencies;
pub mod easy_registry;
pub mod event_logger;
pub mod event_type;
pub mod log_template;
pub mod metadata;
pub mod server_registry;
pub mod spawn_context;

pub use binding_defaults::BindingDefaults;
pub use client_registry::CLIENT_REGISTRY;
pub use connect_context::ConnectContext;
pub use dependencies::ProtocolDependency;
pub use easy_registry::EASY_REGISTRY;
pub use event_logger::{log_action_result, EventLogContext};
pub use event_type::{Event, EventType};
pub use log_template::{LogLevel, LogTemplate};
pub use metadata::{DevelopmentState, ProtocolMetadata, ProtocolMetadataV2};
pub use server_registry::registry;
pub use spawn_context::{SpawnContext, StartupParams};
