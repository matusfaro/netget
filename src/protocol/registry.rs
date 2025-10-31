//! Protocol registry
//!
//! This module provides a centralized registry that maps BaseStack enum variants
//! to their protocol implementations. It enables trait-based protocol lookup
//! and keyword-based parsing.

use super::base_stack::{BaseStack, ProtocolMetadata};
use crate::llm::actions::protocol_trait::ProtocolActions;
use std::collections::HashMap;
use std::sync::Arc;

/// Global protocol registry mapping BaseStack to protocol implementations
pub struct ProtocolRegistry {
    /// Maps BaseStack enum to protocol implementation
    protocols: HashMap<BaseStack, Arc<dyn ProtocolActions>>,
    /// Maps lowercase keywords to BaseStack for fast parsing
    keyword_map: HashMap<String, BaseStack>,
}

impl ProtocolRegistry {
    /// Create a new protocol registry
    fn new() -> Self {
        let mut registry = Self {
            protocols: HashMap::new(),
            keyword_map: HashMap::new(),
        };

        // Register all protocols based on feature flags
        registry.register_protocols();
        registry.build_keyword_map();

        registry
    }

    /// Register all available protocols based on compiled features
    fn register_protocols(&mut self) {
        // Core protocols
        #[cfg(feature = "tcp")]
        self.register(BaseStack::Tcp, Arc::new(crate::server::TcpProtocol::new()));

        #[cfg(feature = "http")]
        self.register(BaseStack::Http, Arc::new(crate::server::HttpProtocol::new()));

        #[cfg(feature = "udp")]
        self.register(BaseStack::Udp, Arc::new(crate::server::UdpProtocol::new()));

        self.register(
            BaseStack::DataLink,
            Arc::new(crate::server::DataLinkProtocol::new()),
        );

        #[cfg(feature = "dns")]
        self.register(BaseStack::Dns, Arc::new(crate::server::DnsProtocol::new()));

        #[cfg(feature = "dhcp")]
        self.register(BaseStack::Dhcp, Arc::new(crate::server::DhcpProtocol::new()));

        #[cfg(feature = "ntp")]
        self.register(BaseStack::Ntp, Arc::new(crate::server::NtpProtocol::new()));

        #[cfg(feature = "snmp")]
        self.register(BaseStack::Snmp, Arc::new(crate::server::SnmpProtocol::new()));

        #[cfg(feature = "ssh")]
        self.register(BaseStack::Ssh, Arc::new(crate::server::SshProtocol::new()));

        // Application protocols
        #[cfg(feature = "irc")]
        self.register(BaseStack::Irc, Arc::new(crate::server::IrcProtocol::new()));

        #[cfg(feature = "telnet")]
        self.register(
            BaseStack::Telnet,
            Arc::new(crate::server::TelnetProtocol::new()),
        );

        #[cfg(feature = "smtp")]
        self.register(BaseStack::Smtp, Arc::new(crate::server::SmtpProtocol::new()));

        #[cfg(feature = "imap")]
        self.register(BaseStack::Imap, Arc::new(crate::server::ImapProtocol::new()));

        #[cfg(feature = "mdns")]
        self.register(BaseStack::Mdns, Arc::new(crate::server::MdnsProtocol::new()));

        #[cfg(feature = "ldap")]
        self.register(BaseStack::Ldap, Arc::new(crate::server::LdapProtocol::new()));

        // Database protocols
        #[cfg(feature = "mysql")]
        {
            use crate::server::connection::ConnectionId;
            use tokio::sync::mpsc;
            let (tx, _rx) = mpsc::unbounded_channel();
            self.register(
                BaseStack::Mysql,
                Arc::new(crate::server::MysqlProtocol::new(
                    ConnectionId::new(),
                    Arc::new(crate::state::app_state::AppState::new()),
                    tx,
                )),
            );
        }

        #[cfg(feature = "postgresql")]
        {
            use crate::server::connection::ConnectionId;
            use tokio::sync::mpsc;
            let (tx, _rx) = mpsc::unbounded_channel();
            self.register(
                BaseStack::Postgresql,
                Arc::new(crate::server::PostgresqlProtocol::new(
                    ConnectionId::new(),
                    Arc::new(crate::state::app_state::AppState::new()),
                    tx,
                )),
            );
        }

        #[cfg(feature = "redis")]
        {
            use crate::server::connection::ConnectionId;
            use tokio::sync::mpsc;
            let (tx, _rx) = mpsc::unbounded_channel();
            self.register(
                BaseStack::Redis,
                Arc::new(crate::server::RedisProtocol::new(
                    ConnectionId::new(),
                    Arc::new(crate::state::app_state::AppState::new()),
                    tx,
                )),
            );
        }

        #[cfg(feature = "cassandra")]
        {
            use crate::server::connection::ConnectionId;
            use tokio::sync::mpsc;
            let (tx, _rx) = mpsc::unbounded_channel();
            self.register(
                BaseStack::Cassandra,
                Arc::new(crate::server::CassandraProtocol::new(
                    ConnectionId::new(),
                    Arc::new(crate::state::app_state::AppState::new()),
                    tx,
                )),
            );
        }

        #[cfg(feature = "dynamo")]
        self.register(
            BaseStack::Dynamo,
            Arc::new(crate::server::DynamoProtocol::new()),
        );

        #[cfg(feature = "elasticsearch")]
        self.register(
            BaseStack::Elasticsearch,
            Arc::new(crate::server::ElasticsearchProtocol::new()),
        );

        // Web & File protocols
        #[cfg(feature = "ipp")]
        self.register(BaseStack::Ipp, Arc::new(crate::server::IppProtocol::new()));

        #[cfg(feature = "webdav")]
        self.register(
            BaseStack::WebDav,
            Arc::new(crate::server::WebDavProtocol::new()),
        );

        #[cfg(feature = "nfs")]
        self.register(BaseStack::Nfs, Arc::new(crate::server::NfsProtocol::new()));

        #[cfg(feature = "smb")]
        self.register(BaseStack::Smb, Arc::new(crate::server::SmbProtocol::new()));

        // Proxy & Network protocols
        #[cfg(feature = "proxy")]
        self.register(
            BaseStack::Proxy,
            Arc::new(crate::server::ProxyProtocol::new()),
        );

        #[cfg(feature = "socks5")]
        self.register(
            BaseStack::Socks5,
            Arc::new(crate::server::Socks5Protocol::new()),
        );

        #[cfg(feature = "wireguard")]
        self.register(
            BaseStack::Wireguard,
            Arc::new(crate::server::WireguardProtocol::new()),
        );

        #[cfg(feature = "openvpn")]
        {
            use std::net::SocketAddr;
            let std_socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            std_socket.set_nonblocking(true).unwrap();
            let socket = tokio::net::UdpSocket::from_std(std_socket).unwrap();
            let addr = "127.0.0.1:1194".parse::<SocketAddr>().unwrap();
            self.register(
                BaseStack::Openvpn,
                Arc::new(crate::server::OpenvpnProtocol::new(socket, addr)),
            );
        }

        #[cfg(feature = "ipsec")]
        {
            use std::net::SocketAddr;
            let std_socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            std_socket.set_nonblocking(true).unwrap();
            let socket = tokio::net::UdpSocket::from_std(std_socket).unwrap();
            let addr = "127.0.0.1:500".parse::<SocketAddr>().unwrap();
            self.register(
                BaseStack::Ipsec,
                Arc::new(crate::server::IpsecProtocol::new(socket, addr)),
            );
        }

        #[cfg(feature = "stun")]
        self.register(BaseStack::Stun, Arc::new(crate::server::StunProtocol::new()));

        #[cfg(feature = "turn")]
        self.register(BaseStack::Turn, Arc::new(crate::server::TurnProtocol::new()));

        #[cfg(feature = "bgp")]
        self.register(BaseStack::Bgp, Arc::new(crate::server::BgpProtocol::new()));

        // AI & API protocols
        #[cfg(feature = "openai")]
        self.register(
            BaseStack::OpenAi,
            Arc::new(crate::server::OpenAiProtocol::new()),
        );
    }

    /// Build keyword map for fast protocol parsing
    fn build_keyword_map(&mut self) {
        for (base_stack, protocol) in &self.protocols {
            for keyword in protocol.keywords() {
                self.keyword_map
                    .insert(keyword.to_lowercase(), *base_stack);
            }
        }
    }

    /// Register a protocol implementation
    fn register(&mut self, stack: BaseStack, protocol: Arc<dyn ProtocolActions>) {
        self.protocols.insert(stack, protocol);
    }

    /// Get protocol implementation for a BaseStack
    pub fn get(&self, stack: &BaseStack) -> Option<Arc<dyn ProtocolActions>> {
        self.protocols.get(stack).cloned()
    }

    /// Parse protocol from user input string
    ///
    /// Attempts to match keywords from registered protocols.
    /// Returns None if no match found.
    pub fn parse_from_str(&self, input: &str) -> Option<BaseStack> {
        let input_lower = input.to_lowercase();

        // First, try exact match with stack names (for LLM-generated responses)
        for (stack, protocol) in &self.protocols {
            if input_lower == protocol.stack_name().to_lowercase() {
                return Some(*stack);
            }
        }

        // Try keyword matching with priority ordering
        // More specific protocols checked first to avoid substring collisions

        // Priority 1: Check SSH first (specific)
        if let Some(stack) = self.try_keyword_match(&input_lower, &["ssh"]) {
            return Some(stack);
        }

        // Priority 2: Check mDNS before DNS (avoid substring match)
        if let Some(stack) = self.try_keyword_match(&input_lower, &["mdns", "bonjour", "dns-sd", "zeroconf"]) {
            return Some(stack);
        }

        // Priority 3: Check DNS
        if let Some(stack) = self.try_keyword_match(&input_lower, &["dns"]) {
            return Some(stack);
        }

        // Priority 4: Check IMAP before SMTP (more specific for mail/email)
        if let Some(stack) = self.try_keyword_match(&input_lower, &["imap"]) {
            return Some(stack);
        }

        // Priority 5: Check SMTP
        if let Some(stack) = self.try_keyword_match(&input_lower, &["smtp", "mail", "email"]) {
            return Some(stack);
        }

        // Priority 6: Check PostgreSQL before MySQL (avoid "sql" substring)
        if let Some(stack) = self.try_keyword_match(&input_lower, &["postgres", "psql"]) {
            return Some(stack);
        }

        // For all other protocols, check keywords in registration order
        for (stack, protocol) in &self.protocols {
            for keyword in protocol.keywords() {
                if input_lower.contains(&keyword.to_lowercase()) {
                    return Some(*stack);
                }
            }
        }

        // Default to TCP if "tcp", "raw", "ftp", "custom" found
        #[cfg(feature = "tcp")]
        if input_lower.contains("tcp")
            || input_lower.contains("raw")
            || input_lower.contains("ftp")
            || input_lower.contains("custom")
        {
            return Some(BaseStack::Tcp);
        }

        None
    }

    /// Try to match any of the given keywords in the input
    fn try_keyword_match(&self, input_lower: &str, keywords: &[&str]) -> Option<BaseStack> {
        for keyword in keywords {
            if input_lower.contains(keyword) {
                // Find which protocol has this keyword
                for (stack, protocol) in &self.protocols {
                    if protocol.keywords().contains(keyword) {
                        return Some(*stack);
                    }
                }
            }
        }
        None
    }

    /// Get list of available protocol names
    pub fn available_protocols(&self) -> Vec<&'static str> {
        self.protocols
            .values()
            .map(|p| p.protocol_name())
            .collect()
    }

    /// Get stack name for a BaseStack
    pub fn stack_name(&self, stack: &BaseStack) -> Option<&'static str> {
        self.get(stack).map(|p| p.stack_name())
    }

    /// Get metadata for a BaseStack
    pub fn metadata(&self, stack: &BaseStack) -> Option<ProtocolMetadata> {
        self.get(stack).map(|p| p.metadata())
    }

    /// Get all registered protocols with their metadata
    pub fn all_protocols(&self) -> Vec<(BaseStack, Arc<dyn ProtocolActions>)> {
        self.protocols
            .iter()
            .map(|(stack, protocol)| (*stack, Arc::clone(protocol)))
            .collect()
    }
}

/// Global singleton registry instance
static REGISTRY: once_cell::sync::Lazy<ProtocolRegistry> =
    once_cell::sync::Lazy::new(ProtocolRegistry::new);

/// Get the global protocol registry
pub fn registry() -> &'static ProtocolRegistry {
    &REGISTRY
}
