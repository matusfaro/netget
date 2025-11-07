//! Client protocol e2e tests

#[cfg(feature = "tcp")]
pub mod tcp;
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "mysql")]
pub mod mysql;
#[cfg(feature = "redis")]
pub mod redis;
#[cfg(feature = "mdns")]
pub mod mdns;
#[cfg(feature = "wireguard")]
pub mod wireguard;
