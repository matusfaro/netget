//! Protocol trait - common interface for all network protocols
//!
//! This module defines the core Protocol trait that both Server and Client traits extend.
//! It contains common functionality shared across all protocol implementations.

use super::{ActionDefinition, ParameterDefinition};
use crate::protocol::dependencies::ProtocolDependency;
use crate::state::app_state::AppState;

/// Common trait for all protocol implementations (both servers and clients)
///
/// This trait defines the common interface that all protocol implementations
/// must provide, regardless of whether they're servers or clients.
pub trait Protocol: Send + Sync {
    /// Get startup parameters that can be provided when starting this protocol
    ///
    /// These parameters configure the protocol before it starts. Examples:
    /// - HTTP: request_headers, user_agent, follow_redirects (client)
    /// - HTTP: certificate_mode, request_filter_mode (server)
    /// - SSH: username, password, private_key_path (client)
    /// - SSH: host_key_path, banner_message (server)
    ///
    /// Default implementation returns empty vector (no startup parameters).
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        Vec::new()
    }

    /// Get async actions that can be executed anytime from user input
    ///
    /// These actions don't require network context. Examples:
    /// - HTTP client: send_request(method, path, headers, body)
    /// - HTTP server: close_connection(id), send_to_connection(id, data)
    /// - Redis client: execute_command(cmd, args)
    /// - Generic: disconnect(), reconnect()
    fn get_async_actions(&self, state: &AppState) -> Vec<ActionDefinition>;

    /// Get sync actions available during network events
    ///
    /// These actions only make sense in response to network events. Examples:
    /// - TCP: send_tcp_data(output), wait_for_more()
    /// - HTTP: send_http_response(status, headers, body)
    /// - SSH: handle_auth_challenge(), send_command_output()
    fn get_sync_actions(&self) -> Vec<ActionDefinition>;

    /// Get protocol name for debugging and identification
    ///
    /// This should be a short, uppercase identifier. Examples:
    /// - "TCP", "HTTP", "SSH", "DNS", "WireGuard"
    fn protocol_name(&self) -> &'static str;

    /// Get the event types that this protocol can emit
    ///
    /// Each event type includes:
    /// - A unique ID (e.g., "http_request", "ssh_auth")
    /// - A description of when it occurs
    /// - The actions that can be used to respond to this event
    ///
    /// # Returns
    /// A vector of EventType definitions for this protocol
    ///
    /// Default implementation returns empty vector (protocol hasn't migrated to event system yet)
    fn get_event_types(&self) -> Vec<crate::protocol::EventType> {
        Vec::new()
    }

    /// Get the stack name (e.g., "ETH>IP>TCP>HTTP")
    ///
    /// This represents the network stack layers used by this protocol.
    /// Used for display in UI and logging.
    fn stack_name(&self) -> &'static str;

    /// Get parsing keywords for protocol detection
    ///
    /// Returns a list of keywords that can be used to identify this protocol
    /// from user input. Examples:
    /// - HTTP: ["http", "http server", "via http", "hyper"]
    /// - SSH: ["ssh"]
    /// - mDNS: ["mdns", "bonjour", "dns-sd", "zeroconf"]
    ///
    /// Keywords are matched case-insensitively as substrings.
    fn keywords(&self) -> Vec<&'static str>;

    /// Get protocol metadata with implementation details
    ///
    /// Returns detailed metadata including:
    /// - Protocol state (Incomplete, Experimental, Beta, Stable)
    /// - Implementation approach description
    /// - LLM control scope description
    /// - E2E testing approach description
    /// - Privilege requirements
    /// - Optional notes
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2;

    /// Get a short description of this protocol
    ///
    /// This should be a concise, one-line description of what this protocol does.
    /// Examples:
    /// - HTTP: "Web server serving HTTP traffic"
    /// - SSH: "Secure shell server for remote access"
    /// - DNS: "Domain name resolution server"
    fn description(&self) -> &'static str;

    /// Get an example prompt that would trigger this protocol
    ///
    /// This should be a realistic, engaging example that demonstrates
    /// how a user would ask the LLM to start this protocol.
    /// Examples:
    /// - HTTP server: "Pretend to be a sassy HTTP server on port 8080 serving cooking recipes"
    /// - HTTP client: "Connect to http://example.com:8080 and fetch /api/status every 10 seconds"
    /// - SSH server: "Pretend to be a shell via SSH on port 2222"
    fn example_prompt(&self) -> &'static str;

    /// Get the group name for categorizing this protocol
    ///
    /// Protocols are grouped in documentation by category. Valid groups:
    /// - "Core" - Stable, well-tested protocols (TCP, HTTP, UDP, DNS, etc.)
    /// - "Application" - IRC, Telnet, SMTP, IMAP, MQTT, etc.
    /// - "Database" - MySQL, PostgreSQL, Redis, Kafka, etcd, etc.
    /// - "Web & File" - WebDAV, NFS, SMB, IPP, Git, S3
    /// - "Proxy & Network" - HTTP Proxy, SOCKS5, STUN, TURN
    /// - "VPN & Routing" - WireGuard, OpenVPN, IPSec, BGP
    /// - "AI & API" - OpenAI, gRPC, JSON-RPC, MCP, etc.
    /// - "Network Services" - VNC, Tor Directory, Tor Relay
    ///
    /// This method is mandatory and must be implemented by all protocols.
    fn group_name(&self) -> &'static str;

    /// Get runtime dependencies required for this protocol to function
    ///
    /// Returns a list of dependencies that must be available for this protocol
    /// to work at runtime. Examples:
    /// - ARP/DataLink: vec![ProtocolDependency::SystemLibrary("pcap"), ProtocolDependency::RawSocketAccess]
    /// - WireGuard: vec![ProtocolDependency::TunDeviceAccess, ProtocolDependency::RootAccess]
    /// - gRPC: vec![ProtocolDependency::ToolInPath("protoc")] (for .proto file support)
    /// - SSH on port 22: vec![ProtocolDependency::PrivilegedPort(22)]
    ///
    /// Default implementation returns empty vector (no special dependencies).
    fn get_dependencies(&self) -> Vec<ProtocolDependency> {
        Vec::new()
    }
}

/// Result of executing a protocol action
#[derive(Debug)]
pub enum ActionResult {
    /// Data to send over the connection/socket
    Output(Vec<u8>),

    /// Close the connection (connection-oriented protocols only)
    CloseConnection,

    /// Wait for more data before responding (accumulating state)
    WaitForMore,

    /// No action needed (e.g., logging, state update)
    NoAction,

    /// Multiple results (e.g., send data + close connection)
    Multiple(Vec<ActionResult>),

    /// Custom protocol-specific result with structured data
    ///
    /// This is used when a protocol needs to return structured information
    /// that isn't just "send these bytes". Protocols encode their responses
    /// as JSON in the 'data' field, and the protocol handler decodes and
    /// processes them.
    ///
    /// Examples:
    /// - SSH auth: {"name": "ssh_auth", "data": {"allowed": true}}
    /// - MySQL: {"name": "mysql_query", "data": {"columns": [...], "rows": [...]}}
    /// - Redis: {"name": "redis_string", "data": {"value": "OK"}}
    Custom {
        name: String,
        data: serde_json::Value,
    },
}

impl ActionResult {
    /// Check if this result contains output data
    pub fn has_output(&self) -> bool {
        match self {
            ActionResult::Output(_) => true,
            ActionResult::Multiple(results) => results.iter().any(|r| r.has_output()),
            _ => false,
        }
    }

    /// Check if this result closes the connection
    pub fn closes_connection(&self) -> bool {
        match self {
            ActionResult::CloseConnection => true,
            ActionResult::Multiple(results) => results.iter().any(|r| r.closes_connection()),
            _ => false,
        }
    }

    /// Check if this result waits for more data
    pub fn waits_for_more(&self) -> bool {
        match self {
            ActionResult::WaitForMore => true,
            ActionResult::Multiple(results) => results.iter().any(|r| r.waits_for_more()),
            _ => false,
        }
    }

    /// Extract all output data from results
    pub fn get_all_output(&self) -> Vec<Vec<u8>> {
        match self {
            ActionResult::Output(data) => vec![data.clone()],
            ActionResult::Multiple(results) => {
                results.iter().flat_map(|r| r.get_all_output()).collect()
            }
            _ => Vec::new(),
        }
    }
}

/// Trait for protocol server implementations
///
/// Each server protocol implements both the Protocol trait (for common functionality)
/// and this Server trait (for server-specific functionality like spawning).
///
/// The Server trait provides:
/// 1. Server spawning - how to start the protocol server
/// 2. Action executor - parses and executes protocol actions
pub trait Server: Protocol {
    /// Spawn a server instance for this protocol
    ///
    /// This is called when a server needs to be started. The implementation
    /// should bind to the listen address, set up any necessary resources,
    /// and return the actual bound address.
    ///
    /// # Arguments
    /// * `ctx` - Spawn context with all necessary dependencies
    ///
    /// # Returns
    /// * `Ok(SocketAddr)` - The actual address the server bound to
    /// * `Err(_)` - If server spawning failed
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    >;

    /// Execute a protocol-specific action
    ///
    /// # Arguments
    /// * `action` - The action JSON object from LLM
    ///
    /// # Returns
    /// * `Ok(ActionResult)` - Result of execution (data to send, close connection, etc.)
    /// * `Err(_)` - If action execution failed
    fn execute_action(&self, action: serde_json::Value) -> anyhow::Result<ActionResult>;
}
