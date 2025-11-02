//! Application state management

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, RwLock};

use super::server::{ServerId, ServerInstance};
use super::task::{ScheduledTask, TaskId};
use crate::server::connection::ConnectionId;

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
    /// Next server ID to assign
    next_server_id: u32,
    /// Current Ollama model
    ollama_model: String,
    /// Available scripting environments (Python, Node.js)
    scripting_env: crate::scripting::ScriptingEnvironment,
    /// Currently selected scripting mode (LLM, Python, or JavaScript)
    selected_scripting_mode: ScriptingMode,
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
    /// Task name to ID mapping (for user-friendly task_id strings)
    task_names: HashMap<String, TaskId>,
    /// System capabilities detected at startup
    system_capabilities: crate::privilege::SystemCapabilities,
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

        // Generate unique instance ID: process_id + timestamp + random bytes
        let instance_id = Self::generate_instance_id();

        Self {
            inner: Arc::new(RwLock::new(AppStateInner {
                mode: Mode::Idle,
                servers: HashMap::new(),
                next_server_id: 1,
                ollama_model: "qwen3-coder:30b".to_string(),
                scripting_env,
                selected_scripting_mode,
                web_search_mode: WebSearchMode::On, // Default to enabled
                web_approval_tx: None, // Will be set by TUI
                include_disabled_protocols,
                ollama_lock_enabled,
                instance_id,
                tasks: HashMap::new(),
                next_task_id: 1,
                task_names: HashMap::new(),
                system_capabilities,
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
                script_config: s.script_config.clone(),
                #[cfg(feature = "proxy")]
                proxy_filter_config: s.proxy_filter_config.clone(),
                #[cfg(feature = "socks5")]
                socks5_filter_config: s.socks5_filter_config.clone(),
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
                script_config: s.script_config.clone(),
                #[cfg(feature = "proxy")]
                proxy_filter_config: s.proxy_filter_config.clone(),
                #[cfg(feature = "socks5")]
                socks5_filter_config: s.socks5_filter_config.clone(),
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

    /// Update script configuration for a server
    pub async fn set_script_config(
        &self,
        server_id: ServerId,
        config: Option<crate::scripting::ScriptConfig>,
    ) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            server.script_config = config;
        }
    }

    /// Get script configuration for a server
    pub async fn get_script_config(
        &self,
        server_id: ServerId,
    ) -> Option<crate::scripting::ScriptConfig> {
        self.inner.read().await
            .servers.get(&server_id)
            .and_then(|s| s.script_config.clone())
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
                // Remove if stopped/error and status changed more than max_age_secs ago
                matches!(
                    server.status,
                    ServerStatus::Stopped | ServerStatus::Error(_)
                ) && now.duration_since(server.status_changed_at).as_secs() >= max_age_secs
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
            .and_then(|s| s.proxy_filter_config.clone())
    }

    /// Set proxy filter configuration for a server
    #[cfg(feature = "proxy")]
    pub async fn set_proxy_filter_config(
        &self,
        server_id: ServerId,
        config: crate::server::proxy::filter::ProxyFilterConfig,
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
        update_fn: impl FnOnce(&mut crate::server::proxy::filter::ProxyFilterConfig),
    ) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            if let Some(config) = &mut server.proxy_filter_config {
                update_fn(config);
            } else {
                // Initialize with default if not set
                let mut config = crate::server::proxy::filter::ProxyFilterConfig::default();
                update_fn(&mut config);
                server.proxy_filter_config = Some(config);
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
            .and_then(|s| s.socks5_filter_config.clone())
    }

    /// Set SOCKS5 filter configuration for a server
    #[cfg(feature = "socks5")]
    pub async fn set_socks5_filter_config(
        &self,
        server_id: ServerId,
        config: crate::server::socks5::filter::Socks5FilterConfig,
    ) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            server.socks5_filter_config = Some(config);
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
                if let crate::state::server::ProtocolConnectionInfo::Socks5 {
                    target_addr: ref mut addr,
                    username: ref mut user,
                    ..
                } = conn.protocol_info
                {
                    *addr = target_addr;
                    *user = username;
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
                if let crate::state::server::ProtocolConnectionInfo::Imap {
                    session_state: ref mut state,
                    ..
                } = conn.protocol_info
                {
                    *state = session_state;
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
                if let crate::state::server::ProtocolConnectionInfo::Imap {
                    session_state: ref mut state,
                    authenticated_user: ref mut user,
                    selected_mailbox: ref mut mailbox,
                    mailbox_read_only: ref mut readonly,
                    ..
                } = conn.protocol_info
                {
                    if let Some(s) = session_state {
                        *state = s;
                    }
                    if let Some(u) = authenticated_user {
                        *user = u;
                    }
                    if let Some(m) = selected_mailbox {
                        *mailbox = m;
                    }
                    if let Some(r) = mailbox_read_only {
                        *readonly = r;
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
                if let crate::state::server::ProtocolConnectionInfo::Imap {
                    session_state,
                    authenticated_user,
                    selected_mailbox,
                    ..
                } = &conn.protocol_info
                {
                    return Some((
                        session_state.clone(),
                        authenticated_user.clone(),
                        selected_mailbox.clone(),
                    ));
                }
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
                if let crate::state::server::ProtocolConnectionInfo::Imap {
                    state: ref mut pstate,
                    ..
                } = conn.protocol_info
                {
                    *pstate = protocol_state;
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
                if let crate::state::server::ProtocolConnectionInfo::Vnc { authenticated: auth, username: uname, .. } = &mut conn.protocol_info {
                    *auth = authenticated;
                    *uname = username;
                }
            }
        }
    }

    /// Get VNC write half for sending framebuffer updates
    pub async fn get_vnc_write_half(
        &self,
        connection_id: ConnectionId,
    ) -> Option<std::sync::Arc<tokio::sync::Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>> {
        let inner = self.inner.read().await;
        for server in inner.servers.values() {
            if let Some(conn) = server.connections.get(&connection_id) {
                if let crate::state::server::ProtocolConnectionInfo::Vnc { write_half, .. } = &conn.protocol_info {
                    return Some(write_half.clone());
                }
            }
        }
        None
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
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
