//! Base protocol stack definitions
//!
//! Defines the underlying network stack used by the application.
//! Each stack determines how network data is processed and what the LLM controls.

/// Protocol implementation state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolState {
    /// Fully implemented, production-ready
    Implemented,
    /// Stable, feature-complete, recommended for use
    Beta,
    /// Experimental, may have limitations or bugs
    Alpha,
    /// Implementation in-progress, abandoned, not functional (will not show in LLM prompts)
    Disabled,
}

impl ProtocolState {
    /// Get the string representation for display
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Implemented => "Implemented",
            Self::Beta => "Beta",
            Self::Alpha => "Alpha",
            Self::Disabled => "Disabled",
        }
    }
}

/// Protocol metadata including state and notes
#[derive(Debug, Clone)]
pub struct ProtocolMetadata {
    /// Current implementation state
    pub state: ProtocolState,
    /// Optional notes explaining the state or limitations
    pub notes: Option<&'static str>,
}

impl ProtocolMetadata {
    /// Create new metadata with just a state
    pub const fn new(state: ProtocolState) -> Self {
        Self { state, notes: None }
    }

    /// Create new metadata with state and notes
    pub const fn with_notes(state: ProtocolState, notes: &'static str) -> Self {
        Self {
            state,
            notes: Some(notes),
        }
    }

    /// Check if this protocol should be shown to the LLM
    pub fn is_available_to_llm(&self) -> bool {
        self.state != ProtocolState::Disabled
    }
}

/// Base protocol stack types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BaseStack {
    /// Raw TCP/IP stack - LLM controls raw TCP data
    /// The LLM constructs entire protocol messages (FTP, HTTP, etc.) from scratch
    Tcp,

    /// HTTP stack - Uses Rust HTTP library
    /// The LLM only controls HTTP responses (status, headers, body) based on requests
    Http,

    /// Data Link stack - LLM controls layer 2 (Ethernet) frames
    /// The LLM can capture and inject packets at the data link layer
    /// Supports operations like ARP, custom Ethernet frames, etc.
    DataLink,

    /// Raw UDP/IP stack - LLM controls raw UDP data
    /// Similar to TcpRaw but for UDP-based protocols
    Udp,

    /// DNS stack - DNS server using hickory-dns
    /// The LLM generates DNS responses to queries (port 53)
    Dns,

    /// DHCP stack - DHCP server using dhcproto
    /// The LLM handles DHCP requests and assignments (ports 67/68)
    Dhcp,

    /// NTP stack - NTP server using ntpd-rs
    /// The LLM handles time synchronization requests (port 123)
    Ntp,

    /// SNMP stack - SNMP agent using rasn-snmp
    /// The LLM handles SNMP get/set requests and MIB responses (port 161)
    Snmp,

    /// SSH stack - SSH server using russh
    /// The LLM handles SSH authentication and shell sessions (port 22)
    Ssh,

    /// IRC stack - IRC server using irc crate
    /// The LLM handles IRC chat protocol and channel management (port 6667)
    Irc,

    /// Telnet stack - Telnet server using nectar
    /// The LLM handles telnet sessions and terminal interactions (port 23)
    Telnet,

    /// SMTP stack - SMTP server for email
    /// The LLM handles SMTP commands and mail delivery (port 25)
    Smtp,

    /// mDNS/DNS-SD stack - Multicast DNS service discovery
    /// The LLM advertises services on the local network (port 5353)
    Mdns,

    /// MySQL stack - MySQL server using opensrv-mysql
    /// The LLM handles SQL queries and generates result sets (port 3306)
    Mysql,

    /// IPP stack - IPP (Internet Printing Protocol) server
    /// The LLM handles print jobs and printer attributes (port 631)
    Ipp,

    /// PostgreSQL stack - PostgreSQL server using pgwire
    /// The LLM handles SQL queries and generates result sets (port 5432)
    Postgresql,

    /// Redis stack - Redis server with RESP protocol
    /// The LLM handles Redis commands and data operations (port 6379)
    Redis,

    /// HTTP Proxy stack - HTTP/HTTPS proxy server using http-mitm-proxy
    /// The LLM intercepts, modifies, and forwards HTTP requests (port 8080/3128)
    Proxy,

    /// WebDAV stack - WebDAV file server using dav-server
    /// The LLM handles WebDAV file operations over HTTP (port 80/443)
    WebDav,

    /// NFS stack - NFSv3 server using nfsserve
    /// The LLM handles NFS file system operations (port 2049)
    Nfs,

    /// IMAP stack - IMAP mail retrieval server using imap-codec
    /// The LLM handles IMAP mailbox operations and email retrieval (port 143/993)
    Imap,

    /// Elasticsearch stack - Elasticsearch/OpenSearch server using hyper
    /// The LLM handles search queries and generates JSON responses (port 9200)
    Elasticsearch,

    /// WireGuard stack - WireGuard VPN honeypot
    /// The LLM detects WireGuard handshake attempts and logs reconnaissance (port 51820)
    Wireguard,

    /// OpenVPN stack - OpenVPN honeypot
    /// The LLM detects OpenVPN connections and logs reconnaissance (port 1194)
    Openvpn,

    /// IPSec/IKEv2 stack - IPSec VPN honeypot
    /// The LLM detects IKEv2 handshake attempts and logs reconnaissance (port 500/4500)
    Ipsec,

    /// SOCKS5 stack - SOCKS5 proxy server
    /// The LLM controls proxy decisions, authentication, and optional traffic inspection (port 1080)
    Socks5,

    /// SMB stack - SMB/CIFS file server using smb-msg
    /// The LLM handles SMB file operations (port 445)
    Smb,

    /// Cassandra stack - Cassandra/CQL database server
    /// The LLM handles CQL queries (port 9042)
    Cassandra,

    /// STUN stack - STUN server for NAT traversal
    /// The LLM handles STUN binding requests (port 3478)
    Stun,

    /// TURN stack - TURN relay server for NAT traversal
    /// The LLM handles TURN allocations and relaying (port 3478)
    Turn,


    /// DynamoDB stack - DynamoDB-compatible database server
    /// The LLM handles DynamoDB API operations over HTTP (port 8000)
    Dynamo,

    /// OpenAI stack - OpenAI-compatible API server
    /// The LLM handles chat completions and model listings (port 11435/8000)
    OpenAi,

    /// LDAP stack - LDAP directory server
    /// The LLM handles LDAP operations and directory queries (port 389)
    Ldap,

    /// BGP stack - BGP routing protocol
    /// The LLM handles BGP peering, route announcements, and withdrawals (port 179)
    Bgp,
}

impl BaseStack {
    /// Get default base stack
    pub fn default() -> Self {
        Self::Tcp
    }
}

impl std::fmt::Display for BaseStack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Use registry to get stack name
        if let Some(stack_name) = crate::protocol::registry::registry().stack_name(self) {
            write!(f, "{}", stack_name)
        } else {
            write!(f, "{:?}", self)
        }
    }
}
