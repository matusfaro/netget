//! RIP (Routing Information Protocol) E2E tests

#[cfg(all(test, feature = "rip"))]
pub mod e2e_test;

#[cfg(all(test, feature = "rip"))]
mod static_default_test;
