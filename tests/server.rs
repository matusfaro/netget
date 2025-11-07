//! Server protocol E2E tests
//!
//! These tests spawn the actual NetGet binary and interact with it
//! as a black-box system, simulating real protocol clients.

pub mod helpers;

#[path = "server/mod.rs"]
mod server;

pub use server::*;
