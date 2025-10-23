//! End-to-end tests for NetGet
//!
//! These tests spawn the actual NetGet binary and interact with it
//! as a black-box system, simulating real user interactions.

#[cfg(feature = "e2e-tests")]
pub mod helpers;