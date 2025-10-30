//! E2E tests for SOCKS5 proxy server
//!
//! These tests spawn the NetGet binary and test SOCKS5 protocol operations
//! using a custom SOCKS5 client implementation.

#![cfg(all(test, feature = "e2e-tests", feature = "socks5"))]

mod e2e {
    include!("e2e/helpers.rs");
}

// Include the SOCKS5 tests
include!("e2e/server/socks5/test.rs");
