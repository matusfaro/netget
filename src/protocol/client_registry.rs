//! Client protocol registry
//!
//! This module provides a centralized registry that maps client protocol names
//! to their client implementations. It enables trait-based client lookup
//! and keyword-based parsing for client connections.

use crate::llm::actions::Client;
use std::collections::HashMap;
use std::sync::Arc;

/// Global client protocol registry mapping protocol names to client implementations
pub struct ClientProtocolRegistry {
    /// Maps protocol name (e.g., "TCP", "HTTP") to client implementation
    protocols: HashMap<String, Arc<dyn Client>>,
    /// Maps lowercase keywords to protocol name for fast parsing
    keyword_map: HashMap<String, String>,
}

impl ClientProtocolRegistry {
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
        #[cfg(feature = "tcp")]
        self.register(Arc::new(crate::client::tcp::TcpClientProtocol::new()));

        #[cfg(feature = "http")]
        self.register(Arc::new(crate::client::http::HttpClientProtocol::new()));

        #[cfg(feature = "redis")]
        self.register(Arc::new(crate::client::redis::RedisClientProtocol::new()));

        #[cfg(feature = "sip")]
        self.register(Arc::new(crate::client::sip::SipClientProtocol::new()));
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
}

/// Global client protocol registry instance
///
/// This registry is initialized once at startup with all available client protocols
/// based on compiled features. Use `CLIENT_REGISTRY.get(protocol_name)` to retrieve
/// a client protocol implementation.
pub static CLIENT_REGISTRY: std::sync::LazyLock<ClientProtocolRegistry> = std::sync::LazyLock::new(ClientProtocolRegistry::new);
