//! Event management module
//!
//! Handles all types of events (network, UI, timeout) and coordinates responses

pub mod handler;
pub mod types;

pub use handler::EventHandler;
pub use types::{AppEvent, HttpResponse, UserCommand};
