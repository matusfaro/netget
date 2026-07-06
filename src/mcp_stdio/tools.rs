//! MCP tool definitions and handlers for the STDIO server
//!
//! Each tool maps to existing NetGet functionality (server management, protocol listing, etc.)
//! Tools use rmcp's macro system for automatic schema generation.

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{
    CallToolResult, Content, Implementation, InitializeRequestParams, InitializeResult,
    ServerCapabilities,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};
use tracing::info;

/// Deserialize Option<u16> from either a number or a string (MCP clients may send "6178" instead of 6178)
fn deserialize_option_u16_flexible<'de, D>(deserializer: D) -> Result<Option<u16>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrString {
        Number(u16),
        String(String),
    }
    match Option::<NumOrString>::deserialize(deserializer)? {
        Some(NumOrString::Number(n)) => Ok(Some(n)),
        Some(NumOrString::String(s)) => s.parse().map(Some).map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

/// Deserialize u32 from either a number or a string
fn deserialize_u32_flexible<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrString {
        Number(u32),
        String(String),
    }
    match NumOrString::deserialize(deserializer)? {
        NumOrString::Number(n) => Ok(n),
        NumOrString::String(s) => s.parse().map_err(serde::de::Error::custom),
    }
}

use crate::cli::Args;
use crate::llm::OllamaClient;
use crate::mcp_stdio::sampling::SamplingRequest;
use crate::settings::Settings;
use crate::state::app_state::AppState;

/// Shared state for the MCP server
///
/// One instance is shared across all MCP sessions (a single session for STDIO,
/// potentially many for the HTTP transport).
pub(crate) struct SharedState {
    app_state: AppState,
    llm_client: OllamaClient,
    _status_tx: mpsc::UnboundedSender<String>,
    /// Peer connection to the most recently initialized MCP client.
    /// The sampling forwarder reads this on every request.
    peer: Arc<Mutex<Option<rmcp::service::Peer<RoleServer>>>>,
    /// Whether the MCP client supports sampling
    client_supports_sampling: std::sync::atomic::AtomicBool,
    /// Sampling request sender (given to OllamaClient::new_sampling for protocol servers)
    sampling_tx: mpsc::UnboundedSender<SamplingRequest>,
    /// Raw capabilities JSON from initialize request (for debugging - serde may drop unknown fields)
    raw_capabilities: Mutex<String>,
}

/// NetGet MCP STDIO service - exposes NetGet capabilities as MCP tools
#[derive(Clone)]
pub struct NetGetMcpService {
    state: Arc<SharedState>,
    tool_router: ToolRouter<Self>,
}

// === Tool parameter structs ===

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListProtocolsParams {
    /// Filter by "server", "client", or "all"
    #[serde(default = "default_all")]
    pub r#type: Option<String>,
    /// Include experimental/honeypot protocols
    #[serde(default)]
    pub include_disabled: Option<bool>,
}

fn default_all() -> Option<String> {
    Some("all".to_string())
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StartServerParams {
    /// Protocol name. Use list_protocols tool to see all available protocols.
    pub protocol: String,
    /// Port to listen on (0 for OS-assigned)
    #[serde(default, deserialize_with = "deserialize_option_u16_flexible")]
    pub port: Option<u16>,
    /// Natural language instruction for the LLM controlling this server
    #[serde(default)]
    pub instruction: Option<String>,
    /// Host address to bind to (default: 127.0.0.1)
    #[serde(default)]
    pub host: Option<String>,
    /// LLM provider for the protocol server. Options: "ollama" (local Ollama, default), "openai" (OpenAI-compatible API), "sampling" (route LLM calls through MCP client - only if client supports sampling capability).
    #[serde(default)]
    pub llm_provider: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StopServerParams {
    /// Server ID to stop
    #[serde(deserialize_with = "deserialize_u32_flexible")]
    pub server_id: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ServerStatusParams {
    /// Server ID to query
    #[serde(deserialize_with = "deserialize_u32_flexible")]
    pub server_id: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetModelParams {
    /// Model name (e.g., "qwen3-coder:30b", "llama3.2:latest")
    pub model: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetProtocolDocsParams {
    /// Protocol name (e.g., "http", "dns", "tcp")
    pub protocol: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateServerInstructionParams {
    /// Server ID
    #[serde(deserialize_with = "deserialize_u32_flexible")]
    pub server_id: u32,
    /// New instruction text
    pub instruction: String,
}

// === Tool implementations ===

#[tool_router]
impl NetGetMcpService {
    pub async fn new(args: &Args, settings: Settings) -> anyhow::Result<Self> {
        let state = Self::create_shared_state(args, settings).await?;
        Ok(Self::new_with_shared_state(state))
    }

    /// Create the shared state used by all MCP sessions.
    ///
    /// For STDIO this is called once for the single session; for the HTTP
    /// transport it is created once and shared across all sessions so they
    /// see the same servers/clients.
    pub(crate) async fn create_shared_state(
        args: &Args,
        _settings: Settings,
    ) -> anyhow::Result<Arc<SharedState>> {
        let lock_enabled = args.ollama_lock;

        // Create LLM client (Ollama/OpenAI - sampling added after init)
        let llm_client = crate::cli::create_llm_client(args, lock_enabled)?;

        // Create app state
        let app_state = AppState::new();

        // Status channel (messages go to stderr in MCP mode)
        let (status_tx, mut status_rx) = mpsc::unbounded_channel();

        // Sampling channel for forwarding LLM requests to MCP client
        let (sampling_tx, sampling_rx) = mpsc::unbounded_channel();

        // Set up model if specified
        if let Some(ref model) = args.model {
            app_state.set_ollama_model(Some(model.clone())).await;
        }

        let peer = Arc::new(Mutex::new(None));

        // Forward sampling requests to whichever MCP client is currently connected
        crate::mcp_stdio::sampling::spawn_sampling_forwarder(peer.clone(), sampling_rx);

        let state = Arc::new(SharedState {
            app_state,
            llm_client,
            _status_tx: status_tx,
            peer,
            client_supports_sampling: std::sync::atomic::AtomicBool::new(false),
            sampling_tx,
            raw_capabilities: Mutex::new("(not yet initialized)".to_string()),
        });

        // Drain status messages to stderr in background
        tokio::spawn(async move {
            while let Some(msg) = status_rx.recv().await {
                eprintln!("[NETGET] {}", msg);
            }
        });

        Ok(state)
    }

    /// Create a service instance backed by pre-built shared state
    /// (used by the HTTP transport, which creates one service per session)
    pub(crate) fn new_with_shared_state(state: Arc<SharedState>) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "List all available network protocols that can be started as servers or clients")]
    async fn list_protocols(
        &self,
        Parameters(params): Parameters<ListProtocolsParams>,
    ) -> Result<CallToolResult, McpError> {
        let filter_type = params.r#type.as_deref().unwrap_or("all");
        let include_disabled = params.include_disabled.unwrap_or(false);
        let mut result = String::new();

        if filter_type == "all" || filter_type == "server" {
            let registry = crate::protocol::server_registry::registry();
            let protocols = registry.available_protocols();
            result.push_str("## Server Protocols\n\n");
            for name in &protocols {
                if let Some(proto) = registry.get(name) {
                    let metadata = proto.metadata();
                    if !include_disabled
                        && matches!(
                            metadata.state,
                            crate::protocol::metadata::DevelopmentState::Incomplete
                        )
                    {
                        continue;
                    }
                    result.push_str(&format!(
                        "- **{}** ({}) - {}\n",
                        name,
                        metadata.state.as_str(),
                        metadata.implementation
                    ));
                }
            }
            result.push('\n');
        }

        if filter_type == "all" || filter_type == "client" {
            let registry = &crate::protocol::CLIENT_REGISTRY;
            let protocols = registry.list_protocols();
            result.push_str("## Client Protocols\n\n");
            for name in &protocols {
                result.push_str(&format!("- **{}**\n", name));
            }
            result.push('\n');
        }

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Start a network protocol server controlled by an LLM. Set llm_provider to 'sampling' to use the MCP client's LLM (recommended), or 'ollama'/'openai' for a local LLM.")]
    async fn start_server(
        &self,
        Parameters(params): Parameters<StartServerParams>,
    ) -> Result<CallToolResult, McpError> {
        let instruction = params.instruction.unwrap_or_else(|| {
            format!(
                "You are a {} server. Handle requests appropriately.",
                params.protocol
            )
        });

        // Check sampling support from both the AtomicBool flag and the stored peer info
        let supports_sampling_flag = self
            .state
            .client_supports_sampling
            .load(std::sync::atomic::Ordering::SeqCst);

        // Also check peer info directly (more reliable - covers case where initialize override wasn't called)
        let supports_sampling_peer = if let Some(ref peer) = *self.state.peer.lock().await {
            peer.peer_info()
                .map(|info| {
                    info.capabilities.sampling.is_some()
                        || info
                            .capabilities
                            .tasks
                            .as_ref()
                            .and_then(|t| t.requests.as_ref())
                            .and_then(|r| r.sampling.as_ref())
                            .is_some()
                })
                .unwrap_or(false)
        } else {
            false
        };

        let supports_sampling = supports_sampling_flag || supports_sampling_peer;

        // Default to "ollama" unless explicitly set
        let provider = params.llm_provider.as_deref().unwrap_or("ollama");

        // Set the appropriate LLM client on the AppState for this server
        match provider {
            "sampling" if supports_sampling => {
                let sampling_client =
                    OllamaClient::new_sampling(self.state.sampling_tx.clone());
                self.state
                    .app_state
                    .set_llm_client(sampling_client)
                    .await;
                info!("MCP: Using sampling (MCP client's LLM) for {} server", params.protocol);
            }
            "sampling" => {
                return Ok(CallToolResult::error(vec![Content::text(
                    "Cannot use 'sampling' provider: this MCP client does not advertise sampling support. Use 'ollama' or 'openai' instead.",
                )]));
            }
            _ => {
                // Use the default LLM client (Ollama/OpenAI from CLI args)
                self.state
                    .app_state
                    .set_llm_client(self.state.llm_client.clone())
                    .await;
                info!("MCP: Using {} LLM for {} server", provider, params.protocol);
            }
        }

        info!(
            "MCP: Starting {} server on {}:{}",
            params.protocol,
            params.host.as_deref().unwrap_or("127.0.0.1"),
            params.port.unwrap_or(0)
        );

        let (status_tx, mut status_rx) = mpsc::unbounded_channel();
        let server_id = match crate::cli::server_startup::start_server_from_action(
            &self.state.app_state,
            None,                // mac_address
            None,                // interface
            params.host.clone(), // host
            params.port,         // port
            &params.protocol,    // protocol
            false,               // send_first
            None,                // initial_memory
            instruction,         // instruction
            None,                // startup_params
            None,                // event_handlers
            None,                // scheduled_tasks
            None,                // feedback_instructions
            status_tx,           // status_tx
        )
        .await
        {
            Ok(id) => id,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to start server: {}",
                    e
                ))]));
            }
        };

        // Collect startup status messages
        let mut messages = Vec::new();
        while let Ok(msg) = status_rx.try_recv() {
            messages.push(msg);
        }

        let result = format!(
            "Server #{} ({}) started.\n{}",
            server_id.as_u32(),
            params.protocol,
            messages.join("\n")
        );

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Stop a running server by its ID")]
    async fn stop_server(
        &self,
        Parameters(params): Parameters<StopServerParams>,
    ) -> Result<CallToolResult, McpError> {
        let server_id = crate::state::ServerId::new(params.server_id);

        match self.state.app_state.get_server(server_id).await {
            Some(server) => {
                let protocol = server.protocol_name.clone();
                let port = server.port;
                self.state.app_state.remove_server(server_id).await;
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Server #{} ({} on port {}) stopped",
                    params.server_id, protocol, port
                ))]))
            }
            None => Ok(CallToolResult::error(vec![Content::text(format!(
                "Server #{} not found",
                params.server_id
            ))])),
        }
    }

    #[tool(description = "List all running servers with their status, protocol, port, and connection count")]
    async fn list_servers(&self) -> Result<CallToolResult, McpError> {
        let servers = self.state.app_state.get_all_servers().await;

        if servers.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No servers currently running.",
            )]));
        }

        let mut result = String::from("## Running Servers\n\n");
        for server in &servers {
            result.push_str(&format!(
                "- **Server #{}**: {} on port {} ({})\n  Instruction: {}\n  Memory: {}\n\n",
                server.id.as_u32(),
                server.protocol_name,
                server.port,
                server.status,
                if server.instruction.is_empty() {
                    "(none)"
                } else {
                    &server.instruction
                },
                if server.memory.is_empty() {
                    "(empty)"
                } else {
                    &server.memory
                },
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get detailed status of a specific server")]
    async fn server_status(
        &self,
        Parameters(params): Parameters<ServerStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        let server_id = crate::state::ServerId::new(params.server_id);

        match self.state.app_state.get_server(server_id).await {
            Some(server) => {
                let result = format!(
                    "## Server #{}\n\n\
                     - **Protocol**: {}\n\
                     - **Port**: {}\n\
                     - **Status**: {}\n\
                     - **Instruction**: {}\n\
                     - **Memory**: {}\n",
                    server.id.as_u32(),
                    server.protocol_name,
                    server.port,
                    server.status,
                    if server.instruction.is_empty() {
                        "(none)"
                    } else {
                        &server.instruction
                    },
                    if server.memory.is_empty() {
                        "(empty)"
                    } else {
                        &server.memory
                    },
                );
                Ok(CallToolResult::success(vec![Content::text(result)]))
            }
            None => Ok(CallToolResult::error(vec![Content::text(format!(
                "Server #{} not found",
                params.server_id
            ))])),
        }
    }

    #[tool(description = "Get overall NetGet status including model, running servers count, MCP client capabilities, and configuration")]
    async fn get_status(&self) -> Result<CallToolResult, McpError> {
        let servers = self.state.app_state.get_all_servers().await;
        let model = self
            .state
            .app_state
            .get_ollama_model()
            .await
            .unwrap_or_else(|| "(auto-select)".to_string());

        let supports_sampling_flag = self
            .state
            .client_supports_sampling
            .load(std::sync::atomic::Ordering::SeqCst);

        let (supports_sampling_peer, client_name, client_caps) = {
            let peer_guard = self.state.peer.lock().await;
            if let Some(ref peer) = *peer_guard {
                let info = peer.peer_info();
                let sampling = info
                    .map(|i| {
                        i.capabilities.sampling.is_some()
                            || i.capabilities
                                .tasks
                                .as_ref()
                                .and_then(|t| t.requests.as_ref())
                                .and_then(|r| r.sampling.as_ref())
                                .is_some()
                    })
                    .unwrap_or(false);
                let name = info
                    .map(|i| format!("{} v{}", i.client_info.name, i.client_info.version))
                    .unwrap_or_else(|| "(no peer info)".to_string());
                let caps = info
                    .map(|i| {
                        serde_json::to_string(&i.capabilities)
                            .unwrap_or_else(|_| "?".to_string())
                    })
                    .unwrap_or_else(|| "(none)".to_string());
                (sampling, name, caps)
            } else {
                (false, "(no peer)".to_string(), "(none)".to_string())
            }
        };

        let raw_caps = self.state.raw_capabilities.lock().await.clone();

        let mut result = format!(
            "## NetGet Status\n\n\
             - **Model**: {}\n\
             - **Running servers**: {}\n\
             - **MCP client**: {}\n\
             - **Sampling support**: flag={}, peer={}\n\
             - **Client capabilities (deserialized)**: {}\n\
             - **Client capabilities (raw from initialize)**: {}\n",
            model,
            servers.len(),
            client_name,
            supports_sampling_flag,
            supports_sampling_peer,
            client_caps,
            raw_caps,
        );

        if !servers.is_empty() {
            result.push_str("\n### Servers\n");
            for server in &servers {
                result.push_str(&format!(
                    "- #{}: {} on port {} ({})\n",
                    server.id.as_u32(),
                    server.protocol_name,
                    server.port,
                    server.status
                ));
            }
        }

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Change the default LLM model used for new servers/clients")]
    async fn set_model(
        &self,
        Parameters(params): Parameters<SetModelParams>,
    ) -> Result<CallToolResult, McpError> {
        self.state
            .app_state
            .set_ollama_model(Some(params.model.clone()))
            .await;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Model changed to: {}",
            params.model
        ))]))
    }

    #[tool(description = "Get documentation for a specific protocol including actions, events, startup parameters, and examples")]
    async fn get_protocol_docs(
        &self,
        Parameters(params): Parameters<GetProtocolDocsParams>,
    ) -> Result<CallToolResult, McpError> {
        // Try to find protocol in registries first (case-insensitive)
        let server_registry = crate::protocol::server_registry::registry();
        let client_registry = &crate::protocol::CLIENT_REGISTRY;

        // Try exact, uppercase, and lowercase
        let server_found = server_registry.get(&params.protocol).is_some()
            || server_registry.get(&params.protocol.to_uppercase()).is_some()
            || server_registry.get(&params.protocol.to_lowercase()).is_some();
        let client_found = client_registry.get(&params.protocol).is_some()
            || client_registry.get(&params.protocol.to_lowercase()).is_some();

        if !server_found && !client_found {
            // Protocol not compiled in - list what's available
            let available = server_registry.available_protocols();
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "Protocol '{}' not found. It may not be compiled into this build.\n\n\
                 Available protocols ({}):\n{}\n\n\
                 Build with the protocol feature enabled: cargo build --features mcp-stdio,{}",
                params.protocol,
                available.len(),
                available.iter().map(|p| format!("  - {}", p)).collect::<Vec<_>>().join("\n"),
                params.protocol.to_lowercase(),
            ))]));
        }

        // Use the existing read_documentation tool logic
        let result = crate::llm::actions::execute_tool(
            &crate::llm::actions::ToolAction::ReadDocumentation {
                protocols: vec![params.protocol.clone()],
                protocol: None,
            },
            None,
            crate::state::app_state::WebSearchMode::Off,
            None,
        )
        .await;

        if result.success {
            Ok(CallToolResult::success(vec![Content::text(result.result)]))
        } else {
            Ok(CallToolResult::error(vec![Content::text(result.result)]))
        }
    }

    #[tool(description = "Update the LLM instruction for a running server")]
    async fn update_server_instruction(
        &self,
        Parameters(params): Parameters<UpdateServerInstructionParams>,
    ) -> Result<CallToolResult, McpError> {
        let server_id = crate::state::ServerId::new(params.server_id);

        let updated = self
            .state
            .app_state
            .with_server_mut(server_id, |server| {
                server.instruction = params.instruction.clone();
            })
            .await;

        match updated {
            Some(()) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Server #{} instruction updated",
                params.server_id
            ))])),
            None => Ok(CallToolResult::error(vec![Content::text(format!(
                "Server #{} not found",
                params.server_id
            ))])),
        }
    }

    #[tool(description = "Stop all running servers and connections")]
    async fn stop_all(&self) -> Result<CallToolResult, McpError> {
        let server_ids = self.state.app_state.get_all_server_ids().await;
        let count = server_ids.len();

        for id in server_ids {
            self.state.app_state.remove_server(id).await;
        }

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Stopped {} server(s)",
            count
        ))]))
    }
}

#[tool_handler]
impl ServerHandler for NetGetMcpService {
    fn get_info(&self) -> <RoleServer as rmcp::service::ServiceRole>::Info {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("netget", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "NetGet - LLM-controlled network protocol server. \
                 Use list_protocols to discover available protocols, then start_server to \
                 create protocol servers controlled by an LLM.",
            )
    }

    fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<InitializeResult, McpError>> + Send + '_ {
        async move {
            // Store the peer so the sampling forwarder targets this client
            *self.state.peer.lock().await = Some(context.peer.clone());

            // Store raw capabilities for status tool
            *self.state.raw_capabilities.lock().await =
                serde_json::to_string(&request.capabilities).unwrap_or_else(|_| "?".to_string());

            // Check if client supports sampling (two possible locations in MCP spec)
            // 1. capabilities.sampling (SEP-1577 top-level)
            // 2. capabilities.tasks.requests.sampling (task-augmented path)
            let supports_sampling_top = request.capabilities.sampling.is_some();
            let supports_sampling_tasks = request
                .capabilities
                .tasks
                .as_ref()
                .and_then(|t| t.requests.as_ref())
                .and_then(|r| r.sampling.as_ref())
                .is_some();
            let supports_sampling = supports_sampling_top || supports_sampling_tasks;

            let supports_elicitation_top = request.capabilities.elicitation.is_some();
            let supports_elicitation_tasks = request
                .capabilities
                .tasks
                .as_ref()
                .and_then(|t| t.requests.as_ref())
                .and_then(|r| r.elicitation.as_ref())
                .is_some();
            let supports_elicitation = supports_elicitation_top || supports_elicitation_tasks;
            self.state
                .client_supports_sampling
                .store(supports_sampling, std::sync::atomic::Ordering::SeqCst);

            info!(
                "MCP client '{}' v{} initialized (sampling: {}, elicitation: {})",
                request.client_info.name,
                request.client_info.version,
                supports_sampling,
                supports_elicitation,
            );

            // Store peer info so it's available for capability checks later
            if context.peer.peer_info().is_none() {
                context.peer.set_peer_info(request);
            }

            Ok(self.get_info())
        }
    }
}
