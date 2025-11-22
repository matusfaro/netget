//! Application state management

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, RwLock};

use super::client::{ClientId, ClientInstance};
use super::easy::{EasyId, EasyInstance, EasyStatus};
use super::server::{ServerId, ServerInstance};
use super::task::{ScheduledTask, TaskId};
use crate::server::connection::ConnectionId;

/// Source of an LLM conversation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversationSource {
    /// User input from the command line
    User,
    /// Network event from a protocol implementation
    Network {
        server_id: ServerId,
        connection_id: Option<ConnectionId>,
    },
    /// Scheduled task execution
    Task { task_name: String },
    /// Scripting mode execution
    Scripting,
}

impl ConversationSource {
    pub fn display_label(&self) -> String {
        match self {
            ConversationSource::User => "[User]".to_string(),
            ConversationSource::Network {
                server_id,
                connection_id,
            } => {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WebSearchMode {
    /// Web search always enabled
    #[default]
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

/// Event handler mode - controls how LLM should configure event handlers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EventHandlerMode {
    /// LLM chooses handler types (script/static/llm) as appropriate
    #[default]
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

impl std::str::FromStr for EventHandlerMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "any" => Ok(Self::Any),
            "script" => Ok(Self::Script),
            "static" => Ok(Self::Static),
            "llm" => Ok(Self::Llm),
            _ => Err(format!("Invalid event handler mode: {}", s)),
        }
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
    /// Tor client instances (for directory queries)
    #[cfg(feature = "tor")]
    tor_clients: HashMap<ClientId, Arc<arti_client::TorClient<tor_rtcompat::PreferredRuntime>>>,
    /// All easy protocol instances
    easy_instances: HashMap<EasyId, EasyInstance>,
    /// Mapping from underlying server ID to easy ID (for event routing)
    server_to_easy: HashMap<ServerId, EasyId>,
    /// Mapping from underlying client ID to easy ID (for event routing)
    client_to_easy: HashMap<ClientId, EasyId>,
    /// Unified ID counter for servers, connections, and clients
    /// This ensures all IDs are unique across all three types
    next_unified_id: u32,
    #[allow(dead_code)]
    /// Next server ID to assign (DEPRECATED - use next_unified_id)
    next_server_id: u32,
    #[allow(dead_code)]
    /// Next client ID to assign (DEPRECATED - use next_unified_id)
    next_client_id: u32,
    /// Current Ollama model (None = not yet selected/validated)
    ollama_model: Option<String>,
    /// Configured LLM client (with mock config, lock settings, etc.)
    llm_client: Option<crate::llm::OllamaClient>,
    /// Rate limiter for LLM calls (concurrency + token throttling)
    rate_limiter: crate::llm::RateLimiter,
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
    /// Ollama API base URL (default: http://localhost:11434)
    ollama_url: String,
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
    /// Total LLM input tokens (prompt tokens)
    total_input_tokens: u64,
    /// Total LLM output tokens (completion tokens)
    total_output_tokens: u64,
    /// Total number of LLM calls made
    total_llm_calls: u64,
    /// SQLite database manager
    _database_manager: crate::state::DatabaseManager,
}

impl AppState {
    /// Create a new application state
    pub fn new() -> Self {
        Self::new_with_options(false, false, "http://localhost:11434".to_string())
    }

    /// Create a new application state with options
    pub fn new_with_options(
        include_disabled_protocols: bool,
        ollama_lock_enabled: bool,
        ollama_url: String,
    ) -> Self {
        // Detect scripting environments at startup
        let scripting_env = crate::scripting::ScriptingEnvironment::detect();

        // Detect system capabilities at startup
        let system_capabilities = crate::privilege::SystemCapabilities::detect();

        // Default to ON mode - LLM chooses runtime dynamically
        let selected_scripting_mode = ScriptingMode::On;
        let event_handler_mode = EventHandlerMode::default();

        // Generate unique instance ID: process_id + timestamp + random bytes
        let instance_id = Self::generate_instance_id();

        // Create default rate limiter (will be configured from CLI args later)
        let rate_limiter = crate::llm::RateLimiter::new(crate::llm::RateLimiterConfig::default());

        Self {
            inner: Arc::new(RwLock::new(AppStateInner {
                mode: Mode::Idle,
                servers: HashMap::new(),
                clients: HashMap::new(),
                #[cfg(feature = "tor")]
                tor_clients: HashMap::new(),
                easy_instances: HashMap::new(),
                server_to_easy: HashMap::new(),
                client_to_easy: HashMap::new(),
                next_unified_id: 1,
                next_server_id: 1,
                next_client_id: 1,
                ollama_model: None,
                llm_client: None,
                rate_limiter,
                scripting_env,
                selected_scripting_mode,
                event_handler_mode,
                web_search_mode: WebSearchMode::On, // Default to enabled
                web_approval_tx: None,              // Will be set by TUI
                include_disabled_protocols,
                ollama_lock_enabled,
                ollama_url,
                instance_id,
                tasks: HashMap::new(),
                next_task_id: 1,
                user_conversation_state: None,
                task_names: HashMap::new(),
                system_capabilities,
                conversations: Vec::new(),
                total_input_tokens: 0,
                total_output_tokens: 0,
                total_llm_calls: 0,
                _database_manager: crate::state::DatabaseManager::new(),
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
        let random_hex = hex::encode(random_bytes);

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
        let id = ServerId::new(inner.next_unified_id);
        inner.next_unified_id += 1;
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
                feedback_instructions: s.feedback_instructions.clone(),
                feedback_buffer: s.feedback_buffer.clone(),
                last_feedback_processed: s.last_feedback_processed,
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
                feedback_instructions: s.feedback_instructions.clone(),
                feedback_buffer: s.feedback_buffer.clone(),
                last_feedback_processed: s.last_feedback_processed,
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

    /// Add feedback to a server's feedback buffer
    ///
    /// Feedback is accumulated and processed later via debounced LLM invocation
    /// Returns Ok if feedback was added, Err if server not found or feedback_instructions not set
    pub async fn add_server_feedback(
        &self,
        server_id: ServerId,
        feedback: serde_json::Value,
    ) -> anyhow::Result<()> {
        let mut inner = self.inner.write().await;
        if let Some(server) = inner.servers.get_mut(&server_id) {
            // Only accumulate feedback if feedback_instructions is set
            if server.feedback_instructions.is_some() {
                server.feedback_buffer.push(feedback);
                Ok(())
            } else {
                anyhow::bail!("Server {} has no feedback_instructions configured", server_id)
            }
        } else {
            anyhow::bail!("Server {} not found", server_id)
        }
    }

    /// Add feedback to a client's feedback buffer
    ///
    /// Feedback is accumulated and processed later via debounced LLM invocation
    /// Returns Ok if feedback was added, Err if client not found or feedback_instructions not set
    pub async fn add_client_feedback(
        &self,
        client_id: super::ClientId,
        feedback: serde_json::Value,
    ) -> anyhow::Result<()> {
        let mut inner = self.inner.write().await;
        if let Some(client) = inner.clients.get_mut(&client_id) {
            // Only accumulate feedback if feedback_instructions is set
            if client.feedback_instructions.is_some() {
                client.feedback_buffer.push(feedback);
                Ok(())
            } else {
                anyhow::bail!("Client {} has no feedback_instructions configured", client_id)
            }
        } else {
            anyhow::bail!("Client {} not found", client_id)
        }
    }

    /// Get first server ID (synchronous version for use in non-async contexts)
    ///
    /// This is a non-blocking helper that attempts to get the first server ID
    /// Returns None if unable to acquire lock or no servers exist
    pub fn get_first_server_id_sync(&self) -> Option<ServerId> {
        // Try non-blocking read
        self.inner.try_read().ok()?.servers.keys().next().copied()
    }

    /// Get the Ollama model name
    pub async fn get_ollama_model(&self) -> Option<String> {
        self.inner.read().await.ollama_model.clone()
    }

    /// Set the Ollama model name
    pub async fn set_ollama_model(&self, model: Option<String>) {
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
            ScriptingMode::On
            | ScriptingMode::Python
            | ScriptingMode::JavaScript
            | ScriptingMode::Go
            | ScriptingMode::Perl => ScriptingMode::Off,
            ScriptingMode::Off => ScriptingMode::On,
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
    pub async fn get_web_approval_channel(
        &self,
    ) -> Option<mpsc::UnboundedSender<WebApprovalRequest>> {
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

    /// Get the Ollama API base URL
    pub async fn get_ollama_url(&self) -> String {
        self.inner.read().await.ollama_url.clone()
    }

    /// Set the configured LLM client
    pub async fn set_llm_client(&self, client: crate::llm::OllamaClient) {
        self.inner.write().await.llm_client = Some(client);
    }

    /// Get the configured LLM client
    pub async fn get_llm_client(&self) -> Option<crate::llm::OllamaClient> {
        self.inner.read().await.llm_client.clone()
    }

    /// Get the rate limiter
    pub async fn get_rate_limiter(&self) -> crate::llm::RateLimiter {
        self.inner.read().await.rate_limiter.clone()
    }

    /// Configure the rate limiter
    pub async fn configure_rate_limiter(&self, config: crate::llm::RateLimiterConfig) -> anyhow::Result<()> {
        let rate_limiter = self.inner.read().await.rate_limiter.clone();
        rate_limiter.update_config(config).await
    }

    /// Get the unique instance ID for this NetGet process
    pub async fn get_instance_id(&self) -> String {
        self.inner.read().await.instance_id.clone()
    }

    /// Get system capabilities detected at startup
    pub async fn get_system_capabilities(&self) -> crate::privilege::SystemCapabilities {
        self.inner.read().await.system_capabilities.clone()
    }

    /// Get the next unified ID (for connections)
    /// This ensures all IDs (servers, connections, clients) are unique across all types
    pub async fn get_next_unified_id(&self) -> u32 {
        let mut inner = self.inner.write().await;
        let id = inner.next_unified_id;
        inner.next_unified_id += 1;
        id
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
        self.inner
            .read()
            .await
            .servers
            .get(&server_id)
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
        self.cleanup_connection_tasks(server_id, connection_id)
            .await;
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
        self.cleanup_connection_tasks(server_id, connection_id)
            .await;
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
                    obj.insert(
                        "session_state".to_string(),
                        serde_json::to_value(&session_state).unwrap_or(serde_json::Value::Null),
                    );
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
                        obj.insert(
                            "session_state".to_string(),
                            serde_json::to_value(&s).unwrap_or(serde_json::Value::Null),
                        );
                    }
                    if let Some(u) = authenticated_user {
                        obj.insert(
                            "authenticated_user".to_string(),
                            serde_json::to_value(&u).unwrap_or(serde_json::Value::Null),
                        );
                    }
                    if let Some(m) = selected_mailbox {
                        obj.insert(
                            "selected_mailbox".to_string(),
                            serde_json::to_value(&m).unwrap_or(serde_json::Value::Null),
                        );
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
                let session_state = conn
                    .protocol_info
                    .data
                    .get("session_state")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())?;
                let authenticated_user = conn
                    .protocol_info
                    .data
                    .get("authenticated_user")
                    .and_then(|v| serde_json::from_value(v.clone()).ok());
                let selected_mailbox = conn
                    .protocol_info
                    .data
                    .get("selected_mailbox")
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
                    obj.insert(
                        "state".to_string(),
                        serde_json::to_value(&protocol_state).unwrap_or(serde_json::Value::Null),
                    );
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
                    obj.insert(
                        "authenticated".to_string(),
                        serde_json::Value::Bool(authenticated),
                    );
                    obj.insert(
                        "username".to_string(),
                        serde_json::to_value(&username).unwrap_or(serde_json::Value::Null),
                    );
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
                    obj.insert(
                        "last_message_type".to_string(),
                        serde_json::Value::String(last_message_type.clone()),
                    );
                    // Mark handshake complete if we've seen both version and verack
                    if last_message_type == "verack" {
                        obj.insert(
                            "handshake_complete".to_string(),
                            serde_json::Value::Bool(true),
                        );
                    }
                }
            }
        }
    }

    /// Update TFTP connection block number and total bytes
    pub async fn update_tftp_connection_block(
        &self,
        server_id: ServerId,
        connection_id: ConnectionId,
        block_number: u16,
        block_bytes: usize,
    ) {
        if let Some(server) = self.inner.write().await.servers.get_mut(&server_id) {
            if let Some(conn) = server.connections.get_mut(&connection_id) {
                if let Some(obj) = conn.protocol_info.data.as_object_mut() {
                    obj.insert(
                        "current_block".to_string(),
                        serde_json::Value::Number(block_number.into()),
                    );
                    // Update total bytes
                    let current_total = obj
                        .get("total_bytes")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    obj.insert(
                        "total_bytes".to_string(),
                        serde_json::Value::Number((current_total + block_bytes as u64).into()),
                    );
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
    ) -> Option<std::sync::Arc<tokio::sync::Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>>>
    {
        // Write halves are no longer stored in centralized state
        // Protocols manage their own local connection data for I/O
        None
    }

    // ========== Client Management Methods ==========

    /// Add a new client instance and return its ID
    pub async fn add_client(&self, mut client: ClientInstance) -> ClientId {
        let mut inner = self.inner.write().await;
        let id = ClientId::new(inner.next_unified_id);
        inner.next_unified_id += 1;
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
                feedback_instructions: c.feedback_instructions.clone(),
                feedback_buffer: c.feedback_buffer.clone(),
                last_feedback_processed: c.last_feedback_processed,
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
                feedback_instructions: c.feedback_instructions.clone(),
                feedback_buffer: c.feedback_buffer.clone(),
                last_feedback_processed: c.last_feedback_processed,
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
        self.inner.write().await.clients.get_mut(&client_id).map(f)
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

    // ========== Easy Protocol Management ==========

    /// Add a new easy protocol instance
    pub async fn add_easy_instance(&self, mut easy_instance: EasyInstance) -> EasyId {
        let mut inner = self.inner.write().await;
        let id = EasyId::new(inner.next_unified_id);
        inner.next_unified_id += 1;
        easy_instance.id = id;
        inner.easy_instances.insert(id, easy_instance);
        id
    }

    /// Remove an easy protocol instance
    pub async fn remove_easy_instance(&self, id: EasyId) -> Option<EasyInstance> {
        let mut inner = self.inner.write().await;
        let instance = inner.easy_instances.remove(&id);

        // Remove from routing maps if present
        if let Some(ref inst) = instance {
            if let Some(server_id) = inst.underlying_server_id {
                inner.server_to_easy.remove(&server_id);
            }
            if let Some(client_id) = inst.underlying_client_id {
                inner.client_to_easy.remove(&client_id);
            }
        }

        instance
    }

    /// Link an underlying server to an easy protocol instance (for event routing)
    pub async fn link_server_to_easy(&self, server_id: ServerId, easy_id: EasyId) {
        let mut inner = self.inner.write().await;
        inner.server_to_easy.insert(server_id, easy_id);
        if let Some(easy_instance) = inner.easy_instances.get_mut(&easy_id) {
            easy_instance.set_underlying_server(server_id);
        }
    }

    /// Link an underlying client to an easy protocol instance (for event routing)
    pub async fn link_client_to_easy(&self, client_id: ClientId, easy_id: EasyId) {
        let mut inner = self.inner.write().await;
        inner.client_to_easy.insert(client_id, easy_id);
        if let Some(easy_instance) = inner.easy_instances.get_mut(&easy_id) {
            easy_instance.set_underlying_client(client_id);
        }
    }

    /// Get easy ID for a given server ID (returns None if server is not managed by an easy protocol)
    pub async fn get_easy_for_server(&self, server_id: ServerId) -> Option<EasyId> {
        self.inner.read().await.server_to_easy.get(&server_id).copied()
    }

    /// Get easy ID for a given client ID (returns None if client is not managed by an easy protocol)
    pub async fn get_easy_for_client(&self, client_id: ClientId) -> Option<EasyId> {
        self.inner.read().await.client_to_easy.get(&client_id).copied()
    }

    /// Get easy protocol instance
    pub async fn get_easy_instance(&self, id: EasyId) -> Option<EasyInstance> {
        // Note: EasyInstance doesn't impl Clone because it contains JoinHandle
        // Instead we provide specific access methods
        self.inner.read().await.easy_instances.get(&id).map(|inst| {
            // Clone all fields except JoinHandle
            let mut cloned = EasyInstance::new(
                inst.id,
                inst.protocol_name.clone(),
                inst.underlying_protocol.clone(),
                inst.user_instruction.clone(),
            );
            cloned.underlying_server_id = inst.underlying_server_id;
            cloned.underlying_client_id = inst.underlying_client_id;
            cloned.status = inst.status.clone();
            cloned.created_at = inst.created_at;
            cloned.status_changed_at = inst.status_changed_at;
            // Note: handle is NOT cloned (cannot clone JoinHandle)
            cloned
        })
    }

    /// Update easy protocol status
    pub async fn update_easy_status(&self, id: EasyId, status: EasyStatus) {
        if let Some(instance) = self.inner.write().await.easy_instances.get_mut(&id) {
            instance.set_status(status);
        }
    }

    /// Get all easy protocol instances
    pub async fn get_all_easy_instances(&self) -> Vec<EasyInstance> {
        self.inner
            .read()
            .await
            .easy_instances
            .values()
            .map(|inst| {
                // Clone all fields except JoinHandle
                let mut cloned = EasyInstance::new(
                    inst.id,
                    inst.protocol_name.clone(),
                    inst.underlying_protocol.clone(),
                    inst.user_instruction.clone(),
                );
                cloned.underlying_server_id = inst.underlying_server_id;
                cloned.underlying_client_id = inst.underlying_client_id;
                cloned.status = inst.status.clone();
                cloned.created_at = inst.created_at;
                cloned.status_changed_at = inst.status_changed_at;
                cloned
            })
            .collect()
    }

    /// Execute a closure with mutable access to an easy instance
    pub async fn with_easy_mut<F, R>(&self, easy_id: EasyId, f: F) -> Option<R>
    where
        F: FnOnce(&mut EasyInstance) -> R,
    {
        self.inner.write().await.easy_instances.get_mut(&easy_id).map(f)
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

    /// Get the first client's ID (for backwards compat)
    pub async fn get_first_client_id(&self) -> Option<ClientId> {
        self.inner.read().await.clients.keys().next().copied()
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
        self.inner.write().await.servers.get_mut(&server_id).map(f)
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

    /// Get conversations for the "Running" column (User + Global tasks)
    pub async fn get_input_conversations(&self) -> Vec<ConversationInfo> {
        let all_convs = self.get_active_conversations().await;
        all_convs
            .into_iter()
            .filter(|conv| {
                matches!(
                    &conv.source,
                    ConversationSource::User | ConversationSource::Scripting // Global tasks would be ConversationSource::Task but we need to filter by scope
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
    pub async fn get_or_create_user_conversation_state(
        &self,
    ) -> Arc<std::sync::Mutex<crate::llm::ConversationState>> {
        let mut inner = self.inner.write().await;

        if inner.user_conversation_state.is_none() {
            // Create with default 50k character limit for conversation history
            let conversation_state = Arc::new(std::sync::Mutex::new(
                crate::llm::ConversationState::new(50000),
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

    // ===== LLM Token Tracking Methods =====

    /// Record LLM call tokens
    pub async fn record_llm_tokens(&self, input_tokens: u64, output_tokens: u64) {
        let mut inner = self.inner.write().await;
        inner.total_input_tokens += input_tokens;
        inner.total_output_tokens += output_tokens;
        inner.total_llm_calls += 1;
    }

    /// Get LLM token statistics
    pub async fn get_llm_stats(&self) -> (u64, u64, u64) {
        let inner = self.inner.read().await;
        (
            inner.total_input_tokens,
            inner.total_output_tokens,
            inner.total_llm_calls,
        )
    }

    /// Reset LLM token statistics
    pub async fn reset_llm_stats(&self) {
        let mut inner = self.inner.write().await;
        inner.total_input_tokens = 0;
        inner.total_output_tokens = 0;
        inner.total_llm_calls = 0;
    }

    // ===== Database Management =====

    /// Create a new database
    #[cfg(feature = "sqlite")]
    pub async fn create_database(
        &self,
        name: String,
        path: String,
        owner: crate::state::DatabaseOwner,
        init_sql: Option<&str>,
    ) -> anyhow::Result<crate::state::DatabaseId> {
        use anyhow::Context;

        let mut inner = self.inner.write().await;
        let id = crate::state::DatabaseId::new(inner.next_unified_id);
        inner.next_unified_id += 1;

        inner
            ._database_manager
            .create_database(id, name, path, owner, init_sql)
            .context("Failed to create database")?;

        Ok(id)
    }

    /// Execute a SQL query on a database
    #[cfg(feature = "sqlite")]
    pub async fn execute_sql(
        &self,
        db_id: crate::state::DatabaseId,
        sql: &str,
    ) -> anyhow::Result<crate::state::QueryResult> {
        let mut inner = self.inner.write().await;
        inner._database_manager.execute_query(db_id, sql)
    }

    /// Get all databases
    #[cfg(feature = "sqlite")]
    pub async fn get_all_databases(&self) -> Vec<crate::state::DatabaseInstance> {
        let inner = self.inner.read().await;
        inner
            ._database_manager
            .get_all_instances()
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get a database by ID
    #[cfg(feature = "sqlite")]
    pub async fn get_database(
        &self,
        db_id: crate::state::DatabaseId,
    ) -> Option<crate::state::DatabaseInstance> {
        let inner = self.inner.read().await;
        inner._database_manager.get_instance(db_id).cloned()
    }

    /// Delete a database
    #[cfg(feature = "sqlite")]
    pub async fn delete_database(&self, db_id: crate::state::DatabaseId) -> anyhow::Result<()> {
        let mut inner = self.inner.write().await;
        inner._database_manager.delete_database(db_id)
    }

    /// Get databases owned by a server
    #[cfg(feature = "sqlite")]
    pub async fn get_databases_by_server(
        &self,
        server_id: crate::state::ServerId,
    ) -> Vec<crate::state::DatabaseInstance> {
        let inner = self.inner.read().await;
        inner
            ._database_manager
            .get_databases_by_server(server_id)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get databases owned by a client
    #[cfg(feature = "sqlite")]
    pub async fn get_databases_by_client(
        &self,
        client_id: crate::state::ClientId,
    ) -> Vec<crate::state::DatabaseInstance> {
        let inner = self.inner.read().await;
        inner
            ._database_manager
            .get_databases_by_client(client_id)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Delete all databases owned by a server (called when server closes)
    #[cfg(feature = "sqlite")]
    pub async fn cleanup_databases_for_server(
        &self,
        server_id: crate::state::ServerId,
    ) -> anyhow::Result<()> {
        let mut inner = self.inner.write().await;
        inner._database_manager.delete_databases_by_server(server_id)
    }

    /// Delete all databases owned by a client (called when client disconnects)
    #[cfg(feature = "sqlite")]
    pub async fn cleanup_databases_for_client(
        &self,
        client_id: crate::state::ClientId,
    ) -> anyhow::Result<()> {
        let mut inner = self.inner.write().await;
        inner._database_manager.delete_databases_by_client(client_id)
    }

    /// Test-only helper to add a server with a specific ID (bypasses auto-increment)
    ///
    /// WARNING: This method is intended for testing only. In production code, use `add_server()`
    /// which properly assigns server IDs.
    pub async fn add_server_with_id(&self, server: ServerInstance) {
        let mut inner = self.inner.write().await;
        inner.servers.insert(server.id, server);

        // Set mode to Server if this is the first server
        if inner.mode == Mode::Idle {
            inner.mode = Mode::Server;
        }
    }

    /// Store Tor client instance for directory queries
    #[cfg(feature = "tor")]
    pub async fn set_tor_client(
        &self,
        client_id: ClientId,
        tor_client: Arc<arti_client::TorClient<tor_rtcompat::PreferredRuntime>>,
    ) {
        let mut inner = self.inner.write().await;
        inner.tor_clients.insert(client_id, tor_client);
    }

    /// Get Tor client instance by client ID
    #[cfg(feature = "tor")]
    pub async fn get_tor_client(&self, client_id: ClientId) -> Option<Arc<arti_client::TorClient<tor_rtcompat::PreferredRuntime>>> {
        let inner = self.inner.read().await;
        inner.tor_clients.get(&client_id).cloned()
    }

    /// Remove Tor client instance (called when client closes)
    #[cfg(feature = "tor")]
    pub async fn remove_tor_client(&self, client_id: ClientId) {
        let mut inner = self.inner.write().await;
        inner.tor_clients.remove(&client_id);
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
