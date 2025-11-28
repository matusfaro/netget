//! Protocol example E2E tests
//!
//! These tests verify that protocol examples work correctly by:
//! - Starting servers with example configurations
//! - Triggering events using appropriate methods
//! - Verifying that example responses execute correctly

pub mod helpers;

#[path = "examples/mod.rs"]
mod examples;

pub use examples::*;
