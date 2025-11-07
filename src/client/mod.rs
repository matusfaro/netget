//! Client protocol implementations
//!
//! This module contains all client protocol implementations.
//! Each protocol provides LLM-controlled client behavior for connecting
//! to remote servers and exchanging data.

// Phase 3: TCP client
#[cfg(feature = "tcp")]
pub mod tcp;
#[cfg(feature = "tcp")]
pub use tcp::actions::TcpClientProtocol;

// Phase 4: HTTP client
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "http")]
pub use http::actions::HttpClientProtocol;

// Phase 5: Redis client
#[cfg(feature = "redis")]
pub mod redis;
#[cfg(feature = "redis")]
pub use redis::actions::RedisClientProtocol;

// HTTP proxy client
#[cfg(feature = "http_proxy")]
pub mod http_proxy;
#[cfg(feature = "http_proxy")]
pub use http_proxy::actions::HttpProxyClientProtocol;
