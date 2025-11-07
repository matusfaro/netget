//! NTP client E2E tests
//!
//! Feature-gated to only compile when ntp feature is enabled.

#[cfg(all(test, feature = "ntp"))]
mod e2e_test;
