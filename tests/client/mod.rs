//! Client protocol e2e tests

#[cfg(feature = "tcp")]
pub mod tcp;
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "http2")]
pub mod http2;
#[cfg(feature = "redis")]
pub mod redis;
#[cfg(feature = "grpc")]
pub mod grpc;
