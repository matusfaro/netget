//! Reverse-shell listener E2E tests.

#[cfg(all(test, feature = "reverse-shell"))]
pub mod test;

#[cfg(all(test, feature = "reverse-shell"))]
pub mod peer_inject_test;
