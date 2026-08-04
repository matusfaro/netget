//! MCP tool definitions and handlers for the STDIO server
//!
//! Each tool maps to existing NetGet functionality (server management, protocol listing, etc.)
//! Tools use rmcp's macro system for automatic schema generation.

use std::sync::Arc;
use tokio::sync::mpsc;

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

/// Deserialize u64 from either a number or a string
fn deserialize_u64_flexible<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrString {
        Number(u64),
        String(String),
    }
    match NumOrString::deserialize(deserializer)? {
        NumOrString::Number(n) => Ok(n),
        NumOrString::String(s) => s.parse().map_err(serde::de::Error::custom),
    }
}

/// Deserialize Option<u32> from either a number or a string
fn deserialize_option_u32_flexible<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrString {
        Number(u32),
        String(String),
    }
    match Option::<NumOrString>::deserialize(deserializer)? {
        Some(NumOrString::Number(n)) => Ok(Some(n)),
        Some(NumOrString::String(s)) => s.parse().map(Some).map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

use crate::cli::Args;
use crate::llm::OllamaClient;
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
    /// Natural language instruction for the LLM that handles each request.
    /// This is the LLM FALLBACK path: every request/event triggers a (slow,
    /// billable) model call. Use it only when responses genuinely require
    /// reasoning or vary unpredictably. For deterministic behavior (echo,
    /// fixed/canned responses, simple routing, high throughput) prefer
    /// `event_handlers` with a script or static handler instead — see below.
    #[serde(default)]
    pub instruction: Option<String>,
    /// Host address to bind to (default: 127.0.0.1)
    #[serde(default)]
    pub host: Option<String>,
    /// Event handlers that decide how incoming events are handled WITHOUT an LLM
    /// call. Strongly preferred over `instruction` whenever the behavior is
    /// deterministic — a script/static handler runs in-process (instant, free,
    /// deterministic), while `instruction` pays a model round-trip per request.
    ///
    /// Array of objects: { "event_pattern": "<event id>" | "*", "handler": {...} }
    /// matched in order, first match wins. `handler` is one of:
    ///   - {"type":"script","language":"python"|"javascript","code":"..."} — runs
    ///     the script per event. The event is JSON on stdin: read the payload via
    ///     data['event'][<field>] and print {"actions":[ ... ]} to stdout. Field
    ///     names vary by protocol (e.g. Telnet event `telnet_message_received` has
    ///     `message`; HTTP `http_request` has `method`/`path`/`headers`/`body`).
    ///   - {"type":"static","actions":[ ... ]} — always emit these fixed actions.
    ///   - {"type":"llm","instruction":"..."} — fall back to the LLM (reasoning).
    ///
    /// Example — a Telnet echo server with ZERO LLM calls (echoes each line back
    /// verbatim):
    ///   [{"event_pattern":"telnet_message_received","handler":{"type":"script",
    ///     "language":"python",
    ///     "code":"import json,sys;d=json.load(sys.stdin);print(json.dumps({'actions':[{'type':'send_telnet_message','message':d['event']['message']}]}))"}}]
    /// Always confirm a protocol's exact event ids, field names, and action names
    /// with get_protocol_docs before writing a handler.
    #[serde(default)]
    pub event_handlers: Option<Vec<serde_json::Value>>,
    /// Optional protocol-specific startup parameters (JSON object). For example,
    /// HTTP accepts a `request_filter` (array of {methods, path regex, headers}
    /// rules — only matching requests reach the LLM; the rest get `filtered_response`,
    /// default 404) and a `filtered_response` ({status, body, headers}). Use
    /// get_protocol_docs for a protocol's available startup parameters.
    #[serde(default)]
    pub startup_params: Option<serde_json::Value>,
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
pub struct ListAccessLogsParams {
    /// Maximum number of recent entries to return (default 20)
    #[serde(default, deserialize_with = "deserialize_option_u32_flexible")]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetAccessLogParams {
    /// Access-log entry id (from list_access_logs)
    #[serde(deserialize_with = "deserialize_u64_flexible")]
    pub id: u64,
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

        // Create the LLM client (Ollama or OpenAI-compatible) from CLI args
        let llm_client = crate::cli::create_llm_client(args, lock_enabled)?;

        // Create app state
        let app_state = AppState::new();

        // Status channel (messages go to stderr in MCP mode)
        let (status_tx, mut status_rx) = mpsc::unbounded_channel();

        // Set up model if specified
        if let Some(ref model) = args.model {
            app_state.set_ollama_model(Some(model.clone())).await;
        }

        let state = Arc::new(SharedState {
            app_state,
            llm_client,
            _status_tx: status_tx,
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

    #[tool(description = "Start a network protocol server. Choose HOW it responds: for DETERMINISTIC behavior (echo, fixed/canned responses, simple routing, high throughput) pass `event_handlers` with a script or static handler — these run in-process with NO LLM call (instant, free, reproducible). Only use the natural-language `instruction` when responses genuinely need reasoning or vary unpredictably; it invokes NetGet's LLM on every request. Prefer event_handlers whenever the logic is fixed.")]
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

        // Drive the new server with NetGet's configured LLM client (Ollama/OpenAI from CLI args)
        self.state
            .app_state
            .set_llm_client(self.state.llm_client.clone())
            .await;

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
            instruction,           // instruction
            params.startup_params, // startup_params
            params.event_handlers, // event_handlers
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

    #[tool(description = "Get overall NetGet status including the configured model and running servers")]
    async fn get_status(&self) -> Result<CallToolResult, McpError> {
        let servers = self.state.app_state.get_all_servers().await;
        let model = self
            .state
            .app_state
            .get_ollama_model()
            .await
            .unwrap_or_else(|| "(auto-select)".to_string());

        let mut result = format!(
            "## NetGet Status\n\n\
             - **Model**: {}\n\
             - **LLM backend**: {}\n\
             - **Running servers**: {}\n",
            model,
            self.state.llm_client.backend_type(),
            servers.len(),
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

    #[tool(description = "Get MCP-shaped documentation for a protocol: the exact start_server/start_client arguments that apply, the protocol's event ids with their field names and types (needed to write an event_handlers script), its action names with parameter schemas and examples, its startup parameters, its privilege requirement, and its maturity.")]
    async fn get_protocol_docs(
        &self,
        Parameters(params): Parameters<GetProtocolDocsParams>,
    ) -> Result<CallToolResult, McpError> {
        match crate::mcp_stdio::docs::render_protocol_docs(
            &params.protocol,
            &self.state.app_state,
        )
        .await
        {
            Some(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            None => {
                // Protocol not compiled in - list what's available
                let server_registry = crate::protocol::server_registry::registry();
                let available = server_registry.available_protocols();
                Ok(CallToolResult::error(vec![Content::text(format!(
                    "Protocol '{}' not found. It may not be compiled into this build.\n\n\
                     Available server protocols ({}):\n{}\n\n\
                     Build with the protocol feature enabled: cargo build --features mcp-stdio,{}",
                    params.protocol,
                    available.len(),
                    available
                        .iter()
                        .map(|p| format!("  - {}", p))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    params.protocol.to_lowercase(),
                ))]))
            }
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

    #[tool(description = "List recent request/response access-log entries across all servers (newest first). Use get_access_log with an entry id to see the full request and response.")]
    async fn list_access_logs(
        &self,
        Parameters(params): Parameters<ListAccessLogsParams>,
    ) -> Result<CallToolResult, McpError> {
        let limit = params.limit.unwrap_or(20) as usize;
        let entries = self.state.app_state.list_access_logs(Some(limit)).await;

        if entries.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No requests logged yet. Access logs are recorded as servers handle requests.",
            )]));
        }

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let mut out = format!("## Recent requests ({} shown)\n\n", entries.len());
        for e in &entries {
            let age = now_ms.saturating_sub(e.unix_ms) / 1000;
            out.push_str(&format!(
                "- **#{}** [{}s ago] server #{} ({}) — {} — {} → {}\n",
                e.id,
                age,
                e.server_id,
                e.protocol,
                e.event_type,
                summarize_request(&e.request),
                summarize_response(&e.response),
            ));
        }
        out.push_str("\nUse `get_access_log` with an id for the full request and response.");

        Ok(CallToolResult::success(vec![Content::text(out)]))
    }

    #[tool(description = "Get the full request and response for a single access-log entry by id (from list_access_logs)")]
    async fn get_access_log(
        &self,
        Parameters(params): Parameters<GetAccessLogParams>,
    ) -> Result<CallToolResult, McpError> {
        match self.state.app_state.get_access_log(params.id).await {
            Some(e) => {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let age = now_ms.saturating_sub(e.unix_ms) / 1000;
                let request = serde_json::to_string_pretty(&e.request)
                    .unwrap_or_else(|_| e.request.to_string());
                let response = serde_json::to_string_pretty(&e.response)
                    .unwrap_or_else(|_| "[]".to_string());
                let result = format!(
                    "## Access log #{}\n\n\
                     - **Server**: #{} ({})\n\
                     - **Connection**: {}\n\
                     - **Event**: {}\n\
                     - **When**: {}s ago (unix_ms {})\n\n\
                     ### Request\n```json\n{}\n```\n\n\
                     ### Response (actions)\n```json\n{}\n```\n",
                    e.id,
                    e.server_id,
                    e.protocol,
                    e.connection_id
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "(connectionless)".to_string()),
                    e.event_type,
                    age,
                    e.unix_ms,
                    request,
                    response,
                );
                Ok(CallToolResult::success(vec![Content::text(result)]))
            }
            None => Ok(CallToolResult::error(vec![Content::text(format!(
                "No access-log entry #{} (it may have aged out of the buffer). Use list_access_logs to see current entries.",
                params.id
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
                 create one. \
                 IMPORTANT — choose the handling mode deliberately: for deterministic \
                 behavior (echo servers, fixed/canned responses, simple routing, high \
                 throughput) pass `event_handlers` with a script or static handler, which \
                 run in-process with NO LLM call (instant, free, reproducible). Reserve the \
                 natural-language `instruction` (the LLM fallback, one model call per \
                 request) for responses that genuinely need reasoning or vary \
                 unpredictably. Use get_protocol_docs to see a protocol's event ids and \
                 actions before writing a handler.",
            )
    }

    fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<InitializeResult, McpError>> + Send + '_ {
        async move {
            info!(
                "MCP client '{}' v{} initialized",
                request.client_info.name, request.client_info.version,
            );

            // Retain peer info for the connection so rmcp can answer later requests
            if context.peer.peer_info().is_none() {
                context.peer.set_peer_info(request);
            }

            Ok(self.get_info())
        }
    }
}

/// Build a short one-line summary of a request (event data) for the log list.
fn summarize_request(request: &serde_json::Value) -> String {
    // HTTP-style requests: method + path
    let method = request.get("method").and_then(|v| v.as_str());
    let path = request.get("path").and_then(|v| v.as_str());
    if let (Some(m), Some(p)) = (method, path) {
        return format!("{} {}", m, p);
    }
    if let Some(p) = path {
        return p.to_string();
    }
    // Fall back to a compact, truncated form of the request JSON
    let compact = request.to_string();
    crate::utils::truncate_with_suffix(&compact, 79, "…")
}

/// Build a short one-line summary of the response action array for the log list.
fn summarize_response(response: &[serde_json::Value]) -> String {
    if response.is_empty() {
        return "(no response)".to_string();
    }
    let parts: Vec<String> = response
        .iter()
        .map(|a| {
            let ty = a.get("type").and_then(|v| v.as_str()).unwrap_or("action");
            // Surface an HTTP status code if present
            match a
                .get("status_code")
                .or_else(|| a.get("status"))
                .and_then(|v| v.as_u64())
            {
                Some(code) => format!("{}({})", ty, code),
                None => ty.to_string(),
            }
        })
        .collect();
    parts.join(", ")
}
