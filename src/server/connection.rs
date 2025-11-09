//! Connection management

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::net::TcpStream;

/// Unique identifier for a connection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionId(u32);

impl ConnectionId {
    /// Create a new connection ID from a u32 value
    /// NOTE: Callers should use AppState::get_next_unified_id() to ensure uniqueness
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get the raw ID value
    pub fn as_u32(&self) -> u32 {
        self.0
    }

    /// Get the raw ID value as u64 (for backwards compatibility)
    #[deprecated(note = "Use as_u32() instead - IDs are now u32")]
    pub fn as_u64(&self) -> u64 {
        self.0 as u64
    }

    /// Parse from string (expects format "conn-123" or just "123")
    pub fn from_string(s: &str) -> Option<Self> {
        let s = s.trim();
        let id_str = s.strip_prefix("conn-").unwrap_or(s);
        id_str.parse::<u32>().ok().map(Self)
    }
}

impl std::fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "conn-{}", self.0)
    }
}

/// Represents an active network connection
pub struct Connection {
    /// Unique identifier for this connection
    pub id: ConnectionId,
    /// Remote address
    pub remote_addr: SocketAddr,
    /// Local address
    pub local_addr: SocketAddr,
    /// TCP stream
    pub stream: TcpStream,
    /// Number of bytes sent
    pub bytes_sent: u64,
    /// Number of bytes received
    pub bytes_received: u64,
}

impl Connection {
    /// Create a new connection with a specific ID
    /// NOTE: Callers should use AppState::get_next_unified_id() to get the ID
    pub fn new_with_id(id: ConnectionId, stream: TcpStream, remote_addr: SocketAddr, local_addr: SocketAddr) -> Self {
        Self {
            id,
            remote_addr,
            local_addr,
            stream,
            bytes_sent: 0,
            bytes_received: 0,
        }
    }

    /// Record bytes sent
    pub fn add_bytes_sent(&mut self, count: u64) {
        self.bytes_sent += count;
    }

    /// Record bytes received
    pub fn add_bytes_received(&mut self, count: u64) {
        self.bytes_received += count;
    }
}
