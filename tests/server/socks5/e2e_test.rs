//! E2E tests for SOCKS5 proxy server
//!
//! These tests spawn the NetGet binary and test SOCKS5 protocol operations
//! using a custom SOCKS5 client implementation.

#![cfg(all(test, feature = "socks5", feature = "socks5"))]

// Include the SOCKS5 tests
include!("test.rs");
