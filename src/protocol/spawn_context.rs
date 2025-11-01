//! Server spawn context
//!
//! Provides all the necessary context for spawning a protocol server.

use crate::llm::OllamaClient;
use crate::state::app_state::AppState;
use crate::state::ServerId;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Context passed to protocol servers during spawning
///
/// Contains all the dependencies and configuration needed to start a server.
#[derive(Clone)]
pub struct SpawnContext {
    /// Address to listen on (may be 0.0.0.0:0 for dynamic port assignment)
    pub listen_addr: SocketAddr,

    /// LLM client for generating responses
    pub llm_client: OllamaClient,

    /// Application state
    pub state: Arc<AppState>,

    /// Channel for sending status updates to UI
    pub status_tx: mpsc::UnboundedSender<String>,

    /// Unique identifier for this server instance
    pub server_id: ServerId,

    /// Optional startup parameters specific to the protocol
    ///
    /// Protocols can deserialize this JSON to extract their custom configuration.
    /// For example:
    /// - HTTP Proxy: certificate_mode, request_filter_mode
    /// - SSH: host_key_path, banner_message
    /// - SNMP: community_string, allowed_oids
    pub startup_params: Option<serde_json::Value>,
}

impl SpawnContext {
    /// Create a new spawn context
    pub fn new(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: ServerId,
    ) -> Self {
        Self {
            listen_addr,
            llm_client,
            state,
            status_tx,
            server_id,
            startup_params: None,
        }
    }

    /// Create spawn context with startup parameters
    pub fn with_params(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: ServerId,
        startup_params: serde_json::Value,
    ) -> Self {
        Self {
            listen_addr,
            llm_client,
            state,
            status_tx,
            server_id,
            startup_params: Some(startup_params),
        }
    }
}
