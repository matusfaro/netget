//! Protocol action trait
//!
//! This module defines the trait that all protocols must implement
//! to provide their own action systems.

use super::{ActionDefinition, ParameterDefinition};
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

    /// Custom protocol-specific result with structured data
    ///
    /// This is used when a protocol needs to return structured information
    /// that isn't just "send these bytes". For example, SSH auth decisions
    /// return {"allowed": true/false} which the protocol handler interprets.
    Custom {
        name: String,
        data: serde_json::Value,
    },

    /// MySQL query response with result set
    MysqlQueryResponse {
        columns: Vec<serde_json::Value>,
        rows: Vec<serde_json::Value>,
    },

    /// MySQL error response
    MysqlError { error_code: u16, message: String },

    /// MySQL OK response
    MysqlOk {
        affected_rows: u64,
        last_insert_id: u64,
    },

    /// IPP response
    IppResponse { status: u16, body: Vec<u8> },

    /// PostgreSQL query response with result set
    PostgresqlQueryResponse {
        columns: Vec<serde_json::Value>,
        rows: Vec<serde_json::Value>,
    },

    /// PostgreSQL error response
    PostgresqlError {
        severity: String,
        code: String,
        message: String,
    },

    /// PostgreSQL command complete response
    PostgresqlOk { tag: String },

    /// Redis simple string response ("+OK\r\n")
    RedisSimpleString { value: String },

    /// Redis bulk string response ("$5\r\nhello\r\n")
    RedisBulkString { value: Option<Vec<u8>> },

    /// Redis array response
    RedisArray { values: Vec<serde_json::Value> },

    /// Redis integer response (":42\r\n")
    RedisInteger { value: i64 },

    /// Redis error response ("-ERR message\r\n")
    RedisError { message: String },

    /// Redis null response ("$-1\r\n")
    RedisNull,
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
/// 1. Startup parameters - configuration accepted when opening server
/// 2. Async actions - executable anytime from user input
/// 3. Sync actions - executable during network events
/// 4. Action executor - parses and executes protocol actions
pub trait ProtocolActions: Send + Sync {
    /// Get startup parameters that can be provided when opening a server
    ///
    /// These parameters configure the protocol before it starts accepting
    /// connections. Examples:
    /// - Proxy: certificate_mode, request_filter_mode, response_filter_mode
    /// - SSH: host_key_path, banner_message
    /// - SNMP: community_string, allowed_oids
    ///
    /// Default implementation returns empty vector (no startup parameters).
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        Vec::new()
    }

    /// Get async actions that can be executed anytime from user input
    ///
    /// These actions don't require network context. Examples:
    /// - TCP: close_connection(id), send_to_connection(id, data)
    /// - SNMP: send_trap(target, variables)
    /// - IRC: broadcast(message)
    fn get_async_actions(&self, state: &AppState) -> Vec<ActionDefinition>;

    /// Get sync actions available during network events
    ///
    /// These actions only make sense in response to network events. Examples:
    /// - TCP: send_tcp_data(output), wait_for_more()
    /// - HTTP: send_http_response(status, headers, body)
    /// - SNMP: send_snmp_response(variables)
    fn get_sync_actions(&self) -> Vec<ActionDefinition>;

    /// Execute a protocol-specific action
    ///
    /// # Arguments
    /// * `action` - The action JSON object from LLM
    ///
    /// # Returns
    /// * `Ok(ActionResult)` - Result of execution (data to send, close connection, etc.)
    /// * `Err(_)` - If action execution failed
    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult>;

    /// Get protocol name for debugging
    fn protocol_name(&self) -> &'static str;
}
