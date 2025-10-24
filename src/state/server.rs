//! Server instance management

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::WriteHalf;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::network::connection::ConnectionId;
use crate::protocol::BaseStack;

/// Unique identifier for a server instance
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ServerId(u32);

impl ServerId {
    /// Create a new server ID from a u32
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get the raw ID value
    pub fn as_u32(&self) -> u32 {
        self.0
    }

    /// Parse from string (expects format "server-123" or just "123")
    pub fn from_string(s: &str) -> Option<Self> {
        let s = s.trim();
        let id_str = s.strip_prefix("server-").unwrap_or(s);
        id_str.parse::<u32>().ok().map(Self)
    }
}

impl std::fmt::Display for ServerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "server-{}", self.0)
    }
}

/// Status of a server instance
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerStatus {
    /// Server is starting up
    Starting,
    /// Server is running and accepting connections
    Running,
    /// Server has been stopped
    Stopped,
    /// Server encountered an error
    Error(String),
}

impl ServerStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Starting => "Starting",
            Self::Running => "Running",
            Self::Stopped => "Stopped",
            Self::Error(_) => "Error",
        }
    }
}

impl std::fmt::Display for ServerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Starting => write!(f, "Starting"),
            Self::Running => write!(f, "Running"),
            Self::Stopped => write!(f, "Stopped"),
            Self::Error(msg) => write!(f, "Error: {}", msg),
        }
    }
}

/// Connection state for state machine (Idle/Processing/Accumulating)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolState {
    /// Connection is idle, ready to process data
    Idle,
    /// LLM is currently processing a request
    Processing,
    /// LLM requested more data (WAIT_FOR_MORE)
    Accumulating,
}

/// Protocol-specific connection information
#[derive(Debug, Clone)]
pub enum ProtocolConnectionInfo {
    /// TCP connection with write half
    Tcp {
        write_half: Arc<Mutex<WriteHalf<TcpStream>>>,
        state: ProtocolState,
        queued_data: Vec<u8>,
    },
    /// UDP "connection" (recent peers)
    Udp {
        recent_peers: Vec<(SocketAddr, Instant)>,
    },
    /// HTTP connection (recent requests)
    Http {
        recent_requests: Vec<(String, String, Instant)>, // method, path, time
    },
    /// SNMP connection (recent requests)
    Snmp {
        recent_peers: Vec<(SocketAddr, Instant)>,
    },
    /// DNS connection (recent queries)
    Dns {
        recent_queries: Vec<(String, Instant)>, // query, time
    },
    /// DHCP connection (recent requests)
    Dhcp {
        recent_requests: Vec<(String, Instant)>, // client MAC, time
    },
    /// NTP connection (recent clients)
    Ntp {
        recent_clients: Vec<(SocketAddr, Instant)>,
    },
    /// SSH connection (managed by russh library)
    Ssh {
        authenticated: bool,
        username: Option<String>,
        channels: Vec<String>, // Active channel types (shell, sftp)
    },
    /// IRC connection with write half
    Irc {
        write_half: Arc<Mutex<WriteHalf<TcpStream>>>,
        state: ProtocolState,
        queued_data: Vec<u8>,
    },
    /// HTTP Proxy connection (recent requests)
    Proxy {
        recent_requests: Vec<(String, String, Instant)>, // method, URL, time
    },
    /// WebDAV connection (recent operations)
    WebDav {
        recent_operations: Vec<(String, String, Instant)>, // operation, path, time
    },
    /// NFS connection (mounted paths)
    Nfs {
        mounted_paths: Vec<String>,
    },
}

/// Connection status
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionStatus {
    /// Connection is active
    Active,
    /// Connection has been closed
    Closed,
}

/// Connection state within a server
#[derive(Debug, Clone)]
pub struct ConnectionState {
    /// Connection ID
    pub id: ConnectionId,
    /// Remote address
    pub remote_addr: SocketAddr,
    /// Local address
    pub local_addr: SocketAddr,
    /// Bytes sent
    pub bytes_sent: u64,
    /// Bytes received
    pub bytes_received: u64,
    /// Packets sent
    pub packets_sent: u64,
    /// Packets received
    pub packets_received: u64,
    /// Last activity timestamp
    pub last_activity: Instant,
    /// Connection status (Active/Closed)
    pub status: ConnectionStatus,
    /// When status last changed (for cleanup reaper)
    pub status_changed_at: Instant,
    /// Protocol-specific information
    pub protocol_info: ProtocolConnectionInfo,
}

/// A server instance with its own connections, state, and configuration
#[derive(Debug)]
pub struct ServerInstance {
    /// Unique server ID
    pub id: ServerId,
    /// Listening port
    pub port: u16,
    /// Base protocol stack
    pub base_stack: BaseStack,
    /// User instructions for this server
    pub instruction: String,
    /// LLM memory for this server
    pub memory: String,
    /// Server status
    pub status: ServerStatus,
    /// Active connections for this server
    pub connections: HashMap<ConnectionId, ConnectionState>,
    /// Server task handle (for cleanup)
    pub handle: Option<JoinHandle<()>>,
    /// When the server was created
    pub created_at: Instant,
    /// When the server status last changed (for cleanup reaper)
    pub status_changed_at: Instant,
    /// Local listening address
    pub local_addr: Option<SocketAddr>,
    /// Proxy filter configuration (only for proxy servers)
    #[cfg(feature = "proxy")]
    pub proxy_filter_config: Option<crate::network::proxy_filter::ProxyFilterConfig>,
}

impl ServerInstance {
    /// Create a new server instance
    pub fn new(
        id: ServerId,
        port: u16,
        base_stack: BaseStack,
        instruction: String,
    ) -> Self {
        let now = Instant::now();
        Self {
            id,
            port,
            base_stack,
            instruction,
            memory: String::new(),
            status: ServerStatus::Starting,
            connections: HashMap::new(),
            handle: None,
            created_at: now,
            status_changed_at: now,
            local_addr: None,
            #[cfg(feature = "proxy")]
            proxy_filter_config: None,
        }
    }

    /// Get a summary for display
    pub fn summary(&self) -> String {
        format!(
            "#{} {} on port {} ({}) - {} connections",
            self.id.as_u32(),
            self.base_stack,
            self.port,
            self.status.as_str(),
            self.connections.len()
        )
    }

    /// Add a connection to this server
    pub fn add_connection(&mut self, state: ConnectionState) {
        self.connections.insert(state.id, state);
    }

    /// Remove a connection from this server
    pub fn remove_connection(&mut self, id: ConnectionId) -> Option<ConnectionState> {
        self.connections.remove(&id)
    }

    /// Get a connection by ID
    pub fn get_connection(&self, id: ConnectionId) -> Option<&ConnectionState> {
        self.connections.get(&id)
    }

    /// Get a mutable connection by ID
    pub fn get_connection_mut(&mut self, id: ConnectionId) -> Option<&mut ConnectionState> {
        self.connections.get_mut(&id)
    }

    /// Get all connections
    pub fn get_all_connections(&self) -> Vec<&ConnectionState> {
        self.connections.values().collect()
    }

    /// Clean up old connectionless protocol entries (UDP, DNS, etc.)
    pub fn cleanup_old_connections(&mut self, max_age_secs: u64) {
        let now = Instant::now();
        self.connections.retain(|_, state| {
            now.duration_since(state.last_activity).as_secs() < max_age_secs
        });
    }
}
