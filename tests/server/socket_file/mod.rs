//! Unix domain socket file tests
//!
//! Platform: Unix/Linux only
#![cfg(all(test, feature = "socket_file", unix))]

pub mod test;
