//! Protocol type definitions
//!
//! The application supports multiple protocol implementations.
//! Protocol behavior is controlled by the LLM based on the chosen protocol and instructions.

pub mod client_registry;
pub mod connect_context;
pub mod event_type;
pub mod metadata;
pub mod registry;
pub mod spawn_context;

pub use client_registry::CLIENT_REGISTRY;
pub use connect_context::ConnectContext;
pub use metadata::{ProtocolMetadata, ProtocolMetadataV2, DevelopmentState};
pub use event_type::{Event, EventType};
pub use registry::registry;
pub use spawn_context::{SpawnContext, StartupParams};
