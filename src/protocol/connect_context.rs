//! Client connect context
//!
//! Provides all the necessary context for connecting a protocol client.

use crate::llm::OllamaClient;
use crate::protocol::StartupParams;
use crate::state::app_state::AppState;
use crate::state::ClientId;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Context passed to protocol clients during connection
///
/// Contains all the dependencies and configuration needed to connect to a remote server.
#[derive(Clone)]
pub struct ConnectContext {
    /// Remote server address (hostname:port or IP:port)
    /// Example: "example.com:80" or "192.168.1.1:6379"
    pub remote_addr: String,

    /// LLM client for generating requests and interpreting responses
    pub llm_client: OllamaClient,

    /// Application state
    pub state: Arc<AppState>,

    /// Channel for sending status updates to UI
    pub status_tx: mpsc::UnboundedSender<String>,

    /// Unique identifier for this client instance
    pub client_id: ClientId,

    /// Optional type-safe startup parameters specific to the protocol
    ///
    /// Parameters can only be accessed if they were declared in the protocol's
    /// `get_startup_parameters()` implementation. Attempting to access undeclared
    /// parameters will panic at runtime.
    ///
    /// For example:
    /// - HTTP Client: request_headers, user_agent, follow_redirects
    /// - SSH Client: username, password, private_key_path
    /// - MySQL Client: username, password, database
    pub startup_params: Option<StartupParams>,
}

impl ConnectContext {
    /// Create a new connect context
    pub fn new(
        remote_addr: String,
        llm_client: OllamaClient,
        state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Self {
        Self {
            remote_addr,
            llm_client,
            state,
            status_tx,
            client_id,
            startup_params: None,
        }
    }

    /// Create connect context with startup parameters
    pub fn with_params(
        remote_addr: String,
        llm_client: OllamaClient,
        state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: StartupParams,
    ) -> Self {
        Self {
            remote_addr,
            llm_client,
            state,
            status_tx,
            client_id,
            startup_params: Some(startup_params),
        }
    }
}
