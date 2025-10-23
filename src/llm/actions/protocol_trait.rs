//! Protocol action trait
//!
//! This module defines the trait that all protocols must implement
//! to provide their own action systems.

use super::{ActionDefinition, context::NetworkContext};
use crate::state::app_state::AppState;
use anyhow::Result;

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

/// Trait for protocol-specific action systems
///
/// Each protocol implements this trait to provide:
/// 1. Async actions - executable anytime from user input
/// 2. Sync actions - executable during network events with context
/// 3. Action executor - parses and executes protocol actions
pub trait ProtocolActions: Send + Sync {
    /// Get async actions that can be executed anytime from user input
    ///
    /// These actions don't require network context. Examples:
    /// - TCP: close_connection(id), send_to_connection(id, data)
    /// - SNMP: send_trap(target, variables)
    /// - IRC: broadcast(message)
    fn get_async_actions(&self, state: &AppState) -> Vec<ActionDefinition>;

    /// Get sync actions that require network event context
    ///
    /// These actions only make sense in response to network events. Examples:
    /// - TCP: send_tcp_data(output), wait_for_more()
    /// - HTTP: send_http_response(status, headers, body)
    /// - SNMP: send_snmp_response(variables)
    fn get_sync_actions(&self, context: &NetworkContext) -> Vec<ActionDefinition>;

    /// Execute a protocol-specific action
    ///
    /// # Arguments
    /// * `action` - The action JSON object from LLM
    /// * `context` - Optional network context (None for async actions)
    ///
    /// # Returns
    /// * `Ok(ActionResult)` - Result of execution (data to send, close connection, etc.)
    /// * `Err(_)` - If action execution failed
    fn execute_action(
        &self,
        action: serde_json::Value,
        context: Option<&NetworkContext>,
    ) -> Result<ActionResult>;

    /// Get protocol name for debugging
    fn protocol_name(&self) -> &'static str;
}
