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
    /// Implementation abandoned, not functional (not shown in LLM prompts)
    Abandoned,
}

impl ProtocolState {
    /// Get the string representation for display
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Implemented => "Implemented",
            Self::Beta => "Beta",
            Self::Alpha => "Alpha",
            Self::Abandoned => "Abandoned",
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
        self.state != ProtocolState::Abandoned
    }
}

/// Base protocol stack types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// Get protocol metadata (state, notes)
    pub fn metadata(&self) -> ProtocolMetadata {
        match self {
            // Core Protocols (Beta)
            Self::Tcp => ProtocolMetadata::new(ProtocolState::Beta),
            Self::Http => ProtocolMetadata::new(ProtocolState::Beta),
            Self::Udp => ProtocolMetadata::new(ProtocolState::Beta),
            Self::DataLink => ProtocolMetadata::new(ProtocolState::Beta),
            Self::Dns => ProtocolMetadata::new(ProtocolState::Beta),
            Self::Dhcp => ProtocolMetadata::new(ProtocolState::Beta),
            Self::Ntp => ProtocolMetadata::new(ProtocolState::Beta),
            Self::Snmp => ProtocolMetadata::new(ProtocolState::Beta),
            Self::Ssh => ProtocolMetadata::new(ProtocolState::Beta),

            // Application Protocols (Alpha)
            Self::Irc => ProtocolMetadata::new(ProtocolState::Alpha),
            Self::Telnet => ProtocolMetadata::new(ProtocolState::Alpha),
            Self::Smtp => ProtocolMetadata::new(ProtocolState::Alpha),
            Self::Imap => ProtocolMetadata::new(ProtocolState::Alpha),
            Self::Mdns => ProtocolMetadata::new(ProtocolState::Alpha),
            Self::Ldap => ProtocolMetadata::new(ProtocolState::Alpha),

            // Database Protocols (Alpha)
            Self::Mysql => ProtocolMetadata::new(ProtocolState::Alpha),
            Self::Postgresql => ProtocolMetadata::new(ProtocolState::Alpha),
            Self::Redis => ProtocolMetadata::new(ProtocolState::Alpha),
            Self::Cassandra => ProtocolMetadata::new(ProtocolState::Alpha),
            Self::Dynamo => ProtocolMetadata::new(ProtocolState::Alpha),
            Self::Elasticsearch => ProtocolMetadata::new(ProtocolState::Alpha),

            // Web & File Protocols (Alpha)
            Self::Ipp => ProtocolMetadata::new(ProtocolState::Alpha),
            Self::WebDav => ProtocolMetadata::new(ProtocolState::Alpha),
            Self::Nfs => ProtocolMetadata::new(ProtocolState::Alpha),
            Self::Smb => ProtocolMetadata::new(ProtocolState::Alpha),

            // Proxy & Network Protocols (Alpha/Implemented)
            Self::Proxy => ProtocolMetadata::new(ProtocolState::Alpha),
            Self::Socks5 => ProtocolMetadata::new(ProtocolState::Alpha),
            Self::Wireguard => ProtocolMetadata::with_notes(
                ProtocolState::Implemented,
                "Full VPN server with actual tunnel support using defguard_wireguard_rs. Creates TUN interface and supports peer connections."
            ),
            Self::Stun => ProtocolMetadata::new(ProtocolState::Alpha),
            Self::Turn => ProtocolMetadata::new(ProtocolState::Alpha),
            Self::Bgp => ProtocolMetadata::new(ProtocolState::Alpha),

            // VPN Protocols - Abandoned
            Self::Openvpn => ProtocolMetadata::with_notes(
                ProtocolState::Abandoned,
                "Honeypot only - no actual VPN tunnels. Full OpenVPN implementation is infeasible: no viable Rust library exists, protocol is extremely complex (500K+ lines in C++). Use WireGuard for production VPN. OpenVPN honeypot sufficient for detection/logging reconnaissance attempts."
            ),
            Self::Ipsec => ProtocolMetadata::with_notes(
                ProtocolState::Abandoned,
                "Honeypot only - no actual VPN tunnels. Full IPSec/IKEv2 implementation is infeasible: no viable Rust library (ipsec-parser is parse-only), protocol requires deep OS integration (XFRM policy), extremely complex (hundreds of thousands of lines in strongSwan). Use WireGuard for production VPN."
            ),

            // AI & API Protocols (Alpha)
            Self::OpenAi => ProtocolMetadata::new(ProtocolState::Alpha),
        }
    }

    /// Get the stack name as a string
    pub fn name(&self) -> &'static str {
        match self {
            Self::Tcp => "ETH>IP>TCP",
            Self::Http => "ETH>IP>TCP>HTTP",
            Self::DataLink => "ETH",
            Self::Udp => "ETH>IP>UDP",
            Self::Dns => "ETH>IP>UDP>DNS",
            Self::Dhcp => "ETH>IP>UDP>DHCP",
            Self::Ntp => "ETH>IP>UDP>NTP",
            Self::Snmp => "ETH>IP>UDP>SNMP",
            Self::Ssh => "ETH>IP>TCP>SSH",
            Self::Irc => "ETH>IP>TCP>IRC",
            Self::Telnet => "ETH>IP>TCP>Telnet",
            Self::Smtp => "ETH>IP>TCP>SMTP",
            Self::Mdns => "ETH>IP>UDP>mDNS",
            Self::Mysql => "ETH>IP>TCP>MySQL",
            Self::Ipp => "ETH>IP>TCP>HTTP>IPP",
            Self::Postgresql => "ETH>IP>TCP>PostgreSQL",
            Self::Redis => "ETH>IP>TCP>Redis",
            Self::Proxy => "ETH>IP>TCP>HTTP>PROXY",
            Self::WebDav => "ETH>IP>TCP>HTTP>WEBDAV",
            Self::Nfs => "ETH>IP>TCP>NFS",
            Self::Imap => "ETH>IP>TCP>IMAP",
            Self::Wireguard => "ETH>IP>UDP>WG",
            Self::Openvpn => "ETH>IP>TCP/UDP>OPENVPN",
            Self::Socks5 => "ETH>IP>TCP>SOCKS5",
            Self::Ipsec => "ETH>IP>UDP>IPSEC",
            Self::Smb => "ETH>IP>TCP>SMB",
            Self::Cassandra => "ETH>IP>TCP>Cassandra",
            Self::Stun => "ETH>IP>UDP>STUN",
            Self::Turn => "ETH>IP>UDP>TURN",
            Self::Elasticsearch => "ETH>IP>TCP>HTTP>ELASTICSEARCH",
            Self::Dynamo => "ETH>IP>TCP>HTTP>DYNAMODB",
            Self::OpenAi => "ETH>IP>TCP>HTTP>OPENAI",
            Self::Ldap => "ETH>IP>TCP>LDAP",
            Self::Bgp => "ETH>IP>TCP>BGP",
        }
    }

    /// Parse base stack from string
    pub fn from_str(s: &str) -> Option<Self> {
        let s_lower = s.to_lowercase();

        // First, try exact match with stack names (for LLM-generated responses)
        // This allows LLM to specify exact stack like "ETH>IP>TCP>HTTP"
        match s_lower.as_str() {
            "eth>ip>tcp" => return Some(Self::Tcp),
            "eth>ip>tcp>http" => {
                #[cfg(feature = "http")]
                return Some(Self::Http);
            }
            "eth" => return Some(Self::DataLink),
            "eth>ip>udp" => return Some(Self::Udp),
            "eth>ip>udp>dns" => {
                #[cfg(feature = "dns")]
                return Some(Self::Dns);
            }
            "eth>ip>udp>dhcp" => {
                #[cfg(feature = "dhcp")]
                return Some(Self::Dhcp);
            }
            "eth>ip>udp>ntp" => {
                #[cfg(feature = "ntp")]
                return Some(Self::Ntp);
            }
            "eth>ip>udp>snmp" => {
                #[cfg(feature = "snmp")]
                return Some(Self::Snmp);
            }
            "eth>ip>tcp>ssh" => {
                #[cfg(feature = "ssh")]
                return Some(Self::Ssh);
            }
            "eth>ip>tcp>irc" => {
                #[cfg(feature = "irc")]
                return Some(Self::Irc);
            }
            "eth>ip>tcp>telnet" => {
                #[cfg(feature = "telnet")]
                return Some(Self::Telnet);
            }
            "eth>ip>tcp>smtp" => {
                #[cfg(feature = "smtp")]
                return Some(Self::Smtp);
            }
            "eth>ip>udp>mdns" => {
                #[cfg(feature = "mdns")]
                return Some(Self::Mdns);
            }
            "eth>ip>tcp>mysql" => {
                #[cfg(feature = "mysql")]
                return Some(Self::Mysql);
            }
            "eth>ip>tcp>http>ipp" => {
                #[cfg(feature = "ipp")]
                return Some(Self::Ipp);
            }
            "eth>ip>tcp>postgresql" => {
                #[cfg(feature = "postgresql")]
                return Some(Self::Postgresql);
            }
            "eth>ip>tcp>redis" => {
                #[cfg(feature = "redis")]
                return Some(Self::Redis);
            }
            "eth>ip>tcp>http>proxy" => {
                #[cfg(feature = "proxy")]
                return Some(Self::Proxy);
            }
            "eth>ip>tcp>http>webdav" => {
                #[cfg(feature = "webdav")]
                return Some(Self::WebDav);
            }
            "eth>ip>tcp>nfs" => {
                #[cfg(feature = "nfs")]
                return Some(Self::Nfs);
            }
            "eth>ip>tcp>imap" => {
                #[cfg(feature = "imap")]
                return Some(Self::Imap);
            }
            "eth>ip>tcp>socks5" => {
                #[cfg(feature = "socks5")]
                return Some(Self::Socks5);
            }
            "eth>ip>tcp>smb" => {
                #[cfg(feature = "smb")]
                return Some(Self::Smb);
            }
            "eth>ip>tcp>cassandra" => {
                #[cfg(feature = "cassandra")]
                return Some(Self::Cassandra);
            }
            "eth>ip>udp>stun" => {
                #[cfg(feature = "stun")]
                return Some(Self::Stun);
            }
            "eth>ip>udp>turn" => {
                #[cfg(feature = "turn")]
                return Some(Self::Turn);
            }
            "eth>ip>tcp>http>elasticsearch" => {
                #[cfg(feature = "elasticsearch")]
                return Some(Self::Elasticsearch);
            }
            "eth>ip>tcp>http>dynamodb" => {
                #[cfg(feature = "dynamo")]
                return Some(Self::Dynamo);
            }
            "eth>ip>tcp>http>openai" => {
                #[cfg(feature = "openai")]
                return Some(Self::OpenAi);
            }
            "eth>ip>tcp>ldap" => {
                #[cfg(feature = "ldap")]
                return Some(Self::Ldap);
            }
            "eth>ip>udp>wg" => {
                #[cfg(feature = "wireguard")]
                return Some(Self::Wireguard);
            }
            "eth>ip>tcp/udp>openvpn" => {
                #[cfg(feature = "openvpn")]
                return Some(Self::Openvpn);
            }
            "eth>ip>tcp>bgp" => {
                #[cfg(feature = "bgp")]
                return Some(Self::Bgp);
            }
            _ => {} // Fall through to keyword matching
        }

        // Check for specific protocol keywords (more specific matches)

        // SSH stack
        #[cfg(feature = "ssh")]
        if s_lower.contains("ssh") {
            return Some(Self::Ssh);
        }

        // mDNS stack (check before DNS to avoid substring match)
        #[cfg(feature = "mdns")]
        if s_lower.contains("mdns")
            || s_lower.contains("bonjour")
            || s_lower.contains("dns-sd")
            || s_lower.contains("zeroconf")
        {
            return Some(Self::Mdns);
        }

        // DNS stack
        #[cfg(feature = "dns")]
        if s_lower.contains("dns") {
            return Some(Self::Dns);
        }

        // DHCP stack
        #[cfg(feature = "dhcp")]
        if s_lower.contains("dhcp") {
            return Some(Self::Dhcp);
        }

        // NTP stack
        #[cfg(feature = "ntp")]
        if s_lower.contains("ntp") || s_lower.contains("time") {
            return Some(Self::Ntp);
        }

        // SNMP stack
        #[cfg(feature = "snmp")]
        if s_lower.contains("snmp") {
            return Some(Self::Snmp);
        }

        // IRC stack
        #[cfg(feature = "irc")]
        if s_lower.contains("irc") || s_lower.contains("chat") {
            return Some(Self::Irc);
        }

        // Telnet stack
        #[cfg(feature = "telnet")]
        if s_lower.contains("telnet") {
            return Some(Self::Telnet);
        }

        // IMAP stack (check before SMTP to be more specific for mail/email keywords)
        #[cfg(feature = "imap")]
        if s_lower.contains("imap") {
            return Some(Self::Imap);
        }

        // SMTP stack
        #[cfg(feature = "smtp")]
        if s_lower.contains("smtp") || s_lower.contains("mail") || s_lower.contains("email") {
            return Some(Self::Smtp);
        }

        // PostgreSQL stack (check before MySQL to avoid "sql" substring match)
        #[cfg(feature = "postgresql")]
        if s_lower.contains("postgres") || s_lower.contains("psql") {
            return Some(Self::Postgresql);
        }

        // MySQL stack
        #[cfg(feature = "mysql")]
        if s_lower.contains("mysql") {
            return Some(Self::Mysql);
        }

        // Redis stack
        #[cfg(feature = "redis")]
        if s_lower.contains("redis") {
            return Some(Self::Redis);
        }

        // IPP stack
        #[cfg(feature = "ipp")]
        if s_lower.contains("ipp") || s_lower.contains("printer") || s_lower.contains("print") {
            return Some(Self::Ipp);
        }

        // HTTP Proxy stack
        #[cfg(feature = "proxy")]
        if s_lower.contains("proxy") || s_lower.contains("mitm") {
            return Some(Self::Proxy);
        }

        // WebDAV stack
        #[cfg(feature = "webdav")]
        if s_lower.contains("webdav") || s_lower.contains("dav") {
            return Some(Self::WebDav);
        }

        // SOCKS5 stack
        #[cfg(feature = "socks5")]
        if s_lower.contains("socks") {
            return Some(Self::Socks5);
        }

        // NFS stack
        #[cfg(feature = "nfs")]
        if s_lower.contains("nfs") || s_lower.contains("file server") {
            return Some(Self::Nfs);
        }

        // SMB stack
        #[cfg(feature = "smb")]
        if s_lower.contains("smb") || s_lower.contains("cifs") {
            return Some(Self::Smb);
        }

        // Cassandra stack
        #[cfg(feature = "cassandra")]
        if s_lower.contains("cassandra") || s_lower.contains("cql") {
            return Some(Self::Cassandra);
        }

        // STUN stack
        #[cfg(feature = "stun")]
        if s_lower.contains("stun") {
            return Some(Self::Stun);
        }

        // TURN stack
        #[cfg(feature = "turn")]
        if s_lower.contains("turn") {
            return Some(Self::Turn);
        }

        // Elasticsearch stack
        #[cfg(feature = "elasticsearch")]
        if s_lower.contains("elasticsearch") || s_lower.contains("opensearch") {
            return Some(Self::Elasticsearch);
        }

        // DynamoDB stack
        #[cfg(feature = "dynamo")]
        if s_lower.contains("dynamo") {
            return Some(Self::Dynamo);
        }

        // OpenAI stack
        #[cfg(feature = "openai")]
        if s_lower.contains("openai") {
            return Some(Self::OpenAi);
        }

        // LDAP stack
        #[cfg(feature = "ldap")]
        if s_lower.contains("ldap") || s_lower.contains("directory server") {
            return Some(Self::Ldap);
        }

        // WireGuard stack
        #[cfg(feature = "wireguard")]
        if s_lower.contains("wireguard") || s_lower.contains("wg") {
            return Some(Self::Wireguard);
        }

        // OpenVPN stack
        #[cfg(feature = "openvpn")]
        if s_lower.contains("openvpn") {
            return Some(Self::Openvpn);
        }

        // BGP stack
        #[cfg(feature = "bgp")]
        if s_lower.contains("bgp") || s_lower.contains("border gateway") {
            return Some(Self::Bgp);
        }

        // WireGuard VPN stack
        #[cfg(feature = "wireguard")]
        if s_lower.contains("wireguard") || s_lower.contains("wg") {
            return Some(Self::Wireguard);
        }

        // OpenVPN stack
        #[cfg(feature = "openvpn")]
        if s_lower.contains("openvpn") {
            return Some(Self::Openvpn);
        }

        // Elasticsearch stack
        #[cfg(feature = "elasticsearch")]
        if s_lower.contains("elasticsearch") || s_lower.contains("opensearch") {
            return Some(Self::Elasticsearch);
        }

        // IPSec/IKEv2 VPN stack
        #[cfg(feature = "ipsec")]
        if s_lower.contains("ipsec") || s_lower.contains("ikev2") || s_lower.contains("ike") {
            return Some(Self::Ipsec);
        }

        // UDP raw stack
        #[cfg(feature = "udp")]
        if s_lower.contains("udp") {
            return Some(Self::Udp);
        }

        // Data Link stack indicators
        if s_lower.contains("datalink")
            || s_lower.contains("data link")
            || s_lower.contains("layer 2")
            || s_lower.contains("layer2")
            || s_lower.contains("l2")
            || s_lower.contains("ethernet")
            || s_lower.contains("arp")
            || s_lower.contains("pcap")
        {
            return Some(Self::DataLink);
        }

        // HTTP stack indicators
        #[cfg(feature = "http")]
        if s_lower.contains("http stack")
            || s_lower.contains("http server")
            || (s_lower.contains("via http") && !s_lower.contains("tcp"))
            || s_lower.contains("hyper")
        {
            return Some(Self::Http);
        }

        // TCP/IP raw stack indicators
        #[cfg(feature = "tcp")]
        if s_lower.contains("tcp")
            || s_lower.contains("raw")
            || s_lower.contains("ftp")
            || s_lower.contains("custom")
        {
            return Some(Self::Tcp);
        }

        // Default to TCP/IP raw for backwards compatibility (if available)
        #[cfg(feature = "tcp")]
        {
            None
        }
        #[cfg(not(feature = "tcp"))]
        {
            None
        }
    }

    /// Get default base stack
    pub fn default() -> Self {
        Self::Tcp
    }

    /// Get list of available base stacks based on compiled features
    pub fn available_stacks() -> Vec<&'static str> {
        let mut stacks = vec![];

        #[cfg(feature = "tcp")]
        stacks.push("tcp_raw");

        #[cfg(feature = "http")]
        stacks.push("http");

        stacks.push("datalink"); // Always available

        #[cfg(feature = "udp")]
        stacks.push("udp_raw");

        #[cfg(feature = "dns")]
        stacks.push("dns");

        #[cfg(feature = "dhcp")]
        stacks.push("dhcp");

        #[cfg(feature = "ntp")]
        stacks.push("ntp");

        #[cfg(feature = "snmp")]
        stacks.push("snmp");

        #[cfg(feature = "ssh")]
        stacks.push("ssh");

        #[cfg(feature = "irc")]
        stacks.push("irc");

        #[cfg(feature = "telnet")]
        stacks.push("telnet");

        #[cfg(feature = "smtp")]
        stacks.push("smtp");

        #[cfg(feature = "mdns")]
        stacks.push("mdns");

        #[cfg(feature = "mysql")]
        stacks.push("mysql");

        #[cfg(feature = "ipp")]
        stacks.push("ipp");

        #[cfg(feature = "postgresql")]
        stacks.push("postgresql");

        #[cfg(feature = "redis")]
        stacks.push("redis");

        #[cfg(feature = "proxy")]
        stacks.push("proxy");

        #[cfg(feature = "webdav")]
        stacks.push("webdav");

        #[cfg(feature = "nfs")]
        stacks.push("nfs");

        #[cfg(feature = "imap")]
        stacks.push("imap");

        #[cfg(feature = "socks5")]
        stacks.push("socks5");

        #[cfg(feature = "smb")]
        stacks.push("smb");

        #[cfg(feature = "cassandra")]
        stacks.push("cassandra");

        #[cfg(feature = "stun")]
        stacks.push("stun");

        #[cfg(feature = "turn")]
        stacks.push("turn");

        #[cfg(feature = "elasticsearch")]
        stacks.push("elasticsearch");

        #[cfg(feature = "dynamo")]
        stacks.push("dynamo");

        #[cfg(feature = "openai")]
        stacks.push("openai");

        #[cfg(feature = "ldap")]
        stacks.push("ldap");

        #[cfg(feature = "wireguard")]
        stacks.push("wireguard");

        #[cfg(feature = "openvpn")]
        stacks.push("openvpn");

        #[cfg(feature = "bgp")]
        stacks.push("bgp");

        #[cfg(feature = "elasticsearch")]
        stacks.push("elasticsearch");

        stacks
    }

    // Note: get_event_types() method has been removed from BaseStack.
    // Each protocol now implements the ProtocolStack trait directly.
    // To get event types, instantiate the protocol and call .get_event_types() on it.
    // Example: HttpProtocol::new().get_event_types()
}

impl std::fmt::Display for BaseStack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_http_stack() {
        assert_eq!(BaseStack::from_str("http stack"), Some(BaseStack::Http));
        assert_eq!(BaseStack::from_str("http server"), Some(BaseStack::Http));
        assert_eq!(BaseStack::from_str("via http"), Some(BaseStack::Http));
    }

    #[test]
    fn test_parse_tcp_stack() {
        assert_eq!(BaseStack::from_str("tcp"), Some(BaseStack::Tcp));
        assert_eq!(BaseStack::from_str("raw tcp"), Some(BaseStack::Tcp));
        assert_eq!(BaseStack::from_str("ftp"), Some(BaseStack::Tcp));
    }

    #[test]
    fn test_parse_udp_stack() {
        assert_eq!(BaseStack::from_str("udp"), Some(BaseStack::Udp));
        assert_eq!(BaseStack::from_str("via udp"), Some(BaseStack::Udp));
    }

    #[test]
    fn test_parse_dns_stack() {
        assert_eq!(BaseStack::from_str("dns"), Some(BaseStack::Dns));
        assert_eq!(BaseStack::from_str("via dns"), Some(BaseStack::Dns));
        assert_eq!(BaseStack::from_str("dns server"), Some(BaseStack::Dns));
    }

    #[test]
    fn test_parse_dhcp_stack() {
        assert_eq!(BaseStack::from_str("dhcp"), Some(BaseStack::Dhcp));
        assert_eq!(BaseStack::from_str("dhcp server"), Some(BaseStack::Dhcp));
    }

    #[test]
    fn test_parse_ntp_stack() {
        assert_eq!(BaseStack::from_str("ntp"), Some(BaseStack::Ntp));
        assert_eq!(BaseStack::from_str("time server"), Some(BaseStack::Ntp));
    }

    #[test]
    fn test_parse_snmp_stack() {
        assert_eq!(BaseStack::from_str("snmp"), Some(BaseStack::Snmp));
        assert_eq!(BaseStack::from_str("snmp agent"), Some(BaseStack::Snmp));
    }

    #[test]
    fn test_parse_ssh_stack() {
        assert_eq!(BaseStack::from_str("ssh"), Some(BaseStack::Ssh));
        assert_eq!(BaseStack::from_str("ssh server"), Some(BaseStack::Ssh));
        assert_eq!(BaseStack::from_str("via ssh"), Some(BaseStack::Ssh));
    }

    #[test]
    fn test_parse_irc_stack() {
        assert_eq!(BaseStack::from_str("irc"), Some(BaseStack::Irc));
        assert_eq!(BaseStack::from_str("chat server"), Some(BaseStack::Irc));
        assert_eq!(BaseStack::from_str("irc chat"), Some(BaseStack::Irc));
    }

    #[test]
    fn test_parse_telnet_stack() {
        assert_eq!(BaseStack::from_str("telnet"), Some(BaseStack::Telnet));
        assert_eq!(
            BaseStack::from_str("telnet server"),
            Some(BaseStack::Telnet)
        );
    }

    #[test]
    fn test_parse_smtp_stack() {
        assert_eq!(BaseStack::from_str("smtp"), Some(BaseStack::Smtp));
        assert_eq!(BaseStack::from_str("mail server"), Some(BaseStack::Smtp));
        assert_eq!(BaseStack::from_str("email server"), Some(BaseStack::Smtp));
    }

    #[test]
    fn test_parse_mdns_stack() {
        assert_eq!(BaseStack::from_str("mdns"), Some(BaseStack::Mdns));
        assert_eq!(BaseStack::from_str("bonjour"), Some(BaseStack::Mdns));
        assert_eq!(BaseStack::from_str("dns-sd"), Some(BaseStack::Mdns));
    }

    #[test]
    fn test_parse_proxy_stack() {
        assert_eq!(BaseStack::from_str("proxy"), Some(BaseStack::Proxy));
        assert_eq!(BaseStack::from_str("http proxy"), Some(BaseStack::Proxy));
        assert_eq!(BaseStack::from_str("mitm"), Some(BaseStack::Proxy));
    }

    #[test]
    fn test_parse_webdav_stack() {
        assert_eq!(BaseStack::from_str("webdav"), Some(BaseStack::WebDav));
        assert_eq!(BaseStack::from_str("dav server"), Some(BaseStack::WebDav));
        assert_eq!(BaseStack::from_str("via webdav"), Some(BaseStack::WebDav));
    }

    #[test]
    fn test_parse_nfs_stack() {
        assert_eq!(BaseStack::from_str("nfs"), Some(BaseStack::Nfs));
        assert_eq!(BaseStack::from_str("file server"), Some(BaseStack::Nfs));
        assert_eq!(BaseStack::from_str("nfs server"), Some(BaseStack::Nfs));
    }

    #[test]
    fn test_parse_imap_stack() {
        assert_eq!(BaseStack::from_str("imap"), Some(BaseStack::Imap));
        assert_eq!(BaseStack::from_str("imap server"), Some(BaseStack::Imap));
        assert_eq!(BaseStack::from_str("via imap"), Some(BaseStack::Imap));
    }

    #[test]
    fn test_parse_socks5_stack() {
        assert_eq!(BaseStack::from_str("socks5"), Some(BaseStack::Socks5));
        assert_eq!(BaseStack::from_str("socks proxy"), Some(BaseStack::Socks5));
        assert_eq!(BaseStack::from_str("eth>ip>tcp>socks5"), Some(BaseStack::Socks5));
    }

    #[test]
    fn test_parse_smb_stack() {
        assert_eq!(BaseStack::from_str("smb"), Some(BaseStack::Smb));
        assert_eq!(BaseStack::from_str("cifs"), Some(BaseStack::Smb));
        assert_eq!(BaseStack::from_str("eth>ip>tcp>smb"), Some(BaseStack::Smb));
    }

    #[test]
    fn test_parse_cassandra_stack() {
        assert_eq!(BaseStack::from_str("cassandra"), Some(BaseStack::Cassandra));
        assert_eq!(BaseStack::from_str("cql"), Some(BaseStack::Cassandra));
        assert_eq!(BaseStack::from_str("eth>ip>tcp>cassandra"), Some(BaseStack::Cassandra));
    }

    #[test]
    fn test_parse_stun_stack() {
        assert_eq!(BaseStack::from_str("stun"), Some(BaseStack::Stun));
        assert_eq!(BaseStack::from_str("eth>ip>udp>stun"), Some(BaseStack::Stun));
    }

    #[test]
    fn test_parse_turn_stack() {
        assert_eq!(BaseStack::from_str("turn"), Some(BaseStack::Turn));
        assert_eq!(BaseStack::from_str("eth>ip>udp>turn"), Some(BaseStack::Turn));
    }

    #[test]
    fn test_parse_elasticsearch_stack() {
        assert_eq!(BaseStack::from_str("elasticsearch"), Some(BaseStack::Elasticsearch));
        assert_eq!(BaseStack::from_str("opensearch"), Some(BaseStack::Elasticsearch));
        assert_eq!(BaseStack::from_str("eth>ip>tcp>http>elasticsearch"), Some(BaseStack::Elasticsearch));
    }

    #[test]
    fn test_parse_dynamo_stack() {
        assert_eq!(BaseStack::from_str("dynamo"), Some(BaseStack::Dynamo));
        assert_eq!(BaseStack::from_str("eth>ip>tcp>http>dynamodb"), Some(BaseStack::Dynamo));
    }

    #[test]
    fn test_parse_openai_stack() {
        assert_eq!(BaseStack::from_str("openai"), Some(BaseStack::OpenAi));
        assert_eq!(BaseStack::from_str("eth>ip>tcp>http>openai"), Some(BaseStack::OpenAi));
    }

    #[test]
    fn test_parse_ldap_stack() {
        assert_eq!(BaseStack::from_str("ldap"), Some(BaseStack::Ldap));
        assert_eq!(BaseStack::from_str("directory server"), Some(BaseStack::Ldap));
        assert_eq!(BaseStack::from_str("eth>ip>tcp>ldap"), Some(BaseStack::Ldap));
    }

    #[test]
    fn test_parse_wireguard_stack() {
        assert_eq!(BaseStack::from_str("wireguard"), Some(BaseStack::Wireguard));
        assert_eq!(BaseStack::from_str("wg"), Some(BaseStack::Wireguard));
        assert_eq!(BaseStack::from_str("eth>ip>udp>wg"), Some(BaseStack::Wireguard));
    }

    #[test]
    fn test_parse_openvpn_stack() {
        assert_eq!(BaseStack::from_str("openvpn"), Some(BaseStack::Openvpn));
        assert_eq!(BaseStack::from_str("eth>ip>tcp/udp>openvpn"), Some(BaseStack::Openvpn));
    }

    #[test]
    fn test_parse_bgp_stack() {
        assert_eq!(BaseStack::from_str("bgp"), Some(BaseStack::Bgp));
        assert_eq!(BaseStack::from_str("border gateway"), Some(BaseStack::Bgp));
        assert_eq!(BaseStack::from_str("eth>ip>tcp>bgp"), Some(BaseStack::Bgp));
    }

    #[test]
    fn test_parse_ipsec_stack() {
        assert_eq!(BaseStack::from_str("ipsec"), Some(BaseStack::Ipsec));
        assert_eq!(BaseStack::from_str("ikev2"), Some(BaseStack::Ipsec));
    }
}
