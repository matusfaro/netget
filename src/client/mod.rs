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

// BitTorrent Tracker client
#[cfg(feature = "torrent-tracker")]
pub mod torrent_tracker;
#[cfg(feature = "torrent-tracker")]
pub use torrent_tracker::TorrentTrackerClientProtocol;

// BitTorrent DHT client
#[cfg(feature = "torrent-dht")]
pub mod torrent_dht;
#[cfg(feature = "torrent-dht")]
pub use torrent_dht::TorrentDhtClientProtocol;

// BitTorrent Peer Wire client
#[cfg(feature = "torrent-peer")]
pub mod torrent_peer;
#[cfg(feature = "torrent-peer")]
pub use torrent_peer::TorrentPeerClientProtocol;
