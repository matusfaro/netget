//! Client instance management

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Instant;
use tokio::task::JoinHandle;

/// Unique identifier for a client instance
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ClientId(u32);

impl ClientId {
    /// Create a new client ID from a u32
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get the raw ID value
    pub fn as_u32(&self) -> u32 {
        self.0
    }

    /// Parse from string (expects format "client-123" or just "123")
    pub fn from_string(s: &str) -> Option<Self> {
        let s = s.trim();
        let id_str = s.strip_prefix("client-").unwrap_or(s);
        id_str.parse::<u32>().ok().map(Self)
    }
}

impl std::fmt::Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "client-{}", self.0)
    }
}

/// Status of a client connection
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientStatus {
    /// Client is connecting
    Connecting,
    /// Client is connected and active
    Connected,
    /// Client connection has been closed
    Disconnected,
    /// Client encountered an error
    Error(String),
}

impl ClientStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Connecting => "Connecting",
            Self::Connected => "Connected",
            Self::Disconnected => "Disconnected",
            Self::Error(_) => "Error",
        }
    }
}

impl std::fmt::Display for ClientStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connecting => write!(f, "Connecting"),
            Self::Connected => write!(f, "Connected"),
            Self::Disconnected => write!(f, "Disconnected"),
            Self::Error(msg) => write!(f, "Error: {}", msg),
        }
    }
}

/// Client connection state
#[derive(Debug, Clone)]
pub struct ClientConnectionState {
    /// Client ID
    pub id: ClientId,
    /// Remote server address (hostname:port or IP:port)
    pub remote_addr: String,
    /// Actual connected socket address (after DNS resolution)
    pub connected_addr: Option<SocketAddr>,
    /// Local address (our side of the connection)
    pub local_addr: Option<SocketAddr>,
    /// Bytes sent
    pub bytes_sent: u64,
    /// Bytes received
    pub bytes_received: u64,
    /// Packets sent
    pub packets_sent: u64,
    /// Packets received
    pub packets_received: u64,
    /// Last activity timestamp
    pub last_activity: Instant,
    /// Connection status (Connecting/Connected/Disconnected)
    pub status: ClientStatus,
    /// When status last changed
    pub status_changed_at: Instant,
    /// Protocol-specific information (flexible JSON storage)
    pub protocol_info: super::server::ProtocolConnectionInfo,
}

/// A client instance with its own connection, state, and configuration
#[derive(Debug)]
pub struct ClientInstance {
    /// Unique client ID
    pub id: ClientId,
    /// Remote server address (hostname:port or IP:port)
    pub remote_addr: String,
    /// Protocol name (e.g., "HTTP", "SSH", "Redis")
    pub protocol_name: String,
    /// User instructions for this client
    pub instruction: String,
    /// LLM memory for this client
    pub memory: String,
    /// Client connection status
    pub status: ClientStatus,
    /// Connection state (if connected)
    pub connection: Option<ClientConnectionState>,
    /// Client task handle (for cleanup)
    pub handle: Option<JoinHandle<()>>,
    /// When the client was created
    pub created_at: Instant,
    /// When the client status last changed
    pub status_changed_at: Instant,
    /// Protocol-specific startup parameters
    pub startup_params: Option<serde_json::Value>,
    /// Event handler configuration for handling protocol events
    pub event_handler_config: Option<crate::scripting::EventHandlerConfig>,
    /// Protocol-specific client data (flexible storage)
    ///
    /// This replaces protocol-specific feature-gated fields.
    /// Each protocol can store any data structure here by serializing to JSON.
    /// Use get_protocol_data() and set_protocol_data() helper methods.
    pub protocol_data: serde_json::Value,
    /// Log file paths (output_name -> log_file_path)
    pub log_files: HashMap<String, PathBuf>,
    /// Feedback instructions for automatic client adjustment
    /// When set, server responses can provide feedback that triggers LLM-based client adjustments
    pub feedback_instructions: Option<String>,
    /// Accumulated feedback buffer (cleared after processing)
    /// Each entry is a JSON value containing feedback data
    pub feedback_buffer: Vec<serde_json::Value>,
    /// Last time feedback was processed (for debouncing)
    pub last_feedback_processed: Option<Instant>,
}

impl ClientInstance {
    /// Create a new client instance
    pub fn new(
        id: ClientId,
        remote_addr: String,
        protocol_name: String,
        instruction: String,
    ) -> Self {
        let now = Instant::now();
        Self {
            id,
            remote_addr,
            protocol_name,
            instruction,
            memory: String::new(),
            status: ClientStatus::Connecting,
            connection: None,
            handle: None,
            created_at: now,
            status_changed_at: now,
            startup_params: None,
            event_handler_config: None,
            protocol_data: serde_json::Value::Object(serde_json::Map::new()),
            log_files: HashMap::new(),
            feedback_instructions: None,
            feedback_buffer: Vec::new(),
            last_feedback_processed: None,
        }
    }

    /// Get protocol-specific data
    pub fn get_protocol_data<T: serde::de::DeserializeOwned>(
        &self,
    ) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.protocol_data.clone())
    }

    /// Set protocol-specific data
    pub fn set_protocol_data<T: serde::Serialize>(
        &mut self,
        data: T,
    ) -> Result<(), serde_json::Error> {
        self.protocol_data = serde_json::to_value(data)?;
        Ok(())
    }

    /// Get a field from protocol data
    pub fn get_protocol_field(&self, key: &str) -> Option<&serde_json::Value> {
        self.protocol_data.get(key)
    }

    /// Set a field in protocol data
    pub fn set_protocol_field(&mut self, key: String, value: serde_json::Value) {
        if let Some(obj) = self.protocol_data.as_object_mut() {
            obj.insert(key, value);
        } else {
            // Initialize as object if not already
            let mut map = serde_json::Map::new();
            map.insert(key, value);
            self.protocol_data = serde_json::Value::Object(map);
        }
    }

    /// Get or create a log file path for the given output name
    /// Returns the path to the log file with format: netget_<output_name>_<timestamp>.log
    /// The timestamp is based on when the client was created
    pub fn get_or_create_log_path(&mut self, output_name: &str) -> PathBuf {
        if let Some(path) = self.log_files.get(output_name) {
            return path.clone();
        }

        // Calculate the absolute time when the client was created
        // by subtracting the elapsed time from now
        let now = std::time::SystemTime::now();
        let elapsed = self.created_at.elapsed();
        let created_system_time = now - elapsed;

        // Convert to DateTime for formatting
        let timestamp: chrono::DateTime<chrono::Local> = created_system_time.into();
        let timestamp_str = timestamp.format("%Y_%m_%d_%H_%M_%S").to_string();

        let log_filename = format!("netget_{}_{}.log", output_name, timestamp_str);
        let log_path = PathBuf::from(log_filename);

        self.log_files
            .insert(output_name.to_string(), log_path.clone());
        log_path
    }

    /// Get a summary for display
    pub fn summary(&self) -> String {
        let duration = self.created_at.elapsed();
        let duration_str = if duration.as_secs() < 60 {
            format!("{}s", duration.as_secs())
        } else if duration.as_secs() < 3600 {
            format!("{}m", duration.as_secs() / 60)
        } else {
            format!("{}h", duration.as_secs() / 3600)
        };

        format!(
            "#{} {} → {} ({}) - {}",
            self.id.as_u32(),
            self.protocol_name,
            self.remote_addr,
            self.status.as_str(),
            duration_str
        )
    }
}
