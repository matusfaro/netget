//! BGP E2E tests module

#[cfg(all(test, feature = "bgp"))]
pub mod test;

#[cfg(all(test, feature = "bgp"))]
pub mod e2e_test;
