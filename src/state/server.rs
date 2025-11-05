//! Server instance management

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Instant;
use tokio::task::JoinHandle;

use crate::server::connection::ConnectionId;

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

/// IMAP session state (RFC 3501)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImapSessionState {
    /// Not authenticated - initial state, only LOGIN/AUTHENTICATE allowed
    NotAuthenticated,
    /// Authenticated - user logged in, can access mailboxes
    Authenticated,
    /// Selected - mailbox selected for operations (FETCH, STORE, etc.)
    Selected,
    /// Logout - client issued LOGOUT, connection closing
    Logout,
}

/// BGP session state (RFC 4271 FSM)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BgpSessionState {
    /// Idle - initial state, waiting for connection
    Idle,
    /// Connect - TCP connection established, waiting to send OPEN
    Connect,
    /// Active - failed to connect, will retry
    Active,
    /// OpenSent - OPEN message sent, waiting for peer's OPEN
    OpenSent,
    /// OpenConfirm - OPEN received and validated, waiting for KEEPALIVE
    OpenConfirm,
    /// Established - full BGP session established, exchanging routes
    Established,
}

/// OSPF neighbor state (RFC 2328 Section 10.1)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OspfNeighborState {
    /// Down - initial state, no Hello received
    Down,
    /// Init - Hello received from neighbor
    Init,
    /// 2-Way - bidirectional communication established
    TwoWay,
    /// ExStart - master/slave negotiation for database exchange
    ExStart,
    /// Exchange - database description packets exchanged
    Exchange,
    /// Loading - link state requests sent
    Loading,
    /// Full - adjacency complete, databases synchronized
    Full,
}

/// Protocol-specific connection information
///
/// Each protocol variant contains protocol-specific state and connection data.
/// Note: This storage is primarily for UI display and metrics.
/// Protocols maintain their own local connection data for I/O operations.
#[derive(Debug, Clone)]
pub enum ProtocolConnectionInfo {
    /// TCP connection state
    Tcp {
        write_half: Option<Arc<tokio::sync::Mutex<tokio::net::tcp::OwnedWriteHalf>>>,
        state: ProtocolState,
        queued_data: Vec<Vec<u8>>,
    },
    /// UDP connection state
    Udp {
        socket: Option<Arc<tokio::net::UdpSocket>>,
    },
    /// HTTP connection state
    Http {
        recent_requests: Vec<String>,
    },
    /// DNS connection state
    Dns {},
    /// DHCP connection state
    Dhcp {},
    /// NTP connection state
    Ntp {},
    /// SNMP connection state
    Snmp {},
    /// SSH connection state
    Ssh {
        write_half: Option<Arc<tokio::sync::Mutex<tokio::net::tcp::OwnedWriteHalf>>>,
        state: ProtocolState,
        queued_data: Vec<Vec<u8>>,
        authenticated: bool,
        username: Option<String>,
    },
    /// IMAP connection state
    Imap {
        write_half: Arc<tokio::sync::Mutex<tokio::net::tcp::OwnedWriteHalf>>,
        state: ProtocolState,
        queued_data: Vec<Vec<u8>>,
        session_state: ImapSessionState,
        username: Option<String>,
        selected_mailbox: Option<String>,
    },
    /// IRC connection state
    Irc {
        write_half: Arc<tokio::sync::Mutex<tokio::net::tcp::OwnedWriteHalf>>,
        state: ProtocolState,
        queued_data: Vec<Vec<u8>>,
        nickname: Option<String>,
        channels: Vec<String>,
    },
    /// Telnet connection state
    Telnet {
        write_half: Arc<tokio::sync::Mutex<tokio::net::tcp::OwnedWriteHalf>>,
        state: ProtocolState,
        queued_data: Vec<Vec<u8>>,
    },
    /// Git connection state
    Git {
        recent_repos: Vec<String>,
    },
    /// Mercurial connection state
    Mercurial {
        recent_repos: Vec<String>,
    },
    /// MQTT connection state
    Mqtt {
        write_half: Arc<tokio::sync::Mutex<tokio::net::tcp::OwnedWriteHalf>>,
        state: ProtocolState,
        queued_data: Vec<Vec<u8>>,
        client_id: Option<String>,
        subscriptions: Vec<String>,
    },
    /// MySQL connection state
    Mysql {},
    /// PostgreSQL connection state
    Postgresql {},
    /// Redis connection state
    Redis {},
    /// Cassandra connection state
    Cassandra {},
    /// Dynamo connection state
    Dynamo {},
    /// Elasticsearch connection state
    Elasticsearch {},
    /// IPP connection state
    Ipp {},
    /// WebDAV connection state
    WebDav {},
    /// SOCKS5 connection state
    Socks {},
    /// STUN connection state
    Stun {},
    /// TURN connection state
    Turn {},
    /// gRPC connection state
    Grpc {},
    /// MCP connection state
    Mcp {},
    /// JsonRpc connection state
    JsonRpc {},
    /// XmlRpc connection state
    XmlRpc {},
    /// VNC connection state
    Vnc {},
    /// Kafka connection state
    Kafka {},
    /// S3 connection state
    S {},
    /// SQS connection state
    Sqs {},
    /// SMTP connection state
    Smtp {},
    /// OpenAi connection state
    OpenAi {},
    /// DoT connection state
    Dot {},
    /// DoH connection state
    Doh {},
    /// IGMP connection state
    Igmp {},
    /// Bootp connection state
    Bootp {},
    /// Wireguard connection state
    Wireguard {},
    /// OpenVPN connection state
    Openvpn {},
    /// BGP connection state
    Bgp {
        session_state: BgpSessionState,
        peer_as: Option<u32>,
        peer_id: Option<std::net::Ipv4Addr>,
    },
    /// OSPF connection state
    Ospf {
        neighbor_state: OspfNeighborState,
        neighbor_id: Option<std::net::Ipv4Addr>,
        area_id: Option<u32>,
    },
    /// ISIS connection state
    Isis {},
    /// RIP connection state
    Rip {},
    /// XMPP connection state
    Xmpp {
        write_half: Arc<tokio::sync::Mutex<tokio::net::tcp::OwnedWriteHalf>>,
        state: ProtocolState,
        queued_data: Vec<Vec<u8>>,
        jid: Option<String>,
        authenticated: bool,
    },
    /// SIP connection state
    Sip {},
    /// LDAP connection state
    Ldap {},
    /// SMB connection state
    Smb {},
    /// NFS connection state
    Nfs {},
    /// Proxy connection state
    Proxy {},
    /// Syslog connection state
    Syslog {},
    /// NNTP connection state
    Nntp {},
    /// Whois connection state
    Whois {},
    /// Bitcoin connection state
    Bitcoin {},
    /// TorrentTracker connection state
    TorrentTracker {},
    /// TorrentDht connection state
    TorrentDht {},
    /// TorrentPeer connection state
    TorrentPeer {},
    /// DC connection state
    Dc {},
    /// Maven connection state
    Maven {},
    /// Npm connection state
    Npm {},
    /// Pypi connection state
    Pypi {},
    /// OpenApi connection state
    OpenApi {},
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
    /// Protocol name (e.g., "TCP", "HTTP", "SSH")
    pub protocol_name: String,
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
    /// Protocol-specific startup parameters
    pub startup_params: Option<serde_json::Value>,
    /// Script configuration for handling protocol events
    pub script_config: Option<crate::scripting::ScriptConfig>,
    /// Protocol-specific server data (flexible storage)
    ///
    /// This replaces protocol-specific feature-gated fields.
    /// Each protocol can store any data structure here by serializing to JSON.
    /// Use get_protocol_data() and set_protocol_data() helper methods.
    pub protocol_data: serde_json::Value,
    /// Log file paths (output_name -> log_file_path)
    pub log_files: HashMap<String, PathBuf>,
}

impl ServerInstance {
    /// Create a new server instance
    pub fn new(id: ServerId, port: u16, protocol_name: String, instruction: String) -> Self {
        let now = Instant::now();
        Self {
            id,
            port,
            protocol_name,
            instruction,
            memory: String::new(),
            status: ServerStatus::Starting,
            connections: HashMap::new(),
            handle: None,
            created_at: now,
            status_changed_at: now,
            local_addr: None,
            startup_params: None,
            script_config: None,
            protocol_data: serde_json::Value::Object(serde_json::Map::new()),
            log_files: HashMap::new(),
        }
    }

    /// Get protocol-specific data
    pub fn get_protocol_data<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.protocol_data.clone())
    }

    /// Set protocol-specific data
    pub fn set_protocol_data<T: serde::Serialize>(&mut self, data: T) -> Result<(), serde_json::Error> {
        self.protocol_data = serde_json::to_value(data)?;
        Ok(())
    }

    /// Get a field from protocol data
    pub fn get_protocol_field(&self, key: &str) -> Option<&serde_json::Value> {
        self.protocol_data.get(key)
    }

    /// Set a field in protocol data
    pub fn set_protocol_field(&mut self, key: String, value: serde_json::Value) {
        if let Some(obj) = self.protocol_data.as_object_mut() {
            obj.insert(key, value);
        } else {
            // Initialize as object if not already
            let mut map = serde_json::Map::new();
            map.insert(key, value);
            self.protocol_data = serde_json::Value::Object(map);
        }
    }

    /// Get or create a log file path for the given output name
    /// Returns the path to the log file with format: netget_<output_name>_<timestamp>.log
    /// The timestamp is based on when the server was created
    pub fn get_or_create_log_path(&mut self, output_name: &str) -> PathBuf {
        if let Some(path) = self.log_files.get(output_name) {
            return path.clone();
        }

        // Calculate the absolute time when the server was created
        // by subtracting the elapsed time from now
        let now = std::time::SystemTime::now();
        let elapsed = self.created_at.elapsed();
        let created_system_time = now - elapsed;

        // Convert to DateTime for formatting
        let timestamp: chrono::DateTime<chrono::Local> = created_system_time.into();
        let timestamp_str = timestamp.format("%Y_%m_%d_%H_%M_%S").to_string();

        let log_filename = format!("netget_{}_{}.log", output_name, timestamp_str);
        let log_path = PathBuf::from(log_filename);

        self.log_files.insert(output_name.to_string(), log_path.clone());
        log_path
    }

    /// Get a summary for display
    pub fn summary(&self) -> String {
        format!(
            "#{} {} on port {} ({}) - {} connections",
            self.id.as_u32(),
            self.protocol_name,
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
        self.connections
            .retain(|_, state| now.duration_since(state.last_activity).as_secs() < max_age_secs);
    }
}
