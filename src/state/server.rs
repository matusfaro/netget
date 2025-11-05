//! Server instance management

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::WriteHalf;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
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
    /// Maven repository connection (recent artifact requests)
    Maven {
        recent_artifacts: Vec<String>,
    },
    /// SNMP connection (recent requests)
    Snmp {
        recent_peers: Vec<(SocketAddr, Instant)>,
    },
    /// IGMP connection (multicast group management)
    Igmp {
        joined_groups: Vec<std::net::Ipv4Addr>,
    },
    /// Syslog connection (recent messages)
    Syslog {
        recent_peers: Vec<(SocketAddr, Instant)>,
    },
    /// DNS connection (recent queries)
    Dns {
        recent_queries: Vec<(String, Instant)>, // query, time
    },
    /// DNS-over-TLS connection (persistent TCP+TLS)
    Dot {
        peer_addr: SocketAddr,
        recent_queries: Vec<(String, chrono::DateTime<chrono::Utc>)>, // query, time
    },
    /// DNS-over-HTTPS connection (HTTP/2 session)
    Doh {
        peer_addr: SocketAddr,
        recent_queries: Vec<(String, chrono::DateTime<chrono::Utc>)>, // query, time
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
    /// XMPP connection with write half
    Xmpp {
        write_half: Arc<Mutex<WriteHalf<TcpStream>>>,
        state: ProtocolState,
        queued_data: Vec<u8>,
        jid: Option<String>,
        authenticated: bool,
    },
    /// Telnet connection with write half
    Telnet {
        write_half: Arc<Mutex<WriteHalf<TcpStream>>>,
        state: ProtocolState,
        queued_data: Vec<u8>,
    },
    /// SMTP connection with write half
    Smtp {
        write_half: Arc<Mutex<WriteHalf<TcpStream>>>,
        state: ProtocolState,
        queued_data: Vec<u8>,
    },
    /// mDNS service (no traditional connections, just advertisements)
    Mdns {
        advertised_services: Vec<(String, u16, Instant)>, // service_name, port, time
    },
    /// MySQL connection (managed by opensrv-mysql)
    Mysql,
    /// IPP connection (recent print jobs)
    Ipp {
        recent_jobs: Vec<(String, Instant)>, // job ID, time
    },
    /// PostgreSQL connection (managed by pgwire)
    Postgresql,
    /// Redis connection (managed by RESP protocol)
    Redis,
    /// Cassandra connection (CQL native protocol)
    Cassandra {
        ready: bool,
        protocol_version: u8,
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
    Nfs { mounted_paths: Vec<String> },
    /// SMB connection with session and file state
    Smb {
        authenticated: bool,
        username: Option<String>,
        session_id: Option<u64>,
        open_files: Vec<String>, // Paths of currently open files
    },
    /// STUN connection (transaction)
    Stun {
        transaction_id: Option<String>,
    },
    /// TURN connection (allocations and relay)
    Turn {
        allocation_ids: Vec<String>,
        relay_addresses: Vec<String>,
    },
    /// SIP connection (VoIP signaling)
    Sip {
        dialog_id: Option<String>,    // Call-ID + From tag + To tag
        from: Option<String>,          // Caller SIP URI
        to: Option<String>,            // Callee SIP URI
        state: String,                 // idle/early/confirmed/terminated
        call_id: Option<String>,       // Call-ID header value
    },
    /// LDAP connection with write half
    Ldap {
        write_half: Arc<Mutex<WriteHalf<TcpStream>>>,
        state: ProtocolState,
        queued_data: Vec<u8>,
        authenticated: bool,
        bind_dn: Option<String>,
    },
    /// IMAP connection (write_half stored in ImapSession)
    Imap {
        state: ProtocolState,
        queued_data: Vec<u8>,
        session_state: ImapSessionState,
        authenticated_user: Option<String>,
        selected_mailbox: Option<String>,
        mailbox_read_only: bool,
    },
    /// MQTT connection (client session)
    Mqtt {
        client_id: String,
        subscriptions: Vec<String>, // Subscribed topic filters
    },
    /// SOCKS5 proxy connection
    Socks5 {
        target_addr: Option<String>,       // Target address being proxied to
        username: Option<String>,          // Authenticated username (if any)
        mitm_enabled: bool,               // Whether MITM inspection is active
        state: ProtocolState,
        queued_data: Vec<u8>,
    },
    /// Elasticsearch connection (recent queries)
    Elasticsearch {
        recent_requests: Vec<(String, String, Instant)>, // method, path, time
    },
    /// DynamoDB connection (recent operations)
    Dynamo {
        recent_operations: Vec<(String, String, Instant)>, // operation, table, time
    },
    /// S3 connection (recent operations)
    S3 {
        recent_operations: Vec<(String, Option<String>, Option<String>, Instant)>, // operation, bucket, key, time
    },
    /// SQS connection (recent operations)
    Sqs {
        recent_operations: Vec<(String, String, Instant)>, // operation, queue_url, time
    },
    /// NPM registry connection (recent requests)
    Npm {
        recent_requests: Vec<String>, // Recent package requests
    },
    /// OpenAI API connection (recent requests)
    OpenAi {
        recent_requests: Vec<String>, // Recent endpoints accessed
    },
    /// JSON-RPC connection (recent method calls)
    JsonRpc {
        recent_methods: Vec<String>, // Recent RPC methods called
    },
    /// WireGuard VPN connection
    Wireguard {
        public_key: String,                      // Peer's public key (base64)
        endpoint: Option<String>,                // Peer's endpoint address (may be unknown initially)
        allowed_ips: Vec<String>,                // Allowed IPs for this peer
        last_handshake: Option<std::time::SystemTime>, // Last successful handshake time
    },
    /// OpenVPN connection
    Openvpn {
        endpoint: SocketAddr,              // Client endpoint address
        session_id: Option<String>,        // Session ID from handshake (hex)
        protocol_version: u8,              // Protocol version (1 or 2)
        last_packet: Option<Instant>,      // Last packet received time
        tx_bytes: u64,                     // Bytes transmitted
        rx_bytes: u64,                     // Bytes received
    },
    /// IPSec/IKEv2 connection
    Ipsec {
        endpoint: SocketAddr,              // Peer endpoint address
        initiator_spi: String,             // Initiator SPI (hex)
        responder_spi: String,             // Responder SPI (hex)
        ike_version: String,               // IKEv1 or IKEv2
        last_packet: Option<Instant>,      // Last packet received time
        tx_bytes: u64,                     // Bytes transmitted
        rx_bytes: u64,                     // Bytes received
    },
    /// BGP connection with write half and FSM state
    Bgp {
        write_half: Arc<Mutex<WriteHalf<TcpStream>>>,
        state: ProtocolState,
        queued_data: Vec<u8>,
        session_state: BgpSessionState,
        peer_as: Option<u32>,              // Peer AS number
        hold_time: u16,                    // Negotiated hold time (seconds)
        keepalive_time: u16,               // Keepalive interval (seconds)
        announced_prefixes: Vec<String>,   // Announced route prefixes
    },
    /// IS-IS routing protocol connection (Layer 2 neighbor adjacency)
    Isis {
        adjacency_state: String,           // init, up, down
        neighbor_system_id: Option<String>, // e.g., "0000.0000.0002"
        level: String,                     // level-1, level-2, level-1+2
    },
    /// RIP connection (recent peers)
    Rip {
        recent_peers: Vec<(SocketAddr, Instant)>,
    },
    /// gRPC connection (HTTP/2)
    Grpc {
        service_name: String,              // Service being called
        method_name: String,               // Method being called
        metadata: std::collections::HashMap<String, String>,  // gRPC metadata (headers)
    },
    /// etcd connection (gRPC-based distributed KV store)
    Etcd {
        cluster_name: String,              // Cluster name
        last_operation: String,            // Last RPC operation (Range, Put, etc.)
        operations_count: u64,             // Total RPC calls made
    },
    /// XML-RPC connection (HTTP-based RPC)
    XmlRpc {
        recent_methods: Vec<(String, Instant)>,  // method_name, time
    },
    /// MCP (Model Context Protocol) connection
    Mcp {
        session_id: String,                              // Session ID
        initialized: bool,                               // Whether session completed initialization
        capabilities: serde_json::Value,                 // Server capabilities
        subscriptions: std::collections::HashSet<String>, // Resource URIs subscribed to
        tools: std::collections::HashMap<String, String>, // Tool name -> description
        resources: std::collections::HashMap<String, String>, // Resource URI -> name
        prompts: std::collections::HashMap<String, String>, // Prompt name -> description
    },
    /// Tor Directory connection (HTTP requests for consensus/descriptors)
    TorDirectory {
        recent_requests: Vec<(String, Instant)>, // path, time
    },
    /// Tor Relay connection (OR protocol with TLS and cells)
    TorRelay {
        circuits: Vec<String>,         // Active circuit IDs (hex format)
        relay_type: Option<String>,    // Guard, Middle, or Exit (if configured)
        last_cell: Option<Instant>,    // Last cell received time
    },
    /// VNC connection (Remote Frame Buffer protocol)
    Vnc {
        write_half: Arc<Mutex<WriteHalf<TcpStream>>>,
        state: ProtocolState,
        queued_data: Vec<u8>,
        authenticated: bool,
        username: Option<String>,
        framebuffer_width: u16,
        framebuffer_height: u16,
        pixel_format: VncPixelFormat,
    },
    /// OpenAPI connection (HTTP requests validated against OpenAPI spec)
    OpenApi {
        operation_id: Option<String>,  // Matched OpenAPI operation ID
        method: Option<String>,        // HTTP method (GET, POST, etc.)
        path: Option<String>,          // Request path
        validated: bool,               // Whether request was successfully validated
    },
    /// Git Smart HTTP connection
    Git {
        recent_repos: Vec<String>,  // Recently accessed repositories
    },
    /// Kafka connection (recent requests)
    Kafka {
        recent_requests: Vec<(String, Instant)>, // API type, time
    },
    /// BitTorrent Tracker connection
    TorrentTracker {
        recent_requests: Vec<(String, Instant)>, // request type (announce/scrape), time
    },
    /// BitTorrent DHT connection
    TorrentDht {
        recent_queries: Vec<(String, Instant)>, // query type (ping/find_node/get_peers/announce_peer), time
    },
    /// BitTorrent Peer Wire Protocol connection
    TorrentPeer {
        write_half: Arc<Mutex<WriteHalf<TcpStream>>>,
        state: ProtocolState,
        queued_data: Vec<u8>,
        handshake_complete: bool,
        peer_id: Option<String>,
        info_hash: Option<String>,
    },
}

/// VNC pixel format
#[derive(Debug, Clone)]
pub struct VncPixelFormat {
    pub bits_per_pixel: u8,
    pub depth: u8,
    pub big_endian: bool,
    pub true_color: bool,
    pub red_max: u16,
    pub green_max: u16,
    pub blue_max: u16,
    pub red_shift: u8,
    pub green_shift: u8,
    pub blue_shift: u8,
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
    /// Proxy filter configuration (only for proxy servers)
    #[cfg(feature = "proxy")]
    pub proxy_filter_config: Option<crate::server::proxy::filter::ProxyFilterConfig>,
    /// SOCKS5 filter configuration (feature-gated)
    #[cfg(feature = "socks5")]
    pub socks5_filter_config: Option<crate::server::socks5::filter::Socks5FilterConfig>,
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
            #[cfg(feature = "proxy")]
            proxy_filter_config: None,
            #[cfg(feature = "socks5")]
            socks5_filter_config: None,
            log_files: HashMap::new(),
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
