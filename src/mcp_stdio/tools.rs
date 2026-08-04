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
use crate::llm::agent_queue::LlmRequestQueue;
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
    /// Present only in `--llm-agent` mode: the queue protocol servers submit LLM
    /// requests to and the MCP tools answer from.
    agent_queue: Option<Arc<LlmRequestQueue>>,
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
    /// Network interface to bind, for interface-bound (layer 2 / raw socket)
    /// protocols such as `arp`, `datalink`, `icmp` and `isis` — e.g. "en0", "lo".
    /// Ignored by port-based protocols. Check get_protocol_docs: if the protocol
    /// is interface-bound its docs list `interface` instead of `port`.
    #[serde(default)]
    pub interface: Option<String>,
    /// Source MAC address for layer 2 protocols (e.g. "02:00:00:00:00:01").
    /// Only meaningful together with `interface`.
    #[serde(default)]
    pub mac_address: Option<String>,
    /// Make the server speak first on connect instead of waiting for the client
    /// — the FTP/SMTP-style greeting banner. Only protocols that declare a
    /// `send_first` startup parameter honour it (tcp, redis, postgresql, mysql,
    /// mongodb, jsonrpc, irc, ldap, …); for the rest it is ignored with a
    /// warning. The banner content comes from the handler for the protocol's
    /// "connection opened" event.
    #[serde(default)]
    pub send_first: Option<bool>,
    /// Seed the server's LLM memory before the first request. Only affects the
    /// `instruction` path; `event_handlers` never read memory.
    #[serde(default)]
    pub initial_memory: Option<String>,
    /// Natural-language instructions for the automatic feedback loop that
    /// adjusts this server's behaviour as it runs.
    #[serde(default)]
    pub feedback_instructions: Option<String>,
    /// Server-scoped scheduled LLM tasks, removed when the server stops. Array of
    /// objects: {"task_id":"...", "recurring":true, "interval_secs":60,
    /// "instruction":"..."} for a repeating task, or {"task_id":"...",
    /// "recurring":false, "delay_secs":10, "instruction":"..."} for a one-shot.
    /// Optional: "max_executions" (number), "context" (any JSON).
    #[serde(default)]
    pub scheduled_tasks: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StartClientParams {
    /// Protocol name. Use list_protocols with type "client" to see what is
    /// available in this build, and get_protocol_docs for a protocol's event ids,
    /// actions and startup parameters.
    pub protocol: String,
    /// Remote server to connect to, as "host:port" (e.g. "127.0.0.1:6379").
    pub remote_addr: String,
    /// Natural language instruction for the LLM that handles each event on this
    /// connection. This is the LLM FALLBACK path: every event triggers a (slow,
    /// billable) model call. Prefer `event_handlers` whenever the behavior is
    /// deterministic.
    #[serde(default)]
    pub instruction: Option<String>,
    /// Event handlers that decide how events are handled WITHOUT an LLM call.
    /// Same shape as start_server's: an array of
    /// { "event_pattern": "<event id>" | "*", "handler": {...} }, matched in
    /// order, first match wins; `handler` is one of {"type":"script",...},
    /// {"type":"static","actions":[...]} or {"type":"llm","instruction":"..."}.
    /// Use get_protocol_docs for the protocol's client event ids and action names.
    #[serde(default)]
    pub event_handlers: Option<Vec<serde_json::Value>>,
    /// Optional protocol-specific startup parameters (JSON object). Use
    /// get_protocol_docs for a protocol's available startup parameters.
    #[serde(default)]
    pub startup_params: Option<serde_json::Value>,
    /// Seed the client's LLM memory before the first event.
    #[serde(default)]
    pub initial_memory: Option<String>,
    /// Natural-language instructions for the automatic feedback loop that adjusts
    /// this client's behaviour as it runs.
    #[serde(default)]
    pub feedback_instructions: Option<String>,
    /// Client-scoped scheduled LLM tasks. Same shape as start_server's:
    /// {"task_id":"...", "recurring":true, "interval_secs":60, "instruction":"..."}
    /// or {"task_id":"...", "recurring":false, "delay_secs":10, "instruction":"..."}.
    #[serde(default)]
    pub scheduled_tasks: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StopClientParams {
    /// Client ID to stop
    #[serde(deserialize_with = "deserialize_u32_flexible")]
    pub client_id: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ClientStatusParams {
    /// Client ID to query
    #[serde(deserialize_with = "deserialize_u32_flexible")]
    pub client_id: u32,
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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetNextLlmRequestParams {
    /// Seconds to long-poll for a request when none is immediately pending
    /// (0 = return right away). Capped at 120 to stay under the client's tool-call timeout.
    #[serde(default, deserialize_with = "deserialize_option_u32_flexible")]
    pub wait_seconds: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AnswerLlmRequestParams {
    /// The request id returned by get_next_llm_request
    #[serde(deserialize_with = "deserialize_u64_flexible")]
    pub request_id: u64,
    /// NetGet actions to execute, as a JSON array. Each object has a `type` field
    /// naming the action plus its parameters (same shape shown by get_access_log).
    pub actions: Vec<serde_json::Value>,
}

/// Ensure a named pipe (FIFO) exists at `path`, creating it if absent.
#[cfg(unix)]
fn ensure_fifo(path: &std::path::Path) -> anyhow::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    if path.exists() {
        return Ok(());
    }
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| anyhow::anyhow!("invalid FIFO path: {}", path.display()))?;
    // 0o600: owner read/write only.
    let rc = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error())
            .map_err(|e| anyhow::anyhow!("failed to create FIFO {}: {}", path.display(), e));
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_fifo(_path: &std::path::Path) -> anyhow::Result<()> {
    anyhow::bail!("--llm-agent-pipe (named pipes) is only supported on Unix platforms")
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

        // Create app state
        let app_state = AppState::new();

        // Build the LLM client. Three mutually-exclusive sources:
        //   --llm-agent      → queue requests for the calling MCP agent (no model)
        //   --openai-url     → OpenAI-compatible endpoint
        //   (default)        → local Ollama
        let (llm_client, agent_queue) = if args.llm_agent {
            let pipe = args.llm_agent_pipe.clone();
            if let Some(ref path) = pipe {
                ensure_fifo(path)?;
            }
            let queue = Arc::new(LlmRequestQueue::new(pipe));
            let timeout = std::time::Duration::from_secs(args.llm_agent_timeout);
            let client = OllamaClient::new_queue(queue.clone(), timeout);

            // Pre-seed a placeholder model so `ensure_model_selected` never reaches
            // out to Ollama's /api/tags (no real model is involved in this mode).
            if args.model.is_none() {
                app_state.set_ollama_model(Some("agent".to_string())).await;
            }
            (client, Some(queue))
        } else {
            let client = crate::cli::create_llm_client(args, lock_enabled)?;
            (client, None)
        };

        // Status channel (messages go to stderr in MCP mode)
        let (status_tx, mut status_rx) = mpsc::unbounded_channel();

        // Set up model if specified
        if let Some(ref model) = args.model {
            app_state.set_ollama_model(Some(model.clone())).await;
        }

        let state = Arc::new(SharedState {
            app_state,
            llm_client,
            agent_queue,
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
            let mut protocols = registry.list_protocols();
            protocols.sort();
            result.push_str("## Client Protocols\n\n");
            result.push_str("Start one with `start_client` (needs a `remote_addr`).\n\n");
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
                        proto.description()
                    ));
                } else {
                    result.push_str(&format!("- **{}**\n", name));
                }
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

        let scheduled_tasks = match parse_scheduled_tasks(params.scheduled_tasks) {
            Ok(tasks) => tasks,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid scheduled_tasks: {}",
                    e
                ))]));
            }
        };

        let (status_tx, mut status_rx) = mpsc::unbounded_channel();
        let server_id = match crate::cli::server_startup::start_server_from_action(
            &self.state.app_state,
            params.mac_address.clone(),         // mac_address
            params.interface.clone(),           // interface
            params.host.clone(),                // host
            params.port,                        // port
            &params.protocol,                   // protocol
            params.send_first.unwrap_or(false), // send_first
            params.initial_memory,              // initial_memory
            instruction,                        // instruction
            params.startup_params,              // startup_params
            params.event_handlers,              // event_handlers
            scheduled_tasks,                    // scheduled_tasks
            params.feedback_instructions,       // feedback_instructions
            status_tx,                          // status_tx
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

    #[tool(description = "Connect a network protocol client to a remote server. The mirror of start_server: choose HOW it responds the same way — for DETERMINISTIC behavior pass `event_handlers` with a script or static handler (in-process, NO LLM call, instant and reproducible); only use the natural-language `instruction` when the decision genuinely needs reasoning, since it invokes NetGet's LLM on every event. Use get_protocol_docs for the protocol's client event ids, action names and startup parameters.")]
    async fn start_client(
        &self,
        Parameters(params): Parameters<StartClientParams>,
    ) -> Result<CallToolResult, McpError> {
        let instruction = params.instruction.unwrap_or_else(|| {
            format!(
                "You are a {} client connected to {}. Handle responses appropriately.",
                params.protocol, params.remote_addr
            )
        });

        let scheduled_tasks = match parse_scheduled_tasks(params.scheduled_tasks) {
            Ok(tasks) => tasks,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Invalid scheduled_tasks: {}",
                    e
                ))]));
            }
        };

        // Drive the new client with NetGet's configured LLM client, exactly as
        // start_server does (the client half reads it from the argument rather
        // than from AppState, but keep AppState in sync for anything that does).
        self.state
            .app_state
            .set_llm_client(self.state.llm_client.clone())
            .await;

        info!(
            "MCP: Connecting {} client to {}",
            params.protocol, params.remote_addr
        );

        let client_id = match crate::cli::client_startup::start_client_from_action(
            &self.state.app_state,
            &params.protocol,
            &params.remote_addr,
            instruction,
            params.startup_params,
            params.initial_memory,
            params.event_handlers,
            scheduled_tasks,
            params.feedback_instructions,
            self.state.llm_client.clone(),
        )
        .await
        {
            Ok(id) => id,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to start client: {}",
                    e
                ))]));
            }
        };

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Client #{} ({}) connected to {}.",
            client_id.as_u32(),
            params.protocol,
            params.remote_addr
        ))]))
    }

    #[tool(description = "Forget a client by its ID. LIMITATION — this does NOT actually stop the client: NetGet never stores a JoinHandle for a client's network task (there is no register_client_task(), unlike register_server_task() for servers), so the connection's read loop keeps running and keeps invoking the LLM after this call. All this does is drop the client from NetGet's state, after which it disappears from list_clients and client_status. To truly stop a client, stop the process. Servers do not have this problem: stop_server really stops them.")]
    async fn stop_client(
        &self,
        Parameters(params): Parameters<StopClientParams>,
    ) -> Result<CallToolResult, McpError> {
        let client_id = crate::state::ClientId::new(params.client_id);

        match self.state.app_state.remove_client(client_id).await {
            Some(client) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Client #{} ({} -> {}) removed from NetGet's state. \
                 Note: its network loop is NOT stopped — NetGet does not track \
                 client task handles, so the connection may still be live.",
                params.client_id, client.protocol_name, client.remote_addr
            ))])),
            None => Ok(CallToolResult::error(vec![Content::text(format!(
                "Client #{} not found",
                params.client_id
            ))])),
        }
    }

    #[tool(description = "List all clients with their protocol, remote address, status and instruction")]
    async fn list_clients(&self) -> Result<CallToolResult, McpError> {
        let clients = self.state.app_state.get_all_clients().await;

        if clients.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No clients currently connected.",
            )]));
        }

        let mut clients = clients;
        clients.sort_by_key(|c| c.id.as_u32());

        let mut result = String::from("## Clients\n\n");
        for client in &clients {
            result.push_str(&format!(
                "- **Client #{}**: {} -> {} ({})\n  Instruction: {}\n  Memory: {}\n\n",
                client.id.as_u32(),
                client.protocol_name,
                client.remote_addr,
                client.status,
                if client.instruction.is_empty() {
                    "(none)"
                } else {
                    &client.instruction
                },
                if client.memory.is_empty() {
                    "(empty)"
                } else {
                    &client.memory
                },
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(description = "Get detailed status of a specific client")]
    async fn client_status(
        &self,
        Parameters(params): Parameters<ClientStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        let client_id = crate::state::ClientId::new(params.client_id);

        match self.state.app_state.get_client(client_id).await {
            Some(client) => {
                let mut result = format!(
                    "## Client #{}\n\n\
                     - **Protocol**: {}\n\
                     - **Remote address**: {}\n\
                     - **Status**: {}\n\
                     - **Instruction**: {}\n\
                     - **Memory**: {}\n",
                    client.id.as_u32(),
                    client.protocol_name,
                    client.remote_addr,
                    client.status,
                    if client.instruction.is_empty() {
                        "(none)"
                    } else {
                        &client.instruction
                    },
                    if client.memory.is_empty() {
                        "(empty)"
                    } else {
                        &client.memory
                    },
                );

                if let Some(conn) = &client.connection {
                    result.push_str(&format!(
                        "- **Local address**: {}\n\
                         - **Connected address**: {}\n\
                         - **Bytes sent/received**: {}/{}\n\
                         - **Packets sent/received**: {}/{}\n",
                        conn.local_addr
                            .map(|a| a.to_string())
                            .unwrap_or_else(|| "(unknown)".to_string()),
                        conn.connected_addr
                            .map(|a| a.to_string())
                            .unwrap_or_else(|| "(unknown)".to_string()),
                        conn.bytes_sent,
                        conn.bytes_received,
                        conn.packets_sent,
                        conn.packets_received,
                    ));
                }

                Ok(CallToolResult::success(vec![Content::text(result)]))
            }
            None => Ok(CallToolResult::error(vec![Content::text(format!(
                "Client #{} not found",
                params.client_id
            ))])),
        }
    }

    #[tool(
        description = "Agent-LLM mode only (netget --llm-agent): fetch the next queued LLM request for a protocol server so YOU (the calling agent) can answer it in place of a model. Optionally long-poll with wait_seconds. Returns the request id, the prompt (server instruction + the triggering event), and the actions you may use. Reply by calling answer_llm_request with that id and a JSON array of actions. Returns '(no pending requests)' if none arrive within the wait."
    )]
    async fn get_next_llm_request(
        &self,
        Parameters(params): Parameters<GetNextLlmRequestParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(queue) = self.state.agent_queue.as_ref() else {
            return Ok(CallToolResult::error(vec![Content::text(
                "Agent-LLM mode is not enabled. Start netget with --llm-agent to queue LLM requests for the agent.",
            )]));
        };

        // Cap the long-poll to stay comfortably under typical MCP client tool-call timeouts.
        let wait = params.wait_seconds.unwrap_or(0).min(120);
        let req = queue
            .wait_and_claim(std::time::Duration::from_secs(wait as u64))
            .await;

        let Some(req) = req else {
            return Ok(CallToolResult::success(vec![Content::text(
                "(no pending requests)",
            )]));
        };

        // Render the prompt (system + user messages) and the available actions.
        let mut prompt = String::new();
        for m in &req.messages {
            prompt.push_str(&format!("### {}\n{}\n\n", m.role, m.content));
        }

        let actions_json = serde_json::to_string_pretty(&req.tools)
            .unwrap_or_else(|_| "[]".to_string());

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let age = now_ms.saturating_sub(req.created_unix_ms) / 1000;

        let result = format!(
            "## LLM request #{}\n\n\
             - **Queued**: {}s ago\n\n\
             ### Prompt\n{}\n\
             ### Available actions (tool schemas)\n```json\n{}\n```\n\n\
             To answer, call `answer_llm_request` with:\n\
             ```json\n{{ \"request_id\": {}, \"actions\": [ /* one or more action objects */ ] }}\n```\n\
             Each action object needs a `type` field naming the action, plus its parameters. \
             See `get_access_log` for worked examples of the action JSON a protocol expects.",
            req.id, age, prompt, actions_json, req.id,
        );

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[tool(
        description = "Agent-LLM mode only: answer a queued LLM request (from get_next_llm_request). `actions` is a JSON array of NetGet action objects — each with a `type` field and its parameters — that the protocol server will execute (e.g. [{\"type\":\"send_tcp_data\",\"data\":\"...\"}]). This unblocks the waiting connection."
    )]
    async fn answer_llm_request(
        &self,
        Parameters(params): Parameters<AnswerLlmRequestParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(queue) = self.state.agent_queue.as_ref() else {
            return Ok(CallToolResult::error(vec![Content::text(
                "Agent-LLM mode is not enabled. Start netget with --llm-agent.",
            )]));
        };

        match queue.answer(params.request_id, params.actions) {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Answered LLM request #{}.",
                params.request_id
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
        }
    }

    #[tool(
        description = "Agent-LLM mode only: list outstanding queued LLM requests (pending and claimed-but-unanswered) for monitoring."
    )]
    async fn list_llm_requests(&self) -> Result<CallToolResult, McpError> {
        let Some(queue) = self.state.agent_queue.as_ref() else {
            return Ok(CallToolResult::error(vec![Content::text(
                "Agent-LLM mode is not enabled. Start netget with --llm-agent.",
            )]));
        };

        let pending = queue.list();
        if pending.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No outstanding LLM requests.",
            )]));
        }

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let mut out = String::from("## Outstanding LLM requests\n\n");
        for r in &pending {
            let age = now_ms.saturating_sub(r.created_unix_ms) / 1000;
            out.push_str(&format!(
                "- **#{}** — {} — queued {}s ago\n",
                r.id,
                if r.claimed { "claimed" } else { "pending" },
                age
            ));
        }
        Ok(CallToolResult::success(vec![Content::text(out)]))
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
                 actions before writing a handler.\n\n\
                 CLIENTS: start_client / list_clients / client_status / stop_client are \
                 the mirror of the server tools, for connecting OUT to a remote server \
                 instead of listening. They take the same instruction and event_handlers \
                 arguments plus a remote_addr. Caveat: stop_client only drops the client \
                 from NetGet's state — it does not stop the connection's network loop.\n\n\
                 AGENT-LLM MODE (when netget was started with --llm-agent): there is no \
                 model — YOU answer the LLM calls. When a server needs a reasoned \
                 response it queues a request; fetch it with get_next_llm_request \
                 (optionally long-poll via wait_seconds), then reply with \
                 answer_llm_request(request_id, actions). If --llm-agent-pipe was set, a \
                 line with the new request id is written to that FIFO on each enqueue so \
                 you can block-read it instead of polling. Unanswered requests error out \
                 after the configured timeout. list_llm_requests shows what is outstanding.",
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

/// Parse the MCP `scheduled_tasks` argument into typed task definitions.
///
/// Kept as raw JSON on the wire because `ServerTaskDefinition` does not derive
/// `JsonSchema`; a bad entry is reported to the caller rather than silently
/// dropped.
fn parse_scheduled_tasks(
    tasks: Option<Vec<serde_json::Value>>,
) -> Result<Option<Vec<crate::llm::actions::common::ServerTaskDefinition>>, String> {
    let Some(tasks) = tasks else {
        return Ok(None);
    };
    if tasks.is_empty() {
        return Ok(None);
    }
    let mut parsed = Vec::with_capacity(tasks.len());
    for (i, task) in tasks.into_iter().enumerate() {
        match serde_json::from_value::<crate::llm::actions::common::ServerTaskDefinition>(task) {
            Ok(def) => parsed.push(def),
            Err(e) => {
                return Err(format!(
                    "entry {}: {}. Each entry needs \"task_id\" (string), \"recurring\" (boolean) \
                     and \"instruction\" (string); recurring tasks also take \"interval_secs\", \
                     one-shot tasks \"delay_secs\".",
                    i, e
                ));
            }
        }
    }
    Ok(Some(parsed))
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
