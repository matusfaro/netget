//! Connection management

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::net::TcpStream;

/// Unique identifier for a connection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionId(u64);

impl ConnectionId {
    /// Create a new connection ID
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        Self(COUNTER.fetch_add(1, Ordering::SeqCst))
    }

    /// Get the raw ID value
    pub fn as_u64(&self) -> u64 {
        self.0
    }

    /// Parse from string (expects format "conn-123" or just "123")
    pub fn from_string(s: &str) -> Option<Self> {
        let s = s.trim();
        let id_str = s.strip_prefix("conn-").unwrap_or(s);
        id_str.parse::<u64>().ok().map(Self)
    }
}

impl Default for ConnectionId {
    fn default() -> Self {
        Self::new()
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
    /// Create a new connection
    pub fn new(stream: TcpStream, remote_addr: SocketAddr, local_addr: SocketAddr) -> Self {
        Self {
            id: ConnectionId::new(),
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
