//! IMAP E2E tests module

#[cfg(all(test, feature = "imap"))]
pub mod test;
pub mod e2e_client_test;
