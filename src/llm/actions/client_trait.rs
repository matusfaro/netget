//! Client protocol trait
//!
//! This module defines the trait that all client protocols must implement
//! to provide their own action systems.

use super::protocol_trait::Protocol;
use super::ActionDefinition;
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
/// Each client protocol implements both the Protocol trait (for common functionality)
/// and this Client trait (for client-specific functionality like connecting).
///
/// The Client trait provides:
/// 1. Client connection - how to connect to a remote server
/// 2. Action executor - parses and executes client actions
pub trait Client: Protocol {
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

    /// Execute a protocol-specific action
    ///
    /// # Arguments
    /// * `action` - The action JSON object from LLM
    ///
    /// # Returns
    /// * `Ok(ClientActionResult)` - Result of execution (data to send, disconnect, etc.)
    /// * `Err(_)` - If action execution failed
    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult>;
}
