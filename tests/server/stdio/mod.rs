//! Standard I/O (stdin/stdout/stderr) server tests
//!
//! Platform: Unix/Linux/macOS only
#![cfg(all(test, feature = "stdio", unix))]

pub mod e2e_test;
