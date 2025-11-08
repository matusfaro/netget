//! Application state management

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, RwLock};

use super::client::{ClientId, ClientInstance};
use super::server::{ServerId, ServerInstance};
use super::task::{ScheduledTask, TaskId};
use crate::server::connection::ConnectionId;

/// Source of an LLM conversation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversationSource {
    /// User input from the command line
    User,
    /// Network event from a protocol implementation
    Network { server_id: ServerId, connection_id: Option<ConnectionId> },
    /// Scheduled task execution
    Task { task_name: String },
    /// Scripting mode execution
    Scripting,
}

impl ConversationSource {
    pub fn display_label(&self) -> String {
        match self {
            ConversationSource::User => "[User]".to_string(),
            ConversationSource::Network { server_id, connection_id } => {
                if let Some(conn_id) = connection_id {
                    format!("[Net #{}:{}]", server_id.as_u32(), conn_id)
                } else {
                    format!("[Net #{}]", server_id.as_u32())
                }
            }
            ConversationSource::Task { task_name } => format!("[Task:{}]", task_name),
            ConversationSource::Scripting => "[Scripting]".to_string(),
        }
    }
}

/// Information about an active or recently-completed conversation
#[derive(Debug, Clone)]
pub struct ConversationInfo {
    /// Unique conversation ID
    pub id: String,
    /// Source of the conversation
    pub source: ConversationSource,
    /// Details text (truncated input, context, etc.)
    pub details: String,
    /// When the conversation started
    pub start_time: std::time::Instant,
    /// When the conversation ended (None if still active)
    pub end_time: Option<std::time::Instant>,
}

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

/// Selected scripting mode - which environment is active for the LLM
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptingMode {
    /// LLM chooses runtime for each script
    On,
    /// Scripting disabled - LLM only mode
    Off,
    /// Python scripting enabled
    Python,
    /// JavaScript scripting enabled
    JavaScript,
    /// Go scripting enabled
    Go,
    /// Perl scripting enabled
    Perl,
}

impl ScriptingMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::On => "On",
            Self::Off => "Off",
            Self::Python => "Python",
            Self::JavaScript => "JavaScript",
            Self::Go => "Go",
            Self::Perl => "Perl",
        }
    }
}

impl std::fmt::Display for ScriptingMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Web search mode - controls when web search is allowed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebSearchMode {
    /// Web search always enabled
    On,
    /// Web search always disabled
    Off,
    /// Ask user for approval before each search
    Ask,
}

impl WebSearchMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::Ask => "ask",
        }
    }
}

impl std::fmt::Display for WebSearchMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for WebSearchMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "on" | "enabled" | "enable" | "true" => Ok(Self::On),
            "off" | "disabled" | "disable" | "false" => Ok(Self::Off),
            "ask" => Ok(Self::Ask),
            _ => Err(format!("Invalid web search mode: {}", s)),
        }
    }
}

impl Default for WebSearchMode {
    fn default() -> Self {
        Self::On
    }
}

/// Event handler mode - controls how LLM should configure event handlers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventHandlerMode {
    /// LLM chooses handler types (script/static/llm) as appropriate
    Any,
    /// Force all events to use script handlers
    Script,
    /// Force all events to use static response handlers
    Static,
    /// Force all events to use LLM handlers
    Llm,
}

impl EventHandlerMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Any => "ANY",
            Self::Script => "SCRIPT",
            Self::Static => "STATIC",
            Self::Llm => "LLM",
        }
    }
}

impl std::fmt::Display for EventHandlerMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Default for EventHandlerMode {
    fn default() -> Self {
        Self::Any
    }
}

/// Request for web search approval (sent from tool executor to UI)
pub struct WebApprovalRequest {
    pub url: String,
    pub response_tx: oneshot::Sender<WebApprovalResponse>,
}

/// Response from user for web search approval
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebApprovalResponse {
    /// Allow this single request
    Allow,
    /// Deny this request
    Deny,
    /// Always allow (switch to ON mode for session)
    AlwaysAllow,
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
    /// All client instances
    clients: HashMap<ClientId, ClientInstance>,
    /// Next server ID to assign
    next_server_id: u32,
    /// Next client ID to assign
    next_client_id: u32,
    /// Current Ollama model
    ollama_model: String,
    /// Available scripting environments (Python, Node.js)
    scripting_env: crate::scripting::ScriptingEnvironment,
    /// Currently selected scripting mode (LLM, Python, or JavaScript)
    selected_scripting_mode: ScriptingMode,
    /// Event handler mode (controls how LLM configures event handlers)
    event_handler_mode: EventHandlerMode,
    /// Web search mode (On/Off/Ask)
    web_search_mode: WebSearchMode,
    /// Channel for sending web approval requests to UI
    web_approval_tx: Option<mpsc::UnboundedSender<WebApprovalRequest>>,
    /// Whether to include disabled protocols (for testing)
    include_disabled_protocols: bool,
    /// Whether Ollama API locking is enabled (for concurrent test execution)
    ollama_lock_enabled: bool,
    /// Unique instance ID for this NetGet process (for multi-instance isolation)
    instance_id: String,
    /// Scheduled tasks registry
    tasks: HashMap<TaskId, ScheduledTask>,
    /// Next task ID to assign
    next_task_id: u64,
    /// Conversation state for User Input Agent
    user_conversation_state: Option<Arc<std::sync::Mutex<crate::llm::ConversationState>>>,
    /// Task name to ID mapping (for user-friendly task_id strings)
    task_names: HashMap<String, TaskId>,
    /// System capabilities detected at startup
    system_capabilities: crate::privilege::SystemCapabilities,
    /// Active and recently-completed LLM conversations
    conversations: Vec<ConversationInfo>,
}

impl AppState {
    /// Create a new application state
    pub fn new() -> Self {
        Self::new_with_options(false, false)
    }

    /// Create a new application state with options
    pub fn new_with_options(include_disabled_protocols: bool, ollama_lock_enabled: bool) -> Self {
        // Detect scripting environments at startup
        let scripting_env = crate::scripting::ScriptingEnvironment::detect();

        // Detect system capabilities at startup
        let system_capabilities = crate::privilege::SystemCapabilities::detect();

        // Default to ON mode - LLM chooses runtime dynamically
        let selected_scripting_mode = ScriptingMode::On;
        let event_handler_mode = EventHandlerMode::default();

        // Generate unique instance ID: process_id + timestamp + random bytes
        let instance_id = Self::generate_instance_id();

        Self {
            inner: Arc::new(RwLock::new(AppStateInner {
                mode: Mode::Idle,
                servers: HashMap::new(),
                clients: HashMap::new(),
                next_server_id: 1,
                next_client_id: 1,
                ollama_model: "qwen3-coder:30b".to_string(),
                scripting_env,
                selected_scripting_mode,
                event_handler_mode,
                web_search_mode: WebSearchMode::On, // Default to enabled
                web_approval_tx: None, // Will be set by TUI
                include_disabled_protocols,
                ollama_lock_enabled,
                instance_id,
                tasks: HashMap::new(),
                next_task_id: 1,
                user_conversation_state: None,
                task_names: HashMap::new(),
                system_capabilities,
                conversations: Vec::new(),
            })),
        }
    }

    /// Generate a unique instance ID for this NetGet process
    /// Format: claude-{pid}-{timestamp}-{random4}
    fn generate_instance_id() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};

        let pid = std::process::id();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Generate 4 random hex characters for additional uniqueness
        let random_bytes: [u8; 2] = rand::random();
        let random_hex = hex::encode(&random_bytes);

        format!("claude-{}-{}-{}", pid, timestamp, random_hex)
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

        // Set mode to Idle if no more servers and no clients
        if inner.servers.is_empty() && inner.clients.is_empty() {
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
                protocol_name: s.protocol_name.clone(),
                instruction: s.instruction.clone(),
                memory: s.memory.clone(),
                status: s.status.clone(),
                connections: s.connections.clone(),
                handle: None,
                created_at: s.created_at,
                status_changed_at: s.status_changed_at,
                local_addr: s.local_addr,
                startup_params: s.startup_params.clone(),
                event_handler_config: s.event_handler_config.clone(),
                protocol_data: s.protocol_data.clone(),
                log_files: s.log_files.clone(),
            }
        })
    }

    /// Get all server IDs
    pub async fn get_all_server_ids(&self) -> Vec<ServerId> {
        self.inner.read().await.servers.keys().copied().collect()
    }

    /// Get all servers (lightweight copies without handles)
    pub async fn get_all_servers(&self) -> Vec<ServerInstance> {
        self.inner
            .read()
            .await
            .servers
            .values()
            .map(|s| ServerInstance {
                id: s.id,
                port: s.port,
                protocol_name: s.protocol_name.clone(),
                instruction: s.instruction.clone(),
                memory: s.memory.clone(),
                status: s.status.clone(),
                connections: s.connections.clone(),
                handle: None,
                created_at: s.created_at,
                status_changed_at: s.status_changed_at,
                local_addr: s.local_addr,
                startup_params: s.startup_params.clone(),
                event_handler_config: s.event_handler_config.clone(),
                protocol_data: s.protocol_data.clone(),
                log_files: s.log_files.clone(),
            })
            .collect()
    }

    /// Update server status
    pub async fn update_server_status(&self, id: ServerId, status: super::server::ServerStatus) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&id) {
            server.status = status;
            server.status_changed_at = std::time::Instant::now();
        }
    }

    /// Update server local listening address
    pub async fn update_server_local_addr(&self, id: ServerId, local_addr: std::net::SocketAddr) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&id) {
            server.local_addr = Some(local_addr);
        }
    }

    /// Get instruction for a specific server
    pub async fn get_instruction(&self, server_id: ServerId) -> Option<String> {
        self.inner
            .read()
            .await
            .servers
            .get(&server_id)
            .map(|s| s.instruction.clone())
    }

    /// Set instruction for a specific server
    pub async fn set_instruction(&self, server_id: ServerId, instruction: String) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            server.instruction = instruction;
        }
    }

    /// Get memory for a specific server
    pub async fn get_memory(&self, server_id: ServerId) -> Option<String> {
        self.inner
            .read()
            .await
            .servers
            .get(&server_id)
            .map(|s| s.memory.clone())
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

    /// Get the scripting environment information
    pub async fn get_scripting_env(&self) -> crate::scripting::ScriptingEnvironment {
        self.inner.read().await.scripting_env.clone()
    }

    /// Set scripting environment
    ///
    /// This is primarily used for testing to inject a mock environment.
    /// In production, the environment is auto-detected at startup.
    pub async fn set_scripting_env(&self, env: crate::scripting::ScriptingEnvironment) {
        self.inner.write().await.scripting_env = env;
    }

    /// Get the currently selected scripting mode
    pub async fn get_selected_scripting_mode(&self) -> ScriptingMode {
        self.inner.read().await.selected_scripting_mode
    }

    /// Set the selected scripting mode
    pub async fn set_selected_scripting_mode(&self, mode: ScriptingMode) {
        self.inner.write().await.selected_scripting_mode = mode;
    }

    /// Cycle between ON and OFF scripting modes
    /// Returns the new mode and whether any switch occurred
    pub async fn cycle_scripting_mode(&self) -> (ScriptingMode, bool) {
        let mut inner = self.inner.write().await;
        let current = inner.selected_scripting_mode;

        // Toggle between ON and OFF only
        // If currently on a specific language, treat as ON and toggle to OFF
        let next = match current {
            ScriptingMode::On | ScriptingMode::Python | ScriptingMode::JavaScript | ScriptingMode::Go | ScriptingMode::Perl => {
                ScriptingMode::Off
            }
            ScriptingMode::Off => {
                ScriptingMode::On
            }
        };

        inner.selected_scripting_mode = next;
        (next, true)
    }

    /// Get the currently selected event handler mode
    pub async fn get_event_handler_mode(&self) -> EventHandlerMode {
        self.inner.read().await.event_handler_mode
    }

    /// Set the event handler mode
    pub async fn set_event_handler_mode(&self, mode: EventHandlerMode) {
        self.inner.write().await.event_handler_mode = mode;
    }

    /// Cycle event handler mode through ANY -> SCRIPT -> STATIC -> LLM -> ANY
    /// Returns the new mode
    pub async fn cycle_event_handler_mode(&self) -> EventHandlerMode {
        let mut inner = self.inner.write().await;
        inner.event_handler_mode = match inner.event_handler_mode {
            EventHandlerMode::Any => EventHandlerMode::Script,
            EventHandlerMode::Script => EventHandlerMode::Static,
            EventHandlerMode::Static => EventHandlerMode::Llm,
            EventHandlerMode::Llm => EventHandlerMode::Any,
        };
        inner.event_handler_mode
    }

    /// Get the current web search mode
    pub async fn get_web_search_mode(&self) -> WebSearchMode {
        self.inner.read().await.web_search_mode
    }

    /// Set the web search mode
    pub async fn set_web_search_mode(&self, mode: WebSearchMode) {
        self.inner.write().await.web_search_mode = mode;
    }

    /// Cycle web search mode through ON -> ASK -> OFF -> ON and return the new state
    pub async fn cycle_web_search_mode(&self) -> WebSearchMode {
        let mut inner = self.inner.write().await;
        inner.web_search_mode = match inner.web_search_mode {
            WebSearchMode::On => WebSearchMode::Ask,
            WebSearchMode::Ask => WebSearchMode::Off,
            WebSearchMode::Off => WebSearchMode::On,
        };
        inner.web_search_mode
    }

    /// Set the web approval channel (called by TUI on startup)
    pub async fn set_web_approval_channel(&self, tx: mpsc::UnboundedSender<WebApprovalRequest>) {
        self.inner.write().await.web_approval_tx = Some(tx);
    }

    /// Get a clone of the web approval channel
    pub async fn get_web_approval_channel(&self) -> Option<mpsc::UnboundedSender<WebApprovalRequest>> {
        self.inner.read().await.web_approval_tx.clone()
    }

    /// Get whether disabled protocols should be included
    pub async fn get_include_disabled_protocols(&self) -> bool {
        self.inner.read().await.include_disabled_protocols
    }

    /// Get whether Ollama API locking is enabled
    pub async fn get_ollama_lock_enabled(&self) -> bool {
        self.inner.read().await.ollama_lock_enabled
    }

    /// Get the unique instance ID for this NetGet process
    pub async fn get_instance_id(&self) -> String {
        self.inner.read().await.instance_id.clone()
    }

    /// Get system capabilities detected at startup
    pub async fn get_system_capabilities(&self) -> crate::privilege::SystemCapabilities {
        self.inner.read().await.system_capabilities.clone()
    }

    /// Update event handler configuration for a server
    pub async fn set_event_handler_config(
        &self,
        server_id: ServerId,
        config: Option<crate::scripting::EventHandlerConfig>,
    ) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            server.event_handler_config = config;
        }
    }

    /// Get event handler configuration for a server
    pub async fn get_event_handler_config(
        &self,
        server_id: ServerId,
    ) -> Option<crate::scripting::EventHandlerConfig> {
        self.inner.read().await
            .servers.get(&server_id)
            .and_then(|s| s.event_handler_config.clone())
    }

    /// Get a summary of current state for LLM context
    pub async fn get_summary(&self) -> String {
        let inner = self.inner.read().await;
        let total_connections: usize = inner.servers.values().map(|s| s.connections.len()).sum();
        format!(
            "Mode: {}, Servers: {}, Clients: {}, Total Connections: {}",
            inner.mode,
            inner.servers.len(),
            inner.clients.len(),
            total_connections
        )
    }

    /// Get protocol name for a server
    pub async fn get_protocol_name(&self, server_id: ServerId) -> Option<String> {
        self.inner
            .read()
            .await
            .servers
            .get(&server_id)
            .map(|s| s.protocol_name.clone())
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
            let to_remove: Vec<crate::server::connection::ConnectionId> = server
                .connections
                .iter()
                .filter(|(_, conn)| {
                    conn.status == ConnectionStatus::Closed
                        && now.duration_since(conn.status_changed_at).as_secs() >= max_age_secs
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
        let to_remove: Vec<ServerId> = inner
            .servers
            .iter()
            .filter(|(_, server)| {
                let age = now.duration_since(server.status_changed_at).as_secs();
                // Remove if stopped and status changed more than max_age_secs ago
                // (Error servers are removed immediately on failure)
                matches!(server.status, ServerStatus::Stopped) && age >= max_age_secs
            })
            .map(|(id, _)| *id)
            .collect();

        for id in to_remove {
            inner.servers.remove(&id);
        }

        // Set mode to Idle if no more servers and no clients
        if inner.servers.is_empty() && inner.clients.is_empty() {
            inner.mode = Mode::Idle;
        }
    }

    /// Add a connection to a specific server
    pub async fn add_connection_to_server(
        &self,
        server_id: ServerId,
        connection: super::server::ConnectionState,
    ) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            server.add_connection(connection);
        }
    }

    /// Mark a connection as closed (instead of removing it immediately)
    pub async fn close_connection_on_server(
        &self,
        server_id: ServerId,
        connection_id: crate::server::connection::ConnectionId,
    ) {
        use super::server::ConnectionStatus;
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            if let Some(conn) = server.get_connection_mut(connection_id) {
                conn.status = ConnectionStatus::Closed;
                conn.status_changed_at = std::time::Instant::now();
            }
        }

        // Clean up any tasks associated with this connection
        self.cleanup_connection_tasks(server_id, connection_id).await;
    }

    /// Remove a connection from a specific server (used by cleanup task)
    pub async fn remove_connection_from_server(
        &self,
        server_id: ServerId,
        connection_id: crate::server::connection::ConnectionId,
    ) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            server.remove_connection(connection_id);
        }

        // Clean up any tasks associated with this connection (safety measure)
        // Tasks should already be cleaned up when connection was closed,
        // but this ensures no orphaned tasks remain
        self.cleanup_connection_tasks(server_id, connection_id).await;
    }

    // ========== Proxy Filter Configuration Methods ==========

    /// Get proxy filter configuration for a server
    #[cfg(feature = "proxy")]
    pub async fn get_proxy_filter_config(
        &self,
        server_id: ServerId,
    ) -> Option<crate::server::proxy::filter::ProxyFilterConfig> {
        self.inner
            .read()
            .await
            .servers
            .get(&server_id)
            .and_then(|s| {
                s.protocol_data
                    .get("proxy_filter_config")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
            })
    }

    /// Set proxy filter configuration for a server
    #[cfg(feature = "proxy")]
    pub async fn set_proxy_filter_config(
        &self,
        server_id: ServerId,
        config: crate::server::proxy::filter::ProxyFilterConfig,
    ) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            if let Ok(value) = serde_json::to_value(&config) {
                server.set_protocol_field("proxy_filter_config".to_string(), value);
            }
        }
    }

    /// Update proxy filter configuration by merging with existing config
    #[cfg(feature = "proxy")]
    pub async fn update_proxy_filter_config(
        &self,
        server_id: ServerId,
        update_fn: impl FnOnce(&mut crate::server::proxy::filter::ProxyFilterConfig),
    ) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            let mut config = server
                .protocol_data
                .get("proxy_filter_config")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_else(crate::server::proxy::filter::ProxyFilterConfig::default);

            update_fn(&mut config);

            if let Ok(value) = serde_json::to_value(&config) {
                server.set_protocol_field("proxy_filter_config".to_string(), value);
            }
        }
    }

    /// Get SOCKS5 filter configuration for a server
    #[cfg(feature = "socks5")]
    pub async fn get_socks5_filter_config(
        &self,
        server_id: ServerId,
    ) -> Option<crate::server::socks5::filter::Socks5FilterConfig> {
        self.inner
            .read()
            .await
            .servers
            .get(&server_id)
            .and_then(|s| {
                s.protocol_data
                    .get("socks5_filter_config")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
            })
    }

    /// Set SOCKS5 filter configuration for a server
    #[cfg(feature = "socks5")]
    pub async fn set_socks5_filter_config(
        &self,
        server_id: ServerId,
        config: crate::server::socks5::filter::Socks5FilterConfig,
    ) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            if let Ok(value) = serde_json::to_value(&config) {
                server.set_protocol_field("socks5_filter_config".to_string(), value);
            }
        }
    }

    /// Update SOCKS5 connection target address
    #[cfg(feature = "socks5")]
    pub async fn update_socks5_target(
        &self,
        server_id: ServerId,
        connection_id: ConnectionId,
        target_addr: Option<String>,
        username: Option<String>,
    ) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            if let Some(conn) = server.connections.get_mut(&connection_id) {
                // Update fields in the flexible storage
                if let Some(obj) = conn.protocol_info.data.as_object_mut() {
                    if let Some(target) = target_addr {
                        obj.insert("target_addr".to_string(), serde_json::Value::String(target));
                    } else {
                        obj.insert("target_addr".to_string(), serde_json::Value::Null);
                    }
                    if let Some(user) = username {
                        obj.insert("username".to_string(), serde_json::Value::String(user));
                    } else {
                        obj.insert("username".to_string(), serde_json::Value::Null);
                    }
                }
            }
        }
    }

    // ========== IMAP Connection State Methods ==========

    /// Update IMAP connection session state
    #[cfg(feature = "imap")]
    pub async fn update_imap_session_state(
        &self,
        server_id: ServerId,
        connection_id: ConnectionId,
        session_state: crate::state::server::ImapSessionState,
    ) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            if let Some(conn) = server.connections.get_mut(&connection_id) {
                if let Some(obj) = conn.protocol_info.data.as_object_mut() {
                    obj.insert("session_state".to_string(), serde_json::to_value(&session_state).unwrap_or(serde_json::Value::Null));
                }
            }
        }
    }

    /// Update IMAP connection full state (session, user, mailbox)
    #[cfg(feature = "imap")]
    pub async fn update_imap_connection_state(
        &self,
        server_id: ServerId,
        connection_id: ConnectionId,
        session_state: Option<crate::state::server::ImapSessionState>,
        authenticated_user: Option<Option<String>>,
        selected_mailbox: Option<Option<String>>,
        mailbox_read_only: Option<bool>,
    ) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            if let Some(conn) = server.connections.get_mut(&connection_id) {
                if let Some(obj) = conn.protocol_info.data.as_object_mut() {
                    if let Some(s) = session_state {
                        obj.insert("session_state".to_string(), serde_json::to_value(&s).unwrap_or(serde_json::Value::Null));
                    }
                    if let Some(u) = authenticated_user {
                        obj.insert("authenticated_user".to_string(), serde_json::to_value(&u).unwrap_or(serde_json::Value::Null));
                    }
                    if let Some(m) = selected_mailbox {
                        obj.insert("selected_mailbox".to_string(), serde_json::to_value(&m).unwrap_or(serde_json::Value::Null));
                    }
                    if let Some(r) = mailbox_read_only {
                        obj.insert("mailbox_read_only".to_string(), serde_json::Value::Bool(r));
                    }
                }
            }
        }
    }

    /// Get IMAP connection state
    #[cfg(feature = "imap")]
    pub async fn get_imap_connection_state(
        &self,
        server_id: ServerId,
        connection_id: ConnectionId,
    ) -> Option<(
        crate::state::server::ImapSessionState,
        Option<String>,
        Option<String>,
    )> {
        let inner = self.inner.read().await;
        if let Some(server) = inner.servers.get(&server_id) {
            if let Some(conn) = server.connections.get(&connection_id) {
                let session_state = conn.protocol_info.data.get("session_state")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())?;
                let authenticated_user = conn.protocol_info.data.get("authenticated_user")
                    .and_then(|v| serde_json::from_value(v.clone()).ok());
                let selected_mailbox = conn.protocol_info.data.get("selected_mailbox")
                    .and_then(|v| serde_json::from_value(v.clone()).ok());
                return Some((
                    session_state,
                    authenticated_user.flatten(),
                    selected_mailbox.flatten(),
                ));
            }
        }
        None
    }

    /// Update IMAP protocol state (Idle/Processing/Accumulating)
    #[cfg(feature = "imap")]
    pub async fn update_imap_protocol_state(
        &self,
        server_id: ServerId,
        connection_id: ConnectionId,
        protocol_state: crate::state::server::ProtocolState,
    ) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            if let Some(conn) = server.connections.get_mut(&connection_id) {
                if let Some(obj) = conn.protocol_info.data.as_object_mut() {
                    obj.insert("state".to_string(), serde_json::to_value(&protocol_state).unwrap_or(serde_json::Value::Null));
                }
            }
        }
    }

    /// Update connection stats (bytes/packets)
    pub async fn update_connection_stats(
        &self,
        server_id: ServerId,
        connection_id: ConnectionId,
        bytes_received: Option<u64>,
        bytes_sent: Option<u64>,
        packets_received: Option<u64>,
        packets_sent: Option<u64>,
    ) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            if let Some(conn) = server.connections.get_mut(&connection_id) {
                if let Some(br) = bytes_received {
                    conn.bytes_received += br;
                }
                if let Some(bs) = bytes_sent {
                    conn.bytes_sent += bs;
                }
                if let Some(pr) = packets_received {
                    conn.packets_received += pr;
                }
                if let Some(ps) = packets_sent {
                    conn.packets_sent += ps;
                }
                conn.last_activity = std::time::Instant::now();
            }
        }
    }

    /// Update connection status
    pub async fn update_connection_status(
        &self,
        server_id: ServerId,
        connection_id: ConnectionId,
        status: crate::state::server::ConnectionStatus,
    ) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            if let Some(conn) = server.connections.get_mut(&connection_id) {
                conn.status = status;
                conn.status_changed_at = std::time::Instant::now();
            }
        }
    }

    /// Update VNC connection authentication status
    pub async fn update_vnc_connection_auth(
        &self,
        server_id: ServerId,
        connection_id: ConnectionId,
        authenticated: bool,
        username: Option<String>,
    ) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            if let Some(conn) = server.connections.get_mut(&connection_id) {
                if let Some(obj) = conn.protocol_info.data.as_object_mut() {
                    obj.insert("authenticated".to_string(), serde_json::Value::Bool(authenticated));
                    obj.insert("username".to_string(), serde_json::to_value(&username).unwrap_or(serde_json::Value::Null));
                }
            }
        }
    }

    /// Update Bitcoin connection info
    pub async fn update_bitcoin_connection_info(
        &self,
        server_id: ServerId,
        connection_id: ConnectionId,
        last_message_type: String,
    ) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            if let Some(conn) = server.connections.get_mut(&connection_id) {
                if let Some(obj) = conn.protocol_info.data.as_object_mut() {
                    obj.insert("last_message_type".to_string(), serde_json::Value::String(last_message_type.clone()));
                    // Mark handshake complete if we've seen both version and verack
                    if last_message_type == "verack" {
                        obj.insert("handshake_complete".to_string(), serde_json::Value::Bool(true));
                    }
                }
            }
        }
    }

    /// Get VNC write half for sending framebuffer updates
    ///
    /// Note: This method is deprecated. Write halves are now managed locally
    /// within protocol modules and not stored in centralized state.
    pub async fn get_vnc_write_half(
        &self,
        _connection_id: ConnectionId,
    ) -> Option<std::sync::Arc<tokio::sync::Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>> {
        // Write halves are no longer stored in centralized state
        // Protocols manage their own local connection data for I/O
        None
    }

    // ========== Client Management Methods ==========

    /// Add a new client instance and return its ID
    pub async fn add_client(&self, mut client: ClientInstance) -> ClientId {
        let mut inner = self.inner.write().await;
        let id = ClientId::new(inner.next_client_id);
        inner.next_client_id += 1;
        client.id = id;
        inner.clients.insert(id, client);

        // Set mode to Client if this is the first client (and no servers)
        if inner.mode == Mode::Idle {
            inner.mode = Mode::Client;
        }

        id
    }

    /// Remove a client instance
    pub async fn remove_client(&self, id: ClientId) -> Option<ClientInstance> {
        let mut inner = self.inner.write().await;
        let client = inner.clients.remove(&id);

        // Set mode to Idle if no more clients and no servers
        if inner.clients.is_empty() && inner.servers.is_empty() {
            inner.mode = Mode::Idle;
        }

        client
    }

    /// Get a client instance (cloned)
    pub async fn get_client(&self, id: ClientId) -> Option<ClientInstance> {
        // Note: ClientInstance doesn't impl Clone because it contains JoinHandle
        // We'll need to provide specific access methods instead
        self.inner.read().await.clients.get(&id).map(|c| {
            // Create a lightweight copy without the handle
            ClientInstance {
                id: c.id,
                remote_addr: c.remote_addr.clone(),
                protocol_name: c.protocol_name.clone(),
                instruction: c.instruction.clone(),
                memory: c.memory.clone(),
                status: c.status.clone(),
                connection: c.connection.clone(),
                handle: None,
                created_at: c.created_at,
                status_changed_at: c.status_changed_at,
                startup_params: c.startup_params.clone(),
                event_handler_config: c.event_handler_config.clone(),
                protocol_data: c.protocol_data.clone(),
                log_files: c.log_files.clone(),
            }
        })
    }

    /// Get all client IDs
    pub async fn get_all_client_ids(&self) -> Vec<ClientId> {
        self.inner.read().await.clients.keys().copied().collect()
    }

    /// Get all clients (lightweight copies without handles)
    pub async fn get_all_clients(&self) -> Vec<ClientInstance> {
        self.inner
            .read()
            .await
            .clients
            .values()
            .map(|c| ClientInstance {
                id: c.id,
                remote_addr: c.remote_addr.clone(),
                protocol_name: c.protocol_name.clone(),
                instruction: c.instruction.clone(),
                memory: c.memory.clone(),
                status: c.status.clone(),
                connection: c.connection.clone(),
                handle: None,
                created_at: c.created_at,
                status_changed_at: c.status_changed_at,
                startup_params: c.startup_params.clone(),
                event_handler_config: c.event_handler_config.clone(),
                protocol_data: c.protocol_data.clone(),
                log_files: c.log_files.clone(),
            })
            .collect()
    }

    /// Update client status
    pub async fn update_client_status(&self, id: ClientId, status: super::client::ClientStatus) {
        if let Some(client) = self.inner.write().await.clients.get_mut(&id) {
            client.status = status;
            client.status_changed_at = std::time::Instant::now();
        }
    }

    /// Get instruction for a specific client
    pub async fn get_instruction_for_client(&self, client_id: ClientId) -> Option<String> {
        self.inner
            .read()
            .await
            .clients
            .get(&client_id)
            .map(|c| c.instruction.clone())
    }

    /// Set instruction for a specific client
    pub async fn set_instruction_for_client(&self, client_id: ClientId, instruction: String) {
        if let Some(client) = self.inner.write().await.clients.get_mut(&client_id) {
            client.instruction = instruction;
        }
    }

    /// Get memory for a specific client
    pub async fn get_memory_for_client(&self, client_id: ClientId) -> Option<String> {
        self.inner
            .read()
            .await
            .clients
            .get(&client_id)
            .map(|c| c.memory.clone())
    }

    /// Set memory for a specific client
    pub async fn set_memory_for_client(&self, client_id: ClientId, memory: String) {
        if let Some(client) = self.inner.write().await.clients.get_mut(&client_id) {
            client.memory = memory;
        }
    }

    /// Append to memory for a specific client
    pub async fn append_memory_for_client(&self, client_id: ClientId, text: String) {
        if let Some(client) = self.inner.write().await.clients.get_mut(&client_id) {
            if !client.memory.is_empty() {
                client.memory.push('\n');
            }
            client.memory.push_str(&text);
        }
    }

    /// Get protocol name for a client
    pub async fn get_protocol_name_for_client(&self, client_id: ClientId) -> Option<String> {
        self.inner
            .read()
            .await
            .clients
            .get(&client_id)
            .map(|c| c.protocol_name.clone())
    }

    /// Execute a closure with mutable access to a client
    pub async fn with_client_mut<F, R>(&self, client_id: ClientId, f: F) -> Option<R>
    where
        F: FnOnce(&mut ClientInstance) -> R,
    {
        self.inner
            .write()
            .await
            .clients
            .get_mut(&client_id)
            .map(f)
    }

    /// Update event handler configuration for a client
    pub async fn set_client_event_handler_config(
        &self,
        client_id: ClientId,
        config: Option<crate::scripting::EventHandlerConfig>,
    ) {
        if let Some(client) = self.inner.write().await.clients.get_mut(&client_id) {
            client.event_handler_config = config;
        }
    }

    /// Get event handler configuration for a client
    pub async fn get_client_event_handler_config(
        &self,
        client_id: ClientId,
    ) -> Option<crate::scripting::EventHandlerConfig> {
        self.inner
            .read()
            .await
            .clients
            .get(&client_id)
            .and_then(|c| c.event_handler_config.clone())
    }

    /// Cleanup old disconnected clients (removes clients that have been disconnected for more than max_age_secs)
    pub async fn cleanup_old_clients(&self, max_age_secs: u64) {
        use super::client::ClientStatus;
        let now = std::time::Instant::now();

        let mut inner = self.inner.write().await;
        let to_remove: Vec<ClientId> = inner
            .clients
            .iter()
            .filter(|(_, client)| {
                let age = now.duration_since(client.status_changed_at).as_secs();
                // Remove if disconnected and status changed more than max_age_secs ago
                matches!(client.status, ClientStatus::Disconnected) && age >= max_age_secs
            })
            .map(|(id, _)| *id)
            .collect();

        for id in to_remove {
            inner.clients.remove(&id);
        }

        // Set mode to Idle if no more clients and no servers
        if inner.clients.is_empty() && inner.servers.is_empty() {
            inner.mode = Mode::Idle;
        }
    }

    // ========== Backwards Compatibility Methods ==========
    // These methods help bridge old single-server code to new multi-server architecture
    // They typically operate on "the first server" or aggregate across all servers

    /// Get the first server's port (for backwards compat)
    pub async fn get_port(&self) -> Option<u16> {
        self.inner
            .read()
            .await
            .servers
            .values()
            .next()
            .map(|s| s.port)
    }

    /// Get the first server's protocol name (for backwards compat)
    pub async fn get_first_protocol_name(&self) -> Option<String> {
        self.inner
            .read()
            .await
            .servers
            .values()
            .next()
            .map(|s| s.protocol_name.clone())
    }

    /// Get the first server's instruction (for backwards compat)
    pub async fn get_first_instruction(&self) -> String {
        self.inner
            .read()
            .await
            .servers
            .values()
            .next()
            .map(|s| s.instruction.clone())
            .unwrap_or_default()
    }

    /// Get the first server's memory (for backwards compat)
    pub async fn get_first_memory(&self) -> String {
        self.inner
            .read()
            .await
            .servers
            .values()
            .next()
            .map(|s| s.memory.clone())
            .unwrap_or_default()
    }

    /// Get the first server's ID (for backwards compat)
    pub async fn get_first_server_id(&self) -> Option<ServerId> {
        self.inner.read().await.servers.keys().next().copied()
    }

    /// Get local address of first server (for backwards compat)
    pub async fn get_local_addr(&self) -> Option<std::net::SocketAddr> {
        self.inner
            .read()
            .await
            .servers
            .values()
            .next()
            .and_then(|s| s.local_addr)
    }

    /// Execute a closure with mutable access to a server
    pub async fn with_server_mut<F, R>(&self, server_id: ServerId, f: F) -> Option<R>
    where
        F: FnOnce(&mut ServerInstance) -> R,
    {
        self.inner
            .write()
            .await
            .servers
            .get_mut(&server_id)
            .map(f)
    }

    // ===== Task Management Methods =====

    /// Add a new scheduled task
    pub async fn add_task(&self, task: ScheduledTask) -> TaskId {
        let mut inner = self.inner.write().await;
        let id = TaskId::new(inner.next_task_id);
        inner.next_task_id += 1;

        inner.task_names.insert(task.name.clone(), id);
        inner.tasks.insert(id, task);
        id
    }

    /// Get a task by ID or name
    pub async fn get_task(&self, id_or_name: &str) -> Option<ScheduledTask> {
        let inner = self.inner.read().await;

        // Try parsing as numeric ID first
        if let Some(id) = TaskId::from_string(id_or_name) {
            return inner.tasks.get(&id).cloned();
        }

        // Try looking up by name
        if let Some(&id) = inner.task_names.get(id_or_name) {
            return inner.tasks.get(&id).cloned();
        }

        None
    }

    /// Remove a task
    pub async fn remove_task(&self, id: TaskId) -> Option<ScheduledTask> {
        let mut inner = self.inner.write().await;
        if let Some(task) = inner.tasks.remove(&id) {
            inner.task_names.remove(&task.name);
            Some(task)
        } else {
            None
        }
    }

    /// Get all tasks
    pub async fn get_all_tasks(&self) -> Vec<ScheduledTask> {
        self.inner.read().await.tasks.values().cloned().collect()
    }

    /// Get tasks for a specific server
    pub async fn get_server_tasks(&self, server_id: ServerId) -> Vec<ScheduledTask> {
        use crate::state::task::TaskScope;

        self.inner
            .read()
            .await
            .tasks
            .values()
            .filter(|task| matches!(&task.scope, TaskScope::Server(sid) if *sid == server_id))
            .cloned()
            .collect()
    }

    /// Get tasks for a specific connection
    pub async fn get_connection_tasks(
        &self,
        server_id: ServerId,
        connection_id: crate::server::connection::ConnectionId,
    ) -> Vec<ScheduledTask> {
        use crate::state::task::TaskScope;

        self.inner
            .read()
            .await
            .tasks
            .values()
            .filter(|task| {
                matches!(
                    &task.scope,
                    TaskScope::Connection(sid, cid) if *sid == server_id && *cid == connection_id
                )
            })
            .cloned()
            .collect()
    }

    /// Update task status
    pub async fn update_task_status(&self, id: TaskId, status: crate::state::task::TaskStatus) {
        if let Some(task) = self.inner.write().await.tasks.get_mut(&id) {
            task.status = status;
        }
    }

    /// Update task next execution time
    pub async fn update_task_next_execution(&self, id: TaskId, next: std::time::Instant) {
        if let Some(task) = self.inner.write().await.tasks.get_mut(&id) {
            task.next_execution = next;
        }
    }

    /// Record task execution result
    pub async fn record_task_execution(
        &self,
        id: TaskId,
        result: &crate::state::task::TaskExecutionResult,
    ) {
        use crate::state::task::TaskType;

        let mut inner = self.inner.write().await;
        if let Some(task) = inner.tasks.get_mut(&id) {
            if result.success {
                task.failure_count = 0;
                task.last_error = None;
            } else {
                task.failure_count += 1;
                task.last_error = result.error.clone();
            }

            // Update execution count for recurring tasks
            if let TaskType::Recurring {
                ref mut executions_count,
                ..
            } = task.task_type
            {
                *executions_count += 1;
            }
        }
    }

    /// Clean up tasks associated with a server
    pub async fn cleanup_server_tasks(&self, server_id: ServerId) {
        use crate::state::task::TaskScope;

        let mut inner = self.inner.write().await;

        // Collect task IDs to remove
        let task_ids_to_remove: Vec<TaskId> = inner
            .tasks
            .iter()
            .filter(|(_, task)| matches!(&task.scope, TaskScope::Server(sid) if *sid == server_id))
            .map(|(&id, _)| id)
            .collect();

        // Remove tasks and their name mappings
        for id in task_ids_to_remove {
            if let Some(task) = inner.tasks.remove(&id) {
                inner.task_names.remove(&task.name);
            }
        }
    }

    /// Clean up tasks associated with a connection
    pub async fn cleanup_connection_tasks(
        &self,
        server_id: ServerId,
        connection_id: crate::server::connection::ConnectionId,
    ) {
        use crate::state::task::TaskScope;

        let mut inner = self.inner.write().await;

        // Collect task IDs to remove
        let task_ids_to_remove: Vec<TaskId> = inner
            .tasks
            .iter()
            .filter(|(_, task)| {
                matches!(
                    &task.scope,
                    TaskScope::Connection(sid, cid) if *sid == server_id && *cid == connection_id
                )
            })
            .map(|(&id, _)| id)
            .collect();

        // Remove tasks and their name mappings
        for id in task_ids_to_remove {
            if let Some(task) = inner.tasks.remove(&id) {
                inner.task_names.remove(&task.name);
            }
        }
    }

    /// Clean up tasks associated with a client
    pub async fn cleanup_client_tasks(&self, client_id: ClientId) {
        use crate::state::task::TaskScope;

        let mut inner = self.inner.write().await;

        // Collect task IDs to remove
        let task_ids_to_remove: Vec<TaskId> = inner
            .tasks
            .iter()
            .filter(|(_, task)| matches!(&task.scope, TaskScope::Client(cid) if *cid == client_id))
            .map(|(&id, _)| id)
            .collect();

        // Remove tasks and their name mappings
        for id in task_ids_to_remove {
            if let Some(task) = inner.tasks.remove(&id) {
                inner.task_names.remove(&task.name);
            }
        }
    }

    // ===== Conversation Management Methods =====

    /// Register a new conversation
    pub async fn register_conversation(
        &self,
        id: String,
        source: ConversationSource,
        details: String,
    ) {
        let info = ConversationInfo {
            id,
            source,
            details,
            start_time: std::time::Instant::now(),
            end_time: None,
        };

        self.inner.write().await.conversations.push(info);
    }

    /// Mark a conversation as ended
    pub async fn end_conversation(&self, id: &str) {
        let mut inner = self.inner.write().await;
        if let Some(conv) = inner.conversations.iter_mut().find(|c| c.id == id) {
            conv.end_time = Some(std::time::Instant::now());
        }
    }

    /// Get all active conversations and recently-completed ones (within 1 second)
    pub async fn get_active_conversations(&self) -> Vec<ConversationInfo> {
        let inner = self.inner.read().await;
        let now = std::time::Instant::now();

        inner
            .conversations
            .iter()
            .filter(|conv| {
                // Include if still active (no end_time)
                conv.end_time.is_none() ||
                // Or if ended within the last 1 second
                conv.end_time.map(|end| now.duration_since(end).as_secs() < 1).unwrap_or(false)
            })
            .cloned()
            .collect()
    }

    /// Get conversations for the "Inputs" column (User + Global tasks)
    pub async fn get_input_conversations(&self) -> Vec<ConversationInfo> {
        let all_convs = self.get_active_conversations().await;
        all_convs
            .into_iter()
            .filter(|conv| {
                matches!(
                    &conv.source,
                    ConversationSource::User | ConversationSource::Scripting
                    // Global tasks would be ConversationSource::Task but we need to filter by scope
                )
            })
            .collect()
    }

    /// Clean up old completed conversations (older than 1 second)
    pub async fn cleanup_old_conversations(&self) {
        let mut inner = self.inner.write().await;
        let now = std::time::Instant::now();

        inner.conversations.retain(|conv| {
            // Keep if still active
            conv.end_time.is_none() ||
            // Or if ended less than 1 second ago
            conv.end_time.map(|end| now.duration_since(end).as_secs() < 1).unwrap_or(false)
        });
    }

    /// Get or create the user conversation state
    pub async fn get_or_create_user_conversation_state(&self) -> Arc<std::sync::Mutex<crate::llm::ConversationState>> {
        let mut inner = self.inner.write().await;

        if inner.user_conversation_state.is_none() {
            // Create with default 50k character limit for conversation history
            let conversation_state = Arc::new(std::sync::Mutex::new(
                crate::llm::ConversationState::new(50000)
            ));
            inner.user_conversation_state = Some(conversation_state.clone());
        }

        inner.user_conversation_state.as_ref().unwrap().clone()
    }

    /// Clear user conversation history
    pub async fn clear_user_conversation_history(&self) {
        let inner = self.inner.read().await;
        if let Some(conversation_state) = &inner.user_conversation_state {
            if let Ok(mut state) = conversation_state.lock() {
                state.clear_history();
            }
        }
    }

    /// Get formatted conversation history for prompts
    pub async fn get_user_conversation_history(&self) -> Option<String> {
        let inner = self.inner.read().await;
        if let Some(conversation_state) = &inner.user_conversation_state {
            if let Ok(state) = conversation_state.lock() {
                if !state.is_empty() {
                    return Some(state.get_history_for_prompt());
                }
            }
        }
        None
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
