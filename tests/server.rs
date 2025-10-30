//! Server protocol E2E tests
//!
//! These tests spawn the actual NetGet binary and interact with it
//! as a black-box system, simulating real protocol clients.

#[cfg(feature = "e2e-tests")]
#[path = "server/mod.rs"]
mod server;

#[cfg(feature = "e2e-tests")]
pub use server::*;
