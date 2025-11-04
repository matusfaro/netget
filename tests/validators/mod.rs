//! Protocol validators for E2E testing
//!
//! This module provides validators that wrap protocol clients with
//! assertion helpers for clean, readable tests.

#[cfg(all(test, feature = "http"))]
pub mod http_validator;

#[cfg(all(test, feature = "tcp"))]
pub mod tcp_validator;

#[cfg(all(test, feature = "ssh"))]
pub mod ssh_validator;

// Re-export for convenience
#[cfg(all(test, feature = "http"))]
pub use http_validator::HttpValidator;

#[cfg(all(test, feature = "tcp"))]
pub use tcp_validator::TcpValidator;

#[cfg(all(test, feature = "ssh"))]
pub use ssh_validator::SshValidator;