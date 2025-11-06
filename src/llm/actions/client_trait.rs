//! Client protocol trait
//!
//! This module defines the trait that all client protocols must implement
//! to provide their own action systems.

use super::{ActionDefinition, ParameterDefinition};
use crate::state::app_state::AppState;
use anyhow::Result;

/// Result of executing a client action
#[derive(Debug)]
pub enum ClientActionResult {
    /// Data to send to the server
    SendData(Vec<u8>),

    /// Disconnect from the server
    Disconnect,

    /// Wait for more data before responding (accumulating state)
    WaitForMore,

    /// No action needed (e.g., logging, state update)
    NoAction,

    /// Multiple results (e.g., send data + disconnect)
    Multiple(Vec<ClientActionResult>),

    /// Custom protocol-specific result with structured data
    ///
    /// This is used when a client needs to return structured information
    /// that isn't just "send these bytes". Clients encode their responses
    /// as JSON in the 'data' field, and the client handler decodes and
    /// processes them.
    ///
    /// Examples:
    /// - HTTP: {"name": "http_request", "data": {"method": "GET", "path": "/"}}
    /// - Redis: {"name": "redis_command", "data": {"command": "SET", "args": ["key", "value"]}}
    /// - SSH: {"name": "ssh_command", "data": {"command": "ls -la"}}
    Custom {
        name: String,
        data: serde_json::Value,
    },
}

impl ClientActionResult {
    /// Check if this result contains data to send
    pub fn has_data(&self) -> bool {
        match self {
            ClientActionResult::SendData(_) => true,
            ClientActionResult::Multiple(results) => results.iter().any(|r| r.has_data()),
            _ => false,
        }
    }

    /// Check if this result disconnects
    pub fn disconnects(&self) -> bool {
        match self {
            ClientActionResult::Disconnect => true,
            ClientActionResult::Multiple(results) => results.iter().any(|r| r.disconnects()),
            _ => false,
        }
    }

    /// Check if this result waits for more data
    pub fn waits_for_more(&self) -> bool {
        match self {
            ClientActionResult::WaitForMore => true,
            ClientActionResult::Multiple(results) => results.iter().any(|r| r.waits_for_more()),
            _ => false,
        }
    }

    /// Extract all data from results
    pub fn get_all_data(&self) -> Vec<Vec<u8>> {
        match self {
            ClientActionResult::SendData(data) => vec![data.clone()],
            ClientActionResult::Multiple(results) => {
                results.iter().flat_map(|r| r.get_all_data()).collect()
            }
            _ => Vec::new(),
        }
    }
}

/// Trait for client protocol implementations
///
/// Each protocol implements this trait to provide:
/// 1. Client connection - how to connect to a remote server
/// 2. Startup parameters - configuration accepted when connecting
/// 3. Async actions - executable anytime from user input
/// 4. Sync actions - executable during connection events
/// 5. Action executor - parses and executes client actions
/// 6. Protocol metadata - stack name, keywords, and implementation state
pub trait Client: Send + Sync {
    /// Connect to a remote server for this protocol
    ///
    /// This is called when a client needs to be started. The implementation
    /// should connect to the remote address, set up any necessary resources,
    /// and return the connected local socket address.
    ///
    /// # Arguments
    /// * `ctx` - Connect context with all necessary dependencies
    ///
    /// # Returns
    /// * `Ok(SocketAddr)` - The actual local address of the connection
    /// * `Err(_)` - If connection failed
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    >;

    /// Get startup parameters that can be provided when connecting
    ///
    /// These parameters configure the client before connecting. Examples:
    /// - HTTP: request_headers, user_agent, follow_redirects
    /// - SSH: username, password, private_key_path
    /// - MySQL: username, password, database
    ///
    /// Default implementation returns empty vector (no startup parameters).
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        Vec::new()
    }

    /// Get async actions that can be executed anytime from user input
    ///
    /// These actions don't require network context. Examples:
    /// - HTTP: send_request(method, path, headers, body)
    /// - Redis: execute_command(cmd, args)
    /// - SSH: send_command(cmd)
    /// - Generic: disconnect(), reconnect()
    fn get_async_actions(&self, state: &AppState) -> Vec<ActionDefinition>;

    /// Get sync actions available during connection events
    ///
    /// These actions only make sense in response to connection events. Examples:
    /// - TCP: send_data(output), wait_for_more()
    /// - HTTP: handle_response_header(), handle_response_body()
    /// - SSH: handle_auth_challenge(), handle_command_output()
    fn get_sync_actions(&self) -> Vec<ActionDefinition>;

    /// Execute a protocol-specific action
    ///
    /// # Arguments
    /// * `action` - The action JSON object from LLM
    ///
    /// # Returns
    /// * `Ok(ClientActionResult)` - Result of execution (data to send, disconnect, etc.)
    /// * `Err(_)` - If action execution failed
    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult>;

    /// Get protocol name for debugging
    fn protocol_name(&self) -> &'static str;

    /// Get the event types that this client can emit
    ///
    /// Each event type includes:
    /// - A unique ID (e.g., "http_response", "ssh_connected")
    /// - A description of when it occurs
    /// - The actions that can be used to respond to this event
    ///
    /// # Returns
    /// A vector of EventType definitions for this client
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
    /// - HTTP: ["http", "http client", "connect to http"]
    /// - SSH: ["ssh", "ssh client"]
    /// - Redis: ["redis", "redis client"]
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

    /// Get a short description of this protocol client
    ///
    /// This should be a concise, one-line description of what this client does.
    /// Examples:
    /// - HTTP: "HTTP client for making web requests"
    /// - SSH: "SSH client for remote shell access"
    /// - Redis: "Redis client for key-value operations"
    fn description(&self) -> &'static str;

    /// Get an example prompt that would trigger this client
    ///
    /// This should be a realistic, engaging example that demonstrates
    /// how a user would ask the LLM to start this client.
    /// Examples:
    /// - HTTP: "Connect to http://example.com:8080 and fetch /api/status every 10 seconds"
    /// - SSH: "SSH to user@server.com and monitor /var/log/syslog"
    /// - Redis: "Connect to Redis at localhost:6379 and monitor SET commands"
    fn example_prompt(&self) -> &'static str;

    /// Get the group name for categorizing this protocol
    ///
    /// Clients are grouped in documentation by category. Valid groups:
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
}
