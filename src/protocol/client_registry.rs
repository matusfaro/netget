//! Client protocol registry
//!
//! This module provides a centralized registry that maps client protocol names
//! to their client implementations. It enables trait-based client lookup
//! and keyword-based parsing for client connections.

use crate::llm::actions::Client;
use std::collections::HashMap;
use std::sync::Arc;

/// Global client protocol registry mapping protocol names to client implementations
pub struct ClientRegistry {
    /// Maps protocol name (e.g., "TCP", "HTTP") to client implementation
    protocols: HashMap<String, Arc<dyn Client>>,
    /// Maps lowercase keywords to protocol name for fast parsing
    keyword_map: HashMap<String, String>,
}

impl ClientRegistry {
    /// Create a new client protocol registry
    fn new() -> Self {
        let mut registry = Self {
            protocols: HashMap::new(),
            keyword_map: HashMap::new(),
        };

        // Register all client protocols based on feature flags
        registry.register_protocols();
        registry.build_keyword_map();

        // Validate that no keywords overlap between protocols
        registry.validate_keyword_uniqueness();

        registry
    }

    /// Register all available client protocols based on compiled features
    fn register_protocols(&mut self) {
        #[cfg(feature = "arp")]
        self.register(Arc::new(crate::client::arp::ArpClientProtocol::new()));

        #[cfg(feature = "bgp")]
        self.register(Arc::new(crate::client::bgp::BgpClientProtocol::new()));

        #[cfg(feature = "bitcoin")]
        self.register(Arc::new(crate::client::bitcoin::BitcoinClientProtocol::new()));

        #[cfg(feature = "bootp")]
        self.register(Arc::new(crate::client::bootp::BootpClientProtocol::new()));

        #[cfg(feature = "cassandra")]
        self.register(Arc::new(crate::client::cassandra::CassandraClientProtocol::new()));

        #[cfg(feature = "datalink")]
        self.register(Arc::new(crate::client::datalink::DataLinkClientProtocol::new()));

        #[cfg(feature = "dhcp")]
        self.register(Arc::new(crate::client::dhcp::DhcpClientProtocol::new()));

        #[cfg(feature = "dns")]
        self.register(Arc::new(crate::client::dns::DnsClientProtocol::new()));

        #[cfg(feature = "doh")]
        self.register(Arc::new(crate::client::doh::DohClientProtocol::new()));

        #[cfg(feature = "dot")]
        self.register(Arc::new(crate::client::dot::DotClientProtocol::new()));

        #[cfg(feature = "dynamodb")]
        self.register(Arc::new(crate::client::dynamodb::DynamoDbClientProtocol::new()));

        #[cfg(feature = "elasticsearch")]
        self.register(Arc::new(crate::client::elasticsearch::ElasticsearchClientProtocol::new()));

        #[cfg(feature = "etcd")]
        self.register(Arc::new(crate::client::etcd::EtcdClientProtocol::new()));

        #[cfg(feature = "git")]
        self.register(Arc::new(crate::client::git::GitClientProtocol::new()));

        #[cfg(feature = "grpc")]
        self.register(Arc::new(crate::client::grpc::GrpcClientProtocol::new()));

        #[cfg(feature = "http")]
        self.register(Arc::new(crate::client::http::HttpClientProtocol::new()));

        #[cfg(feature = "http2")]
        self.register(Arc::new(crate::client::http2::Http2ClientProtocol::new()));

        #[cfg(feature = "http3")]
        self.register(Arc::new(crate::client::http3::Http3ClientProtocol::new()));

        #[cfg(feature = "http_proxy")]
        self.register(Arc::new(crate::client::http_proxy::HttpProxyClientProtocol::new()));

        #[cfg(feature = "igmp")]
        self.register(Arc::new(crate::client::igmp::IgmpClientProtocol::new()));

        #[cfg(feature = "imap")]
        self.register(Arc::new(crate::client::imap::ImapClientProtocol::new()));

        #[cfg(feature = "ipp")]
        self.register(Arc::new(crate::client::ipp::IppClientProtocol::new()));

        #[cfg(feature = "irc")]
        self.register(Arc::new(crate::client::irc::IrcClientProtocol::new()));

        #[cfg(feature = "isis")]
        self.register(Arc::new(crate::client::isis::IsisClientProtocol::new()));

        #[cfg(feature = "jsonrpc")]
        self.register(Arc::new(crate::client::jsonrpc::JsonRpcClientProtocol::new()));

        #[cfg(feature = "kafka")]
        self.register(Arc::new(crate::client::kafka::KafkaClientProtocol::new()));

        #[cfg(feature = "kubernetes")]
        self.register(Arc::new(crate::client::kubernetes::KubernetesClientProtocol::new()));

        #[cfg(feature = "ldap")]
        self.register(Arc::new(crate::client::ldap::LdapClientProtocol::new()));

        #[cfg(feature = "maven")]
        self.register(Arc::new(crate::client::maven::MavenClientProtocol::new()));

        #[cfg(feature = "mcp")]
        self.register(Arc::new(crate::client::mcp::McpClientProtocol::new()));

        #[cfg(feature = "mdns")]
        self.register(Arc::new(crate::client::mdns::MdnsClientProtocol::new()));

        #[cfg(feature = "mqtt")]
        self.register(Arc::new(crate::client::mqtt::MqttClientProtocol::new()));

        #[cfg(feature = "mysql")]
        self.register(Arc::new(crate::client::mysql::MysqlClientProtocol::new()));

        #[cfg(feature = "nfs")]
        self.register(Arc::new(crate::client::nfs::NfsClientProtocol::new()));

        #[cfg(feature = "nntp")]
        self.register(Arc::new(crate::client::nntp::NntpClientProtocol::new()));

        #[cfg(feature = "npm")]
        self.register(Arc::new(crate::client::npm::NpmClientProtocol::new()));

        #[cfg(feature = "ntp")]
        self.register(Arc::new(crate::client::ntp::NtpClientProtocol::new()));

        #[cfg(feature = "oauth2")]
        self.register(Arc::new(crate::client::oauth2::OAuth2ClientProtocol::new()));

        #[cfg(feature = "openai")]
        self.register(Arc::new(crate::client::openai::OpenAiClientProtocol::new()));

        #[cfg(feature = "openidconnect")]
        self.register(Arc::new(crate::client::openidconnect::OpenIdConnectClientProtocol::new()));

        #[cfg(feature = "ospf")]
        self.register(Arc::new(crate::client::ospf::OspfClientProtocol::new()));

        #[cfg(feature = "postgresql")]
        self.register(Arc::new(crate::client::postgresql::PostgresqlClientProtocol::new()));

        #[cfg(feature = "pypi")]
        self.register(Arc::new(crate::client::pypi::PypiClientProtocol::new()));

        #[cfg(feature = "redis")]
        self.register(Arc::new(crate::client::redis::RedisClientProtocol::new()));

        #[cfg(feature = "rip")]
        self.register(Arc::new(crate::client::rip::RipClientProtocol::new()));

        #[cfg(feature = "s3")]
        self.register(Arc::new(crate::client::s3::S3ClientProtocol::new()));

        #[cfg(feature = "saml-idp")]
        self.register(Arc::new(crate::client::saml::SamlClientProtocol::new()));

        #[cfg(feature = "sip")]
        self.register(Arc::new(crate::client::sip::SipClientProtocol::new()));

        #[cfg(feature = "smb")]
        self.register(Arc::new(crate::client::smb::SmbClientProtocol::new()));

        #[cfg(feature = "smtp")]
        self.register(Arc::new(crate::client::smtp::SmtpClientProtocol::new()));

        #[cfg(feature = "snmp")]
        self.register(Arc::new(crate::client::snmp::SnmpClientProtocol::new()));

        #[cfg(feature = "socks5")]
        self.register(Arc::new(crate::client::socks5::Socks5ClientProtocol::new()));

        #[cfg(feature = "sqs")]
        self.register(Arc::new(crate::client::sqs::SqsClientProtocol::new()));

        #[cfg(feature = "ssh")]
        self.register(Arc::new(crate::client::ssh::SshClientProtocol::new()));

        #[cfg(feature = "stun")]
        self.register(Arc::new(crate::client::stun::StunClientProtocol::new()));

        #[cfg(feature = "syslog")]
        self.register(Arc::new(crate::client::syslog::SyslogClientProtocol::new()));

        #[cfg(feature = "tcp")]
        self.register(Arc::new(crate::client::tcp::TcpClientProtocol::new()));

        #[cfg(feature = "telnet")]
        self.register(Arc::new(crate::client::telnet::TelnetClientProtocol::new()));

        #[cfg(feature = "tor")]
        self.register(Arc::new(crate::client::tor::TorClientProtocol::new()));

        #[cfg(feature = "torrent-dht")]
        self.register(Arc::new(crate::client::torrent_dht::TorrentDhtClientProtocol::new()));

        #[cfg(feature = "torrent-peer")]
        self.register(Arc::new(crate::client::torrent_peer::TorrentPeerClientProtocol::new()));

        #[cfg(feature = "torrent-tracker")]
        self.register(Arc::new(crate::client::torrent_tracker::TorrentTrackerClientProtocol::new()));

        #[cfg(feature = "turn")]
        self.register(Arc::new(crate::client::turn::TurnClientProtocol::new()));

        #[cfg(feature = "udp")]
        self.register(Arc::new(crate::client::udp::UdpClientProtocol::new()));

        #[cfg(feature = "vnc")]
        self.register(Arc::new(crate::client::vnc::VncClientProtocol::new()));

        #[cfg(feature = "webdav")]
        self.register(Arc::new(crate::client::webdav::WebdavClientProtocol::new()));

        #[cfg(feature = "webrtc")]
        self.register(Arc::new(crate::client::webrtc::WebRtcClientProtocol::new()));

        #[cfg(feature = "whois")]
        self.register(Arc::new(crate::client::whois::WhoisClientProtocol::new()));

        #[cfg(feature = "wireguard")]
        self.register(Arc::new(crate::client::wireguard::WireguardClientProtocol::new()));

        #[cfg(feature = "xmlrpc")]
        self.register(Arc::new(crate::client::xmlrpc::XmlRpcClientProtocol::new()));

        #[cfg(feature = "xmpp")]
        self.register(Arc::new(crate::client::xmpp::XmppClientProtocol::new()));
    }

    /// Build keyword map for fast protocol parsing
    fn build_keyword_map(&mut self) {
        for (protocol_name, protocol) in &self.protocols {
            // Add all protocol keywords
            for keyword in protocol.keywords() {
                self.keyword_map
                    .insert(keyword.to_lowercase(), protocol_name.clone());
            }

            // Also add the full stack name as a keyword
            // This allows parsing inputs like "eth>ip>tcp>http" or "ETH>IP>UDP>DNS"
            let stack_name = protocol.stack_name().to_lowercase();
            self.keyword_map.insert(stack_name, protocol_name.clone());
        }
    }

    /// Validate that no two protocols share the same keyword
    ///
    /// This ensures keyword uniqueness across all registered protocols.
    /// Panics if overlapping keywords are detected.
    fn validate_keyword_uniqueness(&self) {
        use std::collections::HashMap;

        // Build a map: keyword (lowercase) -> Vec<(protocol_name, keyword_source)>
        // keyword_source is either "keyword" or "stack_name"
        let mut keyword_to_protocols: HashMap<String, Vec<(String, String)>> = HashMap::new();

        for (protocol_name, protocol) in &self.protocols {
            // Collect all keywords from keywords()
            for keyword in protocol.keywords() {
                let key = keyword.to_lowercase();
                keyword_to_protocols
                    .entry(key)
                    .or_insert_with(Vec::new)
                    .push((protocol_name.clone(), format!("keyword '{}'", keyword)));
            }

            // Also collect the stack name as a keyword
            let stack_name = protocol.stack_name();
            let key = stack_name.to_lowercase();
            keyword_to_protocols
                .entry(key)
                .or_insert_with(Vec::new)
                .push((protocol_name.clone(), format!("stack_name '{}'", stack_name)));
        }

        // Find all keywords that are claimed by multiple protocols
        let mut overlaps = Vec::new();
        for (keyword, protocols) in &keyword_to_protocols {
            if protocols.len() > 1 {
                overlaps.push((keyword.clone(), protocols.clone()));
            }
        }

        // If overlaps found, panic with detailed error message
        if !overlaps.is_empty() {
            let mut error_msg = String::from("Client keyword overlaps detected between protocols:\n");

            for (keyword, protocols) in overlaps {
                error_msg.push_str(&format!("\n  Keyword '{}' is used by:\n", keyword));
                for (protocol_name, source) in protocols {
                    error_msg.push_str(&format!("    - {} ({})\n", protocol_name, source));
                }
            }

            error_msg.push_str("\nEach keyword must be unique to a single protocol.");
            panic!("{}", error_msg);
        }
    }

    /// Register a client protocol implementation
    #[allow(dead_code)]
    fn register(&mut self, protocol: Arc<dyn Client>) {
        let protocol_name = protocol.protocol_name().to_string();
        self.protocols.insert(protocol_name, protocol);
    }

    /// Get client protocol implementation by protocol name
    pub fn get(&self, protocol_name: &str) -> Option<Arc<dyn Client>> {
        self.protocols.get(protocol_name).cloned()
    }

    /// Parse client protocol from user input string
    ///
    /// Attempts to match keywords from registered client protocols.
    /// Returns protocol name if match found, None otherwise.
    pub fn parse_from_str(&self, input: &str) -> Option<String> {
        let input_lower = input.to_lowercase();

        // First, try exact match with stack names (for LLM-generated responses)
        for (protocol_name, protocol) in &self.protocols {
            if input_lower == protocol.stack_name().to_lowercase() {
                return Some(protocol_name.clone());
            }
        }

        // Then try keyword matching (case-insensitive substring search)
        // This is a little greedy but works well in practice
        for (keyword, protocol_name) in &self.keyword_map {
            if input_lower.contains(keyword) {
                return Some(protocol_name.clone());
            }
        }

        None
    }

    /// List all registered client protocol names
    pub fn list_protocols(&self) -> Vec<String> {
        self.protocols.keys().cloned().collect()
    }

    /// Check if a protocol is registered
    pub fn has_protocol(&self, protocol_name: &str) -> bool {
        self.protocols.contains_key(protocol_name)
    }

    /// Get all registered client protocols
    pub fn get_all(&self) -> Vec<Arc<dyn Client>> {
        self.protocols.values().cloned().collect()
    }

    /// Get protocols that are excluded due to missing dependencies
    ///
    /// Returns a map of protocol name -> list of missing dependencies
    pub fn get_excluded_protocols(
        &self,
        caps: &crate::privilege::SystemCapabilities,
    ) -> std::collections::HashMap<String, Vec<super::dependencies::ProtocolDependency>> {
        let mut excluded = std::collections::HashMap::new();

        for (protocol_name, protocol) in &self.protocols {
            let dependencies = protocol.get_dependencies();
            let mut missing = Vec::new();

            for dep in dependencies {
                if !dep.is_available(caps) {
                    missing.push(dep);
                }
            }

            if !missing.is_empty() {
                excluded.insert(protocol_name.clone(), missing);
            }
        }

        excluded
    }

    /// Get protocols that are available (have all dependencies met)
    ///
    /// Returns a list of protocol names that can be used
    pub fn get_available_protocols(
        &self,
        caps: &crate::privilege::SystemCapabilities,
    ) -> Vec<String> {
        let excluded = self.get_excluded_protocols(caps);

        self.protocols
            .keys()
            .filter(|name| !excluded.contains_key(*name))
            .cloned()
            .collect()
    }

    /// Check if a specific protocol is available (has all dependencies met)
    pub fn is_protocol_available(
        &self,
        protocol_name: &str,
        caps: &crate::privilege::SystemCapabilities,
    ) -> bool {
        if let Some(protocol) = self.get(protocol_name) {
            let dependencies = protocol.get_dependencies();
            dependencies.iter().all(|dep| dep.is_available(caps))
        } else {
            false
        }
    }
}

/// Global client protocol registry instance
///
/// This registry is initialized once at startup with all available client protocols
/// based on compiled features. Use `CLIENT_REGISTRY.get(protocol_name)` to retrieve
/// a client protocol implementation.
pub static CLIENT_REGISTRY: std::sync::LazyLock<ClientRegistry> = std::sync::LazyLock::new(ClientRegistry::new);
