//! Protocol type definitions
//!
//! The application supports multiple base protocol stacks.
//! Protocol behavior is controlled by the LLM based on the chosen stack and instructions.

pub mod base_stack;
pub mod event_type;

pub use base_stack::{BaseStack, ProtocolMetadata, ProtocolState};
pub use event_type::{Event, EventType};
