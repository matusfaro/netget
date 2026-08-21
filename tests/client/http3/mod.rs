//! HTTP/3 client tests module

#[cfg(all(test, feature = "http3"))]
mod command_channel_test;
#[cfg(all(test, feature = "http3"))]
mod e2e_test;
