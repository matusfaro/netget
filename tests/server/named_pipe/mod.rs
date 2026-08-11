//! Named pipe (POSIX FIFO) server tests
//!
//! Platform: Unix/Linux/macOS only
#![cfg(all(test, feature = "named_pipe", unix))]

pub mod e2e_test;
