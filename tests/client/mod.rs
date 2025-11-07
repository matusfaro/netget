//! Client protocol e2e tests

#[cfg(feature = "tcp")]
pub mod tcp;
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "redis")]
pub mod redis;
#[cfg(feature = "wireguard")]
pub mod wireguard;
#[cfg(feature = "oauth2")]
pub mod oauth2;
