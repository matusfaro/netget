//! Application state management

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::protocol::BaseStack;
use super::server::{ServerId, ServerInstance};

/// Operating mode for the application
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Server mode - one or more servers listening
    Server,
    /// Client mode - connecting to remote servers
    Client,
    /// Idle - not yet configured
    Idle,
}

impl Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Server => "Server",
            Self::Client => "Client",
            Self::Idle => "Idle",
        }
    }
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Global application state
#[derive(Clone)]
pub struct AppState {
    inner: Arc<RwLock<AppStateInner>>,
}

struct AppStateInner {
    /// Current operating mode
    mode: Mode,
    /// All server instances
    servers: HashMap<ServerId, ServerInstance>,
    /// Next server ID to assign
    next_server_id: u32,
    /// Current Ollama model
    ollama_model: String,
}

impl AppState {
    /// Create a new application state
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(AppStateInner {
                mode: Mode::Idle,
                servers: HashMap::new(),
                next_server_id: 1,
                ollama_model: "qwen3-coder:30b".to_string(),
            })),
        }
    }

    /// Get the current mode
    pub async fn get_mode(&self) -> Mode {
        self.inner.read().await.mode
    }

    /// Set the mode
    pub async fn set_mode(&self, mode: Mode) {
        self.inner.write().await.mode = mode;
    }

    /// Add a new server instance and return its ID
    pub async fn add_server(&self, mut server: ServerInstance) -> ServerId {
        let mut inner = self.inner.write().await;
        let id = ServerId::new(inner.next_server_id);
        inner.next_server_id += 1;
        server.id = id;
        inner.servers.insert(id, server);

        // Set mode to Server if this is the first server
        if inner.mode == Mode::Idle {
            inner.mode = Mode::Server;
        }

        id
    }

    /// Remove a server instance
    pub async fn remove_server(&self, id: ServerId) -> Option<ServerInstance> {
        let mut inner = self.inner.write().await;
        let server = inner.servers.remove(&id);

        // Set mode to Idle if no more servers
        if inner.servers.is_empty() {
            inner.mode = Mode::Idle;
        }

        server
    }

    /// Get a server instance (cloned)
    pub async fn get_server(&self, id: ServerId) -> Option<ServerInstance> {
        // Note: ServerInstance doesn't impl Clone because it contains JoinHandle
        // We'll need to provide specific access methods instead
        self.inner.read().await.servers.get(&id).map(|s| {
            // Create a lightweight copy without the handle
            ServerInstance {
                id: s.id,
                port: s.port,
                base_stack: s.base_stack,
                instruction: s.instruction.clone(),
                memory: s.memory.clone(),
                status: s.status.clone(),
                connections: s.connections.clone(),
                handle: None,
                created_at: s.created_at,
                status_changed_at: s.status_changed_at,
                local_addr: s.local_addr,
                #[cfg(feature = "proxy")]
                proxy_filter_config: s.proxy_filter_config.clone(),
            }
        })
    }

    /// Get all server IDs
    pub async fn get_all_server_ids(&self) -> Vec<ServerId> {
        self.inner.read().await.servers.keys().copied().collect()
    }

    /// Get all servers (lightweight copies without handles)
    pub async fn get_all_servers(&self) -> Vec<ServerInstance> {
        self.inner.read().await.servers.values().map(|s| {
            ServerInstance {
                id: s.id,
                port: s.port,
                base_stack: s.base_stack,
                instruction: s.instruction.clone(),
                memory: s.memory.clone(),
                status: s.status.clone(),
                connections: s.connections.clone(),
                handle: None,
                created_at: s.created_at,
                status_changed_at: s.status_changed_at,
                local_addr: s.local_addr,
                #[cfg(feature = "proxy")]
                proxy_filter_config: s.proxy_filter_config.clone(),
            }
        }).collect()
    }

    /// Update server status
    pub async fn update_server_status(&self, id: ServerId, status: super::server::ServerStatus) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&id) {
            server.status = status;
            server.status_changed_at = std::time::Instant::now();
        }
    }

    /// Get instruction for a specific server
    pub async fn get_instruction(&self, server_id: ServerId) -> Option<String> {
        self.inner.read().await.servers.get(&server_id).map(|s| s.instruction.clone())
    }

    /// Set instruction for a specific server
    pub async fn set_instruction(&self, server_id: ServerId, instruction: String) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            server.instruction = instruction;
        }
    }

    /// Get memory for a specific server
    pub async fn get_memory(&self, server_id: ServerId) -> Option<String> {
        self.inner.read().await.servers.get(&server_id).map(|s| s.memory.clone())
    }

    /// Set memory for a specific server
    pub async fn set_memory(&self, server_id: ServerId, memory: String) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            server.memory = memory;
        }
    }

    /// Append to memory for a specific server
    pub async fn append_memory(&self, server_id: ServerId, text: String) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            if !server.memory.is_empty() {
                server.memory.push('\n');
            }
            server.memory.push_str(&text);
        }
    }

    /// Get the Ollama model name
    pub async fn get_ollama_model(&self) -> String {
        self.inner.read().await.ollama_model.clone()
    }

    /// Set the Ollama model name
    pub async fn set_ollama_model(&self, model: String) {
        self.inner.write().await.ollama_model = model;
    }

    /// Get a summary of current state for LLM context
    pub async fn get_summary(&self) -> String {
        let inner = self.inner.read().await;
        let total_connections: usize = inner.servers.values().map(|s| s.connections.len()).sum();
        format!(
            "Mode: {}, Servers: {}, Total Connections: {}",
            inner.mode,
            inner.servers.len(),
            total_connections
        )
    }

    /// Get base stack for a server
    pub async fn get_base_stack(&self, server_id: ServerId) -> Option<BaseStack> {
        self.inner.read().await.servers.get(&server_id).map(|s| s.base_stack)
    }

    /// Cleanup old connections across all servers (connectionless protocols like UDP)
    pub async fn cleanup_old_connections(&self, max_age_secs: u64) {
        let mut inner = self.inner.write().await;
        for server in inner.servers.values_mut() {
            server.cleanup_old_connections(max_age_secs);
        }
    }

    /// Cleanup old closed connections (removes connections that have been closed for more than max_age_secs)
    pub async fn cleanup_closed_connections(&self, max_age_secs: u64) {
        use super::server::ConnectionStatus;
        let now = std::time::Instant::now();

        let mut inner = self.inner.write().await;
        for server in inner.servers.values_mut() {
            let to_remove: Vec<crate::network::connection::ConnectionId> = server.connections.iter()
                .filter(|(_, conn)| {
                    conn.status == ConnectionStatus::Closed &&
                    now.duration_since(conn.status_changed_at).as_secs() >= max_age_secs
                })
                .map(|(id, _)| *id)
                .collect();

            for id in to_remove {
                server.remove_connection(id);
            }
        }
    }

    /// Cleanup old stopped/error servers (removes servers that have been stopped/error for more than max_age_secs)
    pub async fn cleanup_old_servers(&self, max_age_secs: u64) {
        use super::server::ServerStatus;
        let now = std::time::Instant::now();

        let mut inner = self.inner.write().await;
        let to_remove: Vec<ServerId> = inner.servers.iter()
            .filter(|(_, server)| {
                // Remove if stopped/error and status changed more than max_age_secs ago
                matches!(server.status, ServerStatus::Stopped | ServerStatus::Error(_)) &&
                now.duration_since(server.status_changed_at).as_secs() >= max_age_secs
            })
            .map(|(id, _)| *id)
            .collect();

        for id in to_remove {
            inner.servers.remove(&id);
        }

        // Set mode to Idle if no more servers
        if inner.servers.is_empty() {
            inner.mode = Mode::Idle;
        }
    }

    /// Add a connection to a specific server
    pub async fn add_connection_to_server(&self, server_id: ServerId, connection: super::server::ConnectionState) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            server.add_connection(connection);
        }
    }

    /// Mark a connection as closed (instead of removing it immediately)
    pub async fn close_connection_on_server(&self, server_id: ServerId, connection_id: crate::network::connection::ConnectionId) {
        use super::server::ConnectionStatus;
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            if let Some(conn) = server.get_connection_mut(connection_id) {
                conn.status = ConnectionStatus::Closed;
                conn.status_changed_at = std::time::Instant::now();
            }
        }
    }

    /// Remove a connection from a specific server (used by cleanup task)
    pub async fn remove_connection_from_server(&self, server_id: ServerId, connection_id: crate::network::connection::ConnectionId) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            server.remove_connection(connection_id);
        }
    }

    // ========== Proxy Filter Configuration Methods ==========

    /// Get proxy filter configuration for a server
    #[cfg(feature = "proxy")]
    pub async fn get_proxy_filter_config(
        &self,
        server_id: ServerId,
    ) -> Option<crate::network::proxy_filter::ProxyFilterConfig> {
        self.inner.read().await.servers.get(&server_id)
            .and_then(|s| s.proxy_filter_config.clone())
    }

    /// Set proxy filter configuration for a server
    #[cfg(feature = "proxy")]
    pub async fn set_proxy_filter_config(
        &self,
        server_id: ServerId,
        config: crate::network::proxy_filter::ProxyFilterConfig,
    ) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            server.proxy_filter_config = Some(config);
        }
    }

    /// Update proxy filter configuration by merging with existing config
    #[cfg(feature = "proxy")]
    pub async fn update_proxy_filter_config(
        &self,
        server_id: ServerId,
        update_fn: impl FnOnce(&mut crate::network::proxy_filter::ProxyFilterConfig),
    ) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            if let Some(config) = &mut server.proxy_filter_config {
                update_fn(config);
            } else {
                // Initialize with default if not set
                let mut config = crate::network::proxy_filter::ProxyFilterConfig::default();
                update_fn(&mut config);
                server.proxy_filter_config = Some(config);
            }
        }
    }

    // ========== Backwards Compatibility Methods ==========
    // These methods help bridge old single-server code to new multi-server architecture
    // They typically operate on "the first server" or aggregate across all servers

    /// Get the first server's port (for backwards compat)
    pub async fn get_port(&self) -> Option<u16> {
        self.inner.read().await.servers.values().next().map(|s| s.port)
    }

    /// Get the first server's base stack (for backwards compat)
    pub async fn get_first_base_stack(&self) -> Option<BaseStack> {
        self.inner.read().await.servers.values().next().map(|s| s.base_stack)
    }

    /// Get the first server's instruction (for backwards compat)
    pub async fn get_first_instruction(&self) -> String {
        self.inner.read().await.servers.values().next()
            .map(|s| s.instruction.clone())
            .unwrap_or_default()
    }

    /// Get the first server's memory (for backwards compat)
    pub async fn get_first_memory(&self) -> String {
        self.inner.read().await.servers.values().next()
            .map(|s| s.memory.clone())
            .unwrap_or_default()
    }

    /// Get the first server's ID (for backwards compat)
    pub async fn get_first_server_id(&self) -> Option<ServerId> {
        self.inner.read().await.servers.keys().next().copied()
    }

    /// Get local address of first server (for backwards compat)
    pub async fn get_local_addr(&self) -> Option<std::net::SocketAddr> {
        self.inner.read().await.servers.values().next()
            .and_then(|s| s.local_addr)
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
