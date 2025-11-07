//! Client protocol E2E tests
//!
//! These tests spawn the actual NetGet binary and interact with it
//! as a black-box system, simulating real protocol servers.

pub mod helpers;

#[path = "client/mod.rs"]
mod client;

pub use client::*;
