//! E2E test module
//!
//! This module contains end-to-end tests using the test framework
//! with protocol validators and NetGet wrapper.

pub mod netget_wrapper;

#[cfg(all(test, feature = "http"))]
pub mod http_test;

#[cfg(all(test, feature = "tcp"))]
pub mod tcp_test;
