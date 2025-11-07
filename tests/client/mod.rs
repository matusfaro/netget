//! Client protocol e2e tests

#[cfg(feature = "tcp")]
pub mod tcp;
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "redis")]
pub mod redis;
#[cfg(feature = "socks5")]
pub mod socks5;
#[cfg(feature = "wireguard")]
pub mod wireguard;
