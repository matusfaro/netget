//! Socket File client tests module

#[cfg(all(test, feature = "socket_file", unix))]
mod e2e_test;
