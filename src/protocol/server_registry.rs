//! Protocol registry
//!
//! This module provides a centralized registry that maps protocol names
//! to their protocol implementations. It enables trait-based protocol lookup
//! and keyword-based parsing.

use super::metadata::ProtocolMetadataV2;
use crate::llm::actions::Server;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::error;

/// Global protocol registry mapping protocol names to protocol implementations
pub struct ServerRegistry {
    /// Maps protocol name (e.g., "TCP", "HTTP") to protocol implementation
    protocols: HashMap<String, Arc<dyn Server>>,
    /// Maps lowercase keywords to protocol name for fast parsing
    keyword_map: HashMap<String, String>,
}

impl ServerRegistry {
    /// Create a new protocol registry
    fn new() -> Self {
        let mut registry = Self {
            protocols: HashMap::new(),
            keyword_map: HashMap::new(),
        };

        // Register all protocols based on feature flags
        registry.register_protocols();
        registry.build_keyword_map();

        // Validate that no keywords overlap between protocols
        registry.validate_keyword_uniqueness();

        registry
    }

    /// Register all available protocols based on compiled features
    fn register_protocols(&mut self) {
        #[cfg(feature = "tcp")]
        self.register(Arc::new(crate::server::TcpProtocol::new()));

        #[cfg(all(feature = "socket_file", unix))]
        self.register(Arc::new(crate::server::SocketFileProtocol::new()));

        #[cfg(feature = "http")]
        self.register(Arc::new(crate::server::HttpProtocol::new()));

        #[cfg(feature = "http2")]
        self.register(Arc::new(crate::server::Http2Protocol::new()));

        #[cfg(feature = "pypi")]
        self.register(Arc::new(crate::server::PypiProtocol::new()));

        #[cfg(feature = "maven")]
        self.register(Arc::new(crate::server::MavenProtocol::new()));

        #[cfg(feature = "udp")]
        self.register(Arc::new(crate::server::UdpProtocol::new()));

        #[cfg(feature = "datalink")]
        self.register(
            Arc::new(crate::server::DataLinkProtocol::new()),
        );

        #[cfg(feature = "arp")]
        self.register(
            Arc::new(crate::server::ArpProtocol::new()),
        );

        #[cfg(feature = "dc")]
        self.register(Arc::new(crate::server::DcProtocol::new()));

        #[cfg(feature = "dns")]
        self.register(Arc::new(crate::server::DnsProtocol::new()));

        #[cfg(feature = "dot")]
        self.register(Arc::new(crate::server::DotProtocol::new()));

        #[cfg(feature = "doh")]
        self.register(Arc::new(crate::server::DohProtocol::new()));

        #[cfg(feature = "dhcp")]
        self.register(Arc::new(crate::server::DhcpProtocol::new()));

        #[cfg(feature = "bootp")]
        self.register(Arc::new(crate::server::BootpProtocol::new()));

        #[cfg(feature = "ntp")]
        self.register(Arc::new(crate::server::NtpProtocol::new()));

        #[cfg(feature = "whois")]
        self.register(Arc::new(crate::server::WhoisProtocol::new()));

        #[cfg(feature = "snmp")]
        self.register(Arc::new(crate::server::SnmpProtocol::new()));

        #[cfg(feature = "igmp")]
        self.register(Arc::new(crate::server::IgmpProtocol::new()));

        #[cfg(feature = "syslog")]
        self.register(Arc::new(crate::server::SyslogProtocol::new()));

        #[cfg(feature = "ssh")]
        self.register(Arc::new(crate::server::SshProtocol::new()));

        #[cfg(all(feature = "ssh-agent", unix))]
        self.register(Arc::new(crate::server::SshAgentProtocol::new()));

        #[cfg(feature = "svn")]
        self.register(Arc::new(crate::server::SvnProtocol::new()));

        #[cfg(feature = "irc")]
        self.register(Arc::new(crate::server::IrcProtocol::new()));

        #[cfg(feature = "xmpp")]
        self.register(Arc::new(crate::server::XmppProtocol::new()));

        #[cfg(feature = "telnet")]
        self.register(
            Arc::new(crate::server::TelnetProtocol::new()),
        );

        #[cfg(feature = "smtp")]
        self.register(Arc::new(crate::server::SmtpProtocol::new()));

        #[cfg(feature = "imap")]
        self.register(Arc::new(crate::server::ImapProtocol::new()));

        #[cfg(feature = "nntp")]
        self.register(Arc::new(crate::server::NntpProtocol::new()));

        #[cfg(feature = "mqtt")]
        self.register(Arc::new(crate::server::MqttProtocol::new()));

        #[cfg(feature = "mdns")]
        self.register(Arc::new(crate::server::MdnsProtocol::new()));

        #[cfg(feature = "ldap")]
        self.register(Arc::new(crate::server::LdapProtocol::new()));

        #[cfg(feature = "mysql")]
        {
            use crate::server::connection::ConnectionId;
            use tokio::sync::mpsc;
            let (tx, _rx) = mpsc::unbounded_channel();
            self.register(
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
                Arc::new(crate::server::CassandraProtocol::new(
                    ConnectionId::new(),
                    Arc::new(crate::state::app_state::AppState::new()),
                    tx,
                )),
            );
        }

        #[cfg(feature = "dynamo")]
        self.register(
            Arc::new(crate::server::DynamoProtocol::new()),
        );

        #[cfg(feature = "s3")]
        self.register(
            Arc::new(crate::server::S3Protocol::new()),
        );

        #[cfg(feature = "sqs")]
        self.register(
            Arc::new(crate::server::SqsProtocol::new()),
        );

        #[cfg(feature = "elasticsearch")]
        self.register(
            Arc::new(crate::server::ElasticsearchProtocol::new()),
        );

        #[cfg(feature = "npm")]
        self.register(
            Arc::new(crate::server::NpmProtocol::new()),
        );

        #[cfg(feature = "ipp")]
        self.register(Arc::new(crate::server::IppProtocol::new()));

        #[cfg(feature = "webdav")]
        self.register(
            Arc::new(crate::server::WebDavProtocol::new()),
        );

        #[cfg(feature = "nfs")]
        self.register(Arc::new(crate::server::NfsProtocol::new()));

        #[cfg(feature = "smb")]
        self.register(Arc::new(crate::server::SmbProtocol::new()));

        #[cfg(feature = "proxy")]
        self.register(
            Arc::new(crate::server::ProxyProtocol::new()),
        );

        #[cfg(feature = "socks5")]
        self.register(
            Arc::new(crate::server::Socks5Protocol::new()),
        );

        #[cfg(feature = "wireguard")]
        self.register(
            Arc::new(crate::server::WireguardProtocol::new()),
        );

        #[cfg(feature = "openvpn")]
        self.register(
            Arc::new(crate::server::OpenvpnProtocol::new()),
        );

        #[cfg(feature = "ipsec")]
        self.register(
            Arc::new(crate::server::IpsecProtocol::new()),
        );

        #[cfg(feature = "stun")]
        self.register(Arc::new(crate::server::StunProtocol::new()));

        #[cfg(feature = "turn")]
        self.register(Arc::new(crate::server::TurnProtocol::new()));

        #[cfg(feature = "sip")]
        self.register(Arc::new(crate::server::SipProtocol::new()));

        #[cfg(feature = "bgp")]
        self.register(Arc::new(crate::server::BgpProtocol::new()));

        #[cfg(feature = "ospf")]
        self.register(Arc::new(crate::server::OspfProtocol::new()));

        #[cfg(feature = "isis")]
        self.register(Arc::new(crate::server::IsisProtocol::new()));

        #[cfg(feature = "rip")]
        self.register(Arc::new(crate::server::RipProtocol::new()));

        #[cfg(feature = "bitcoin")]
        self.register(Arc::new(crate::server::BitcoinProtocol::new()));

        #[cfg(feature = "mcp")]
        self.register(Arc::new(crate::server::McpProtocol::new()));

        #[cfg(feature = "openai")]
        self.register(
            Arc::new(crate::server::OpenAiProtocol::new()),
        );

        #[cfg(feature = "oauth2")]
        self.register(
            Arc::new(crate::server::OAuth2Protocol::new()),
        );

        #[cfg(feature = "jsonrpc")]
        self.register(
            Arc::new(crate::server::JsonRpcProtocol::new()),
        );

        #[cfg(feature = "xmlrpc")]
        self.register(
            Arc::new(crate::server::XmlRpcProtocol::new()),
        );

        #[cfg(feature = "grpc")]
        self.register(Arc::new(crate::server::GrpcProtocol::new()));

        #[cfg(feature = "etcd")]
        self.register(Arc::new(crate::server::EtcdProtocol::new()));

        #[cfg(feature = "tor")]
        self.register(
            Arc::new(crate::server::TorDirectoryProtocol::new()),
        );

        #[cfg(feature = "tor")]
        self.register(
            Arc::new(crate::server::TorRelayProtocol::new()),
        );

        #[cfg(feature = "vnc")]
        self.register(Arc::new(crate::server::VncProtocol::new()));

        #[cfg(feature = "openapi")]
        self.register(
            Arc::new(crate::server::OpenApiProtocol::new()),
        );

        #[cfg(feature = "openid")]
        self.register(
            Arc::new(crate::server::OpenIdProtocol::new()),
        );

        #[cfg(feature = "git")]
        self.register(Arc::new(crate::server::GitProtocol::new()));

        #[cfg(feature = "mercurial")]
        self.register(Arc::new(crate::server::MercurialProtocol::new()));

        #[cfg(feature = "kafka")]
        self.register(Arc::new(crate::server::KafkaProtocol::new()));

        #[cfg(feature = "http3")]
        self.register(Arc::new(crate::server::Http3Protocol::new()));

        #[cfg(feature = "torrent-tracker")]
        self.register(Arc::new(crate::server::TorrentTrackerProtocol::new()));

        #[cfg(feature = "torrent-dht")]
        self.register(Arc::new(crate::server::TorrentDhtProtocol::new()));

        #[cfg(feature = "torrent-peer")]
        self.register(Arc::new(crate::server::TorrentPeerProtocol::new()));

        #[cfg(feature = "tls")]
        self.register(Arc::new(crate::server::TlsProtocol::new()));

        #[cfg(feature = "saml-idp")]
        self.register(Arc::new(crate::server::SamlIdpProtocol::new()));

        #[cfg(feature = "saml-sp")]
        self.register(Arc::new(crate::server::SamlSpProtocol::new()));
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

    /// Get keyword overlaps between protocols
    ///
    /// Returns a vector of (keyword, protocols) tuples for keywords
    /// that are claimed by multiple protocols.
    pub fn get_keyword_overlaps(&self) -> Vec<(String, Vec<(String, String)>)> {
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

        overlaps
    }

    /// Validate that no two protocols share the same keyword
    ///
    /// This logs warnings for any overlapping keywords detected,
    /// but does not panic to allow the application to continue running.
    fn validate_keyword_uniqueness(&self) {
        let overlaps = self.get_keyword_overlaps();

        // If overlaps found, log warnings with detailed information
        if !overlaps.is_empty() {
            error!("⚠️  WARNING: Keyword overlaps detected between protocols:\n");

            for (keyword, protocols) in &overlaps {
                error!("  Keyword '{}' is used by:", keyword);
                for (protocol_name, source) in protocols {
                    error!("    - {} ({})", protocol_name, source);
                }
            }

            error!("Note: Each keyword should ideally be unique to a single protocol.");
            error!("      Run 'cargo test test_keyword_overlaps -- --ignored' to see all overlaps.\n");
        }
    }

    /// Register a protocol implementation
    #[allow(dead_code)]
    fn register(&mut self, protocol: Arc<dyn Server>) {
        let protocol_name = protocol.protocol_name().to_string();
        self.protocols.insert(protocol_name, protocol);
    }

    /// Get protocol implementation by protocol name
    pub fn get(&self, protocol_name: &str) -> Option<Arc<dyn Server>> {
        self.protocols.get(protocol_name).cloned()
    }

    /// Parse protocol from user input string
    ///
    /// Attempts to match keywords from registered protocols.
    /// Returns protocol name if match found, None otherwise.
    pub fn parse_from_str(&self, input: &str) -> Option<String> {
        let input_lower = input.to_lowercase();

        // First, try exact match with stack names (for LLM-generated responses)
        for (protocol_name, protocol) in &self.protocols {
            if input_lower == protocol.stack_name().to_lowercase() {
                return Some(protocol_name.clone());
            }
        }

        // Try keyword matching with priority ordering
        // More specific protocols checked first to avoid substring collisions

        // Priority 1: Check mDNS before DNS (avoid substring match)
        if let Some(stack) = self.match_protocol_by_any_keyword(&input_lower, "mDNS") {
            return Some(stack);
        }

        // Priority 2: Check IMAP before SMTP (more specific for mail/email)
        if let Some(stack) = self.match_protocol_by_any_keyword(&input_lower, "IMAP") {
            return Some(stack);
        }

        // Priority 3: Check PostgreSQL before MySQL (avoid "sql" substring)
        if let Some(stack) = self.match_protocol_by_any_keyword(&input_lower, "PostgreSQL") {
            return Some(stack);
        }

        // Priority 4: Check XML-RPC and JSON-RPC before HTTP (avoid "http" substring in stack names)
        if let Some(stack) = self.match_protocol_by_any_keyword(&input_lower, "XmlRPC") {
            return Some(stack);
        }
        if let Some(stack) = self.match_protocol_by_any_keyword(&input_lower, "JsonRPC") {
            return Some(stack);
        }

        // Priority 5: Check Proxy before HTTP (avoid "http" substring in "http proxy")
        if let Some(stack) = self.match_protocol_by_any_keyword(&input_lower, "Proxy") {
            return Some(stack);
        }

        // Priority 6: Check Tor protocols before TCP fallback
        if let Some(stack) = self.match_protocol_by_any_keyword(&input_lower, "TorDirectory") {
            return Some(stack);
        }
        if let Some(stack) = self.match_protocol_by_any_keyword(&input_lower, "TorRelay") {
            return Some(stack);
        }

        // For all other protocols, check ALL keywords from each protocol
        for (protocol_name, protocol) in &self.protocols {
            for keyword in protocol.keywords() {
                if input_lower.contains(&keyword.to_lowercase()) {
                    return Some(protocol_name.clone());
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
            return Some("TCP".to_string());
        }

        None
    }

    /// Match a specific protocol by checking if input contains ANY of its keywords
    ///
    /// This method checks ALL keywords defined by the protocol, not just a hardcoded subset.
    /// Returns the protocol name if any keyword matches.
    fn match_protocol_by_any_keyword(&self, input_lower: &str, protocol_name: &str) -> Option<String> {
        if let Some(protocol) = self.protocols.get(protocol_name) {
            for keyword in protocol.keywords() {
                if input_lower.contains(&keyword.to_lowercase()) {
                    return Some(protocol_name.to_string());
                }
            }
        }
        None
    }

    /// Get list of available protocol names
    pub fn available_protocols(&self) -> Vec<&'static str> {
        let mut protocols: Vec<&'static str> = self.protocols
            .values()
            .map(|p| p.protocol_name())
            .collect();
        // Sort alphabetically for deterministic output
        protocols.sort();
        protocols
    }

    /// Get stack name by protocol name (e.g., "HTTP" -> "ETH>IP>TCP>HTTP")
    pub fn stack_name_by_protocol(&self, protocol_name: &str) -> Option<&'static str> {
        self.get(protocol_name).map(|p| p.stack_name())
    }

    /// Get metadata for a protocol by name
    pub fn metadata(&self, protocol_name: &str) -> Option<ProtocolMetadataV2> {
        self.get(protocol_name).map(|p| p.metadata())
    }

    /// Get all registered protocols with their metadata
    pub fn all_protocols(&self) -> Vec<(String, Arc<dyn Server>)> {
        self.protocols
            .iter()
            .map(|(name, protocol)| (name.clone(), Arc::clone(protocol)))
            .collect()
    }

    /// Get protocols that are excluded due to missing dependencies
    ///
    /// Returns a map of protocol name -> list of missing dependencies
    pub fn get_excluded_protocols(
        &self,
        caps: &crate::privilege::SystemCapabilities,
    ) -> HashMap<String, Vec<super::dependencies::ProtocolDependency>> {
        let mut excluded = HashMap::new();

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

/// Global singleton registry instance
static REGISTRY: once_cell::sync::Lazy<ServerRegistry> =
    once_cell::sync::Lazy::new(ServerRegistry::new);

/// Get the global protocol registry
pub fn registry() -> &'static ServerRegistry {
    &REGISTRY
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to build a registry without validation panic
    /// (for testing purposes only)
    fn build_test_registry() -> ServerRegistry {
        let mut registry = ServerRegistry {
            protocols: HashMap::new(),
            keyword_map: HashMap::new(),
        };

        // Register all protocols based on feature flags
        registry.register_protocols();
        registry.build_keyword_map();

        // Don't validate - just return the registry
        registry
    }

    #[test]
    fn test_keyword_overlaps() {
        // Build registry without validation to see what overlaps exist
        let registry = build_test_registry();
        let overlaps = registry.get_keyword_overlaps();

        if overlaps.is_empty() {
            println!("✓ No keyword overlaps detected!");
            return;
        }

        // Print all overlaps
        println!("\n{} keyword overlaps detected:\n", overlaps.len());

        for (keyword, protocols) in &overlaps {
            println!("  Keyword '{}' is used by:", keyword);
            for (protocol_name, source) in protocols {
                println!("    - {} ({})", protocol_name, source);
            }
            println!();
        }

        // List all protocols with their keywords
        println!("\n=== All Protocol Keywords ===\n");
        let mut protocol_keywords = Vec::new();
        for (protocol_name, protocol) in &registry.protocols {
            let keywords = protocol
                .keywords()
                .iter()
                .map(|k| k.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            protocol_keywords.push((protocol_name.clone(), keywords));
        }

        protocol_keywords.sort_by(|a, b| a.0.cmp(&b.0));
        for (protocol_name, keywords) in &protocol_keywords {
            println!("  {}: {}", protocol_name, keywords);
        }

        // Fail the test if there are overlaps
        assert!(
            overlaps.is_empty(),
            "Found {} keyword overlaps. See above for details.",
            overlaps.len()
        );
    }
}
