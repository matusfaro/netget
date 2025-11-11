//! IMAP E2E tests module

pub mod e2e_client_test;
#[cfg(all(test, feature = "imap"))]
pub mod test;
