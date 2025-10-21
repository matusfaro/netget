//! Application state management

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::network::connection::ConnectionId;
use crate::protocol::{BaseStack, ProtocolType};

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
    /// Current protocol type (only relevant for TcpRaw stack)
    protocol_type: ProtocolType,
    /// Local listening address (for server mode)
    local_addr: Option<SocketAddr>,
    /// Active connections
    connections: HashMap<ConnectionId, ConnectionInfo>,
    /// User instructions history
    instructions: Vec<String>,
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
                protocol_type: ProtocolType::Custom,
                local_addr: None,
                connections: HashMap::new(),
                instructions: Vec::new(),
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

    /// Get the current protocol type
    pub async fn get_protocol_type(&self) -> ProtocolType {
        self.inner.read().await.protocol_type
    }

    /// Set the protocol type
    pub async fn set_protocol_type(&self, protocol_type: ProtocolType) {
        self.inner.write().await.protocol_type = protocol_type;
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

    /// Add a user instruction to history
    pub async fn add_instruction(&self, instruction: String) {
        self.inner.write().await.instructions.push(instruction);
    }

    /// Get all instructions
    pub async fn get_instructions(&self) -> Vec<String> {
        self.inner.read().await.instructions.clone()
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
        let protocol_info = match inner.base_stack {
            BaseStack::TcpRaw => format!("Stack: {}, Protocol: {}", inner.base_stack, inner.protocol_type),
            BaseStack::Http => format!("Stack: {}", inner.base_stack),
            BaseStack::DataLink => format!("Stack: {}", inner.base_stack),
        };
        format!(
            "Mode: {}, {}, Connections: {}, Local: {}",
            inner.mode,
            protocol_info,
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
