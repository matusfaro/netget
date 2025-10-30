//! Base protocol stack definitions
//!
//! Defines the underlying network stack used by the application.
//! Each stack determines how network data is processed and what the LLM controls.

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
}

impl BaseStack {
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

        // NFS stack
        #[cfg(feature = "nfs")]
        if s_lower.contains("nfs") || s_lower.contains("file server") {
            return Some(Self::Nfs);
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

