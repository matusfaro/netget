//! Event management module
//!
//! Handles all types of events (network, UI, timeout) and coordinates responses

pub mod types;
pub mod handler;

pub use types::{AppEvent, NetworkEvent, UserCommand};
pub use handler::EventHandler;
