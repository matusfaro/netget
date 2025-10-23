//! Application state management

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::network::connection::ConnectionId;
use crate::protocol::BaseStack;

/// Operating mode for the application
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Server mode - listening for incoming connections
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
    /// Base protocol stack
    base_stack: BaseStack,
    /// Server port (for server mode)
    port: Option<u16>,
    /// Whether to send data first on connection (for server mode)
    send_first: bool,
    /// Local listening address (for server mode)
    local_addr: Option<SocketAddr>,
    /// Active connections
    connections: HashMap<ConnectionId, ConnectionInfo>,
    /// Current instruction for the LLM
    instruction: String,
    /// LLM memory (persistent context across requests)
    memory: String,
    /// Current Ollama model
    ollama_model: String,
}

/// Information about a connection
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub id: ConnectionId,
    pub remote_addr: SocketAddr,
    pub local_addr: SocketAddr,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_sent: u64,
    pub packets_received: u64,
}

impl AppState {
    /// Create a new application state
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(AppStateInner {
                mode: Mode::Idle,
                base_stack: BaseStack::TcpRaw,
                port: None,
                send_first: false,
                local_addr: None,
                connections: HashMap::new(),
                instruction: String::new(),
                memory: String::new(),
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

    /// Get the current base stack
    pub async fn get_base_stack(&self) -> BaseStack {
        self.inner.read().await.base_stack
    }

    /// Set the base stack
    pub async fn set_base_stack(&self, base_stack: BaseStack) {
        self.inner.write().await.base_stack = base_stack;
    }

    /// Get the server port
    pub async fn get_port(&self) -> Option<u16> {
        self.inner.read().await.port
    }

    /// Set the server port
    pub async fn set_port(&self, port: u16) {
        self.inner.write().await.port = Some(port);
    }

    /// Get whether to send data first on connection
    pub async fn get_send_first(&self) -> bool {
        self.inner.read().await.send_first
    }

    /// Set whether to send data first on connection
    pub async fn set_send_first(&self, send_first: bool) {
        self.inner.write().await.send_first = send_first;
    }

    /// Get the local listening address
    pub async fn get_local_addr(&self) -> Option<SocketAddr> {
        self.inner.read().await.local_addr
    }

    /// Set the local listening address
    pub async fn set_local_addr(&self, addr: Option<SocketAddr>) {
        self.inner.write().await.local_addr = addr;
    }

    /// Add a new connection
    pub async fn add_connection(&self, info: ConnectionInfo) {
        self.inner.write().await.connections.insert(info.id, info);
    }

    /// Remove a connection
    pub async fn remove_connection(&self, id: ConnectionId) {
        self.inner.write().await.connections.remove(&id);
    }

    /// Get connection info
    pub async fn get_connection(&self, id: ConnectionId) -> Option<ConnectionInfo> {
        self.inner.read().await.connections.get(&id).cloned()
    }

    /// Get all connections
    pub async fn get_all_connections(&self) -> Vec<ConnectionInfo> {
        self.inner.read().await.connections.values().cloned().collect()
    }

    /// Update connection stats
    pub async fn update_connection_stats(
        &self,
        id: ConnectionId,
        bytes_sent: u64,
        bytes_received: u64,
        packets_sent: u64,
        packets_received: u64,
    ) {
        if let Some(conn) = self.inner.write().await.connections.get_mut(&id) {
            conn.bytes_sent += bytes_sent;
            conn.bytes_received += bytes_received;
            conn.packets_sent += packets_sent;
            conn.packets_received += packets_received;
        }
    }

    /// Set the current instruction for the LLM
    pub async fn set_instruction(&self, instruction: String) {
        self.inner.write().await.instruction = instruction;
    }

    /// Get the current instruction
    pub async fn get_instruction(&self) -> String {
        self.inner.read().await.instruction.clone()
    }

    /// Set the LLM memory
    pub async fn set_memory(&self, memory: String) {
        self.inner.write().await.memory = memory;
    }

    /// Append to the LLM memory
    pub async fn append_memory(&self, text: String) {
        let mut inner = self.inner.write().await;
        if !inner.memory.is_empty() {
            inner.memory.push('\n');
        }
        inner.memory.push_str(&text);
    }

    /// Get the LLM memory
    pub async fn get_memory(&self) -> String {
        self.inner.read().await.memory.clone()
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
        format!(
            "Mode: {}, Stack: {}, Connections: {}, Local: {}",
            inner.mode,
            inner.base_stack,
            inner.connections.len(),
            inner.local_addr.map(|a| a.to_string()).unwrap_or_else(|| "None".to_string())
        )
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
