//! S3 client tests module
#[cfg(all(test, feature = "s3"))]
pub mod e2e_test;

#[cfg(all(test, feature = "s3"))]
mod command_channel_test;
