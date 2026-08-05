//! Command-line argument parsing

use anyhow::Result;
use clap::Parser;
use std::io::{self, IsTerminal, Read};
use tracing::Level;

/// Get default log level based on build type
/// Development builds (debug_assertions) default to "debug"
/// Release builds default to "info"
///
/// Dev builds used to default to `trace`. TRACE is the level at which NetGet
/// writes whole network payloads and whole LLM prompts/responses to
/// `netget.log` - which is both a 481 MB-per-day disk problem and a disclosure
/// one, since those payloads can carry credentials from the wire. DEBUG keeps
/// the per-event summaries that make development logs useful and stops short of
/// the payloads; `--log-level trace` still turns them back on for the sessions
/// that genuinely need byte-level detail.
fn default_log_level() -> String {
    if cfg!(debug_assertions) {
        "debug".to_string()
    } else {
        "info".to_string()
    }
}

/// NetGet - LLM-Controlled Network Application
#[derive(Parser, Debug)]
#[clap(
    author,
    version,
    about,
    long_about = "NetGet - LLM-Controlled Network Application\n\n\
                  NetGet is an AI-powered network tool where an LLM controls network protocols.\n\
                  It can operate in interactive mode (with TUI) or non-interactive mode.",
    after_help = "EXAMPLES:\n\
                  \n\
                  Interactive mode (TUI):\n\
                      netget\n\
                  \n\
                  Non-interactive with prompt (no quotes needed):\n\
                      netget listen on port 80 via http\n\
                      netget \"listen on port 80 via http\"     # quoted version\n\
                      cat prompt.txt | netget\n\
                  \n\
                  Specify model with prompt after --:\n\
                      netget -m llama3.2:latest -- listen on port 80\n\
                      netget --model deepseek-coder:latest show version\n\
                  \n\
                  Specify scripting environment:\n\
                      netget -e python listen on port 80\n\
                      netget --env javascript -- start http server\n\
                      netget --env llm show version\n\
                  \n\
                  Server configuration:\n\
                      netget --listen-addr 0.0.0.0 listen on port 8080\n\
                  \n\
                  Connect a client without a model round-trip:\n\
                      netget --client redis --connect 127.0.0.1:6379\n\
                      netget --client tcp --connect 10.0.0.5:9000 log every banner you receive\n\
                      netget --client-list",
    trailing_var_arg = true,
    allow_hyphen_values = true
)]
pub struct Args {
    /// LLM model to use (e.g., "llama3.2:latest", "deepseek-coder:latest")
    #[clap(short = 'm', long = "model", value_name = "MODEL")]
    pub model: Option<String>,

    /// Log level (off, error, warn, info, debug, trace)
    /// Development builds default to 'debug', release builds default to 'info'
    /// ('trace' additionally writes full network payloads and LLM prompts to netget.log)
    #[clap(
        short = 'l',
        long = "log-level",
        value_name = "LEVEL",
        default_value_t = default_log_level()
    )]
    pub log_level: String,

    /// Scripting environment to use (on, off, python, javascript, go, perl)
    #[clap(
        short = 'e',
        long = "env",
        value_name = "ENVIRONMENT",
        help = "Scripting environment: on (LLM chooses runtime), off (LLM only mode), python (Python scripting), javascript (JavaScript scripting), go (Go scripting), perl (Perl scripting)"
    )]
    pub scripting_env: Option<String>,

    /// Event handler mode to use (any, script, static, llm)
    #[clap(
        long = "handler",
        value_name = "MODE",
        help = "Event handler mode: any (LLM chooses handler types), script (force script handlers), static (force static responses), llm (force LLM handlers)"
    )]
    pub event_handler_mode: Option<String>,

    /// Listen address for servers (default: 127.0.0.1)
    #[clap(
        long = "listen-addr",
        value_name = "ADDRESS",
        help = "IP address to bind servers to (e.g., 127.0.0.1, 0.0.0.0)"
    )]
    pub listen_addr: Option<String>,

    /// Include disabled protocols (for testing honeypot-only protocols like IPSec, OpenVPN)
    #[clap(
        long = "include-disabled-protocols",
        help = "Includes experimental protocols for testing purposes"
    )]
    pub include_disabled_protocols: bool,

    /// Use file locking to serialize Ollama API access (enables concurrent test execution)
    #[clap(
        long = "ollama-lock",
        help = "Enable file-based locking for Ollama API access. This prevents concurrent requests from overloading the LLM, allowing multiple NetGet instances to run safely in parallel. The lock file is created at ./ollama.lock in the current directory."
    )]
    pub ollama_lock: bool,

    /// Ollama API base URL (default: http://localhost:11434)
    #[clap(
        long = "ollama-url",
        value_name = "URL",
        help = "Base URL for Ollama API (default: http://localhost:11434). Use this to point to a custom Ollama instance or mock server for testing.",
        conflicts_with = "openai_url",
        hide = true  // Hidden from help output - primarily for testing
    )]
    pub ollama_url: Option<String>,

    /// OpenAI-compatible API base URL (e.g., "https://api.openai.com", "http://localhost:1234")
    #[clap(
        long = "openai-url",
        value_name = "URL",
        help = "Base URL for an OpenAI-compatible API endpoint (e.g., https://api.openai.com, http://localhost:1234 for vLLM/LM Studio). Requires --model and --api-key (or NETGET_API_KEY/OPENAI_API_KEY env var).",
        conflicts_with = "ollama_url"
    )]
    pub openai_url: Option<String>,

    /// API key for OpenAI-compatible endpoint
    #[clap(
        long = "api-key",
        value_name = "KEY",
        help = "API key for the OpenAI-compatible endpoint. Also reads NETGET_API_KEY or OPENAI_API_KEY environment variables."
    )]
    pub api_key: Option<String>,

    /// Route all LLM calls to the calling MCP agent instead of a model (MCP mode only)
    #[clap(
        long = "llm-agent",
        help = "Do not call any model. Queue every LLM request for the calling MCP agent (e.g. Claude Code) to answer via the get_next_llm_request / answer_llm_request tools. Only meaningful with --mcp / --mcp-http; mutually exclusive with --ollama-url / --openai-url.",
        conflicts_with_all = ["ollama_url", "openai_url"]
    )]
    pub llm_agent: bool,

    /// FIFO path for agent-queue push notifications
    #[clap(
        long = "llm-agent-pipe",
        value_name = "PATH",
        help = "Named pipe (FIFO) that receives the id of each newly queued LLM request, so the agent can block-read it to get woken instead of polling. Created if absent. Requires --llm-agent; the long-poll tool works without it.",
        requires = "llm_agent"
    )]
    pub llm_agent_pipe: Option<std::path::PathBuf>,

    /// Seconds a queued LLM request waits for the agent's answer before erroring
    #[clap(
        long = "llm-agent-timeout",
        value_name = "SECONDS",
        default_value = "300",
        help = "How long (seconds) a queued LLM request waits for the agent to answer before the call errors and the connection resets to Idle. Requires --llm-agent.",
        requires = "llm_agent"
    )]
    pub llm_agent_timeout: u64,

    /// Path to embedded GGUF model file (enables embedded LLM inference)
    #[cfg(feature = "embedded-llm")]
    #[clap(
        long = "embedded-model",
        value_name = "PATH",
        help = "Path to GGUF model file for embedded inference (e.g., mistral-7b.Q4_K_M.gguf). When specified, NetGet will use embedded llama.cpp instead of or alongside Ollama."
    )]
    pub embedded_model: Option<std::path::PathBuf>,

    /// Force use of embedded LLM backend (skip Ollama)
    #[cfg(feature = "embedded-llm")]
    #[clap(
        long = "use-embedded",
        help = "Use embedded LLM backend exclusively, skipping Ollama health check. Requires --embedded-model to be set or configured in ~/.netget/config.toml"
    )]
    pub use_embedded: bool,

    /// Terminal color theme (auto, light, dark, neutral)
    #[clap(
        long = "theme",
        value_name = "THEME",
        default_value = "auto",
        help = "Color theme for TUI: auto (detect background), light (dark colors on light background), dark (bright colors on dark background), neutral (medium contrast for both)"
    )]
    pub theme: String,

    /// Suppress ASCII art banner on startup
    #[clap(
        long = "suppress-art",
        help = "Skip the Ollama-generated ASCII art banner on startup"
    )]
    pub suppress_art: bool,

    /// Maximum concurrent LLM requests (default: 1)
    #[clap(
        long = "llm-max-concurrent",
        value_name = "NUM",
        help = "Maximum number of concurrent LLM requests. Default: 1 for sequential processing. Increase for higher throughput with local Ollama."
    )]
    pub llm_max_concurrent: Option<usize>,

    /// Token limit per time window (optional, for cloud API usage control)
    #[clap(
        long = "llm-token-limit",
        value_name = "TOKENS",
        help = "Maximum tokens (input + output) per time window. Use for cloud API rate limiting. No limit by default (suitable for local Ollama)."
    )]
    pub llm_token_limit: Option<u64>,

    /// Token limit time window in seconds (default: 60)
    #[clap(
        long = "llm-token-window",
        value_name = "SECONDS",
        default_value = "60",
        help = "Time window in seconds for token limit enforcement."
    )]
    pub llm_token_window: u64,

    /// Disable scripting (LLM will only use actions, no script generation)
    #[clap(
        long = "no-scripts",
        help = "Disable script generation for tests that need action-only responses",
        hide = true  // Hidden from help output - primarily for testing
    )]
    pub no_scripts: bool,

    /// Load server/client configuration from a .netget file
    #[clap(
        long = "load",
        value_name = "FILE",
        help = "Load and execute server/client configurations from a .netget file"
    )]
    pub load_file: Option<String>,

    /// Path to mock LLM configuration file (for testing)
    #[clap(
        long = "mock-config-file",
        value_name = "FILE",
        help = "Path to JSON file containing mock LLM responses (used by tests)",
        hide = true  // Hidden from help output - internal testing flag
    )]
    pub mock_config_file: Option<std::path::PathBuf>,

    /// Start a simple protocol (simplified LLM interface for "dumb" models)
    #[clap(
        long = "simple",
        value_name = "PROTOCOL",
        help = "Start a simple protocol server (e.g., http). Use --simple-list to see available protocols."
    )]
    pub simple_protocol: Option<String>,

    /// List available simple protocols
    #[clap(
        long = "simple-list",
        help = "List available simple protocols that can be started with --simple"
    )]
    pub simple_list: bool,

    /// Connect a protocol client to a remote server (non-interactive)
    #[clap(
        long = "client",
        value_name = "PROTOCOL",
        help = "Connect a client of PROTOCOL to the address given by --connect, without asking the model to do it. Any trailing text is used as the client's instruction. Use --client-list to see available protocols."
    )]
    pub client_protocol: Option<String>,

    /// Remote address for --client
    #[clap(
        long = "connect",
        value_name = "ADDRESS",
        requires = "client_protocol",
        help = "Remote address the --client connects to, e.g. 127.0.0.1:6379"
    )]
    pub client_addr: Option<String>,

    /// Startup parameters for --client
    #[clap(
        long = "client-params",
        value_name = "JSON",
        requires = "client_protocol",
        help = "JSON object of startup parameters for --client (same keys as the MCP start_client tool's startup_params)"
    )]
    pub client_params: Option<String>,

    /// Event handlers for --client
    #[clap(
        long = "client-handlers",
        value_name = "JSON",
        requires = "client_protocol",
        help = "JSON array of event handlers for --client (script or static handlers run in-process with NO LLM call, which is what makes a scripted client deterministic)"
    )]
    pub client_handlers: Option<String>,

    /// List available client protocols
    #[clap(
        long = "client-list",
        help = "List protocols that can be connected as clients with --client"
    )]
    pub client_list: bool,

    /// Run as MCP STDIO server (for Claude Desktop/Code integration)
    #[clap(
        long = "mcp",
        alias = "mcp-stdio",
        help = "Run as an MCP server over stdin/stdout for integration with MCP clients like Claude Desktop"
    )]
    pub mcp_stdio: bool,

    /// Run as an MCP server over HTTP/SSE on the given port (for remote/web MCP clients)
    #[clap(
        long = "mcp-http",
        value_name = "PORT",
        conflicts_with = "mcp_stdio",
        help = "Run as an MCP server over HTTP/SSE on the given port (e.g. --mcp-http 8080). Bind address comes from --listen-addr (default 127.0.0.1)"
    )]
    pub mcp_http: Option<u16>,

    /// Prompt/command to execute (can be specified after --, or as trailing args, or via stdin)
    #[clap(value_name = "PROMPT", num_args = 0..)]
    pub prompt: Vec<String>,
}

impl Args {
    /// Resolve the API key from --api-key flag, NETGET_API_KEY, or OPENAI_API_KEY env vars
    pub fn resolve_api_key(&self) -> Option<String> {
        self.api_key
            .clone()
            .or_else(|| std::env::var("NETGET_API_KEY").ok())
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
    }

    /// Get the effective log level from --log-level flag
    pub fn effective_log_level(&self) -> Level {
        match self.log_level.to_lowercase().as_str() {
            "off" | "none" => Level::ERROR, // We'll filter this out separately
            "error" => Level::ERROR,
            "warn" | "warning" => Level::WARN,
            "info" => Level::INFO,
            "debug" => Level::DEBUG,
            "trace" => Level::TRACE,
            _ => Level::ERROR,
        }
    }

    /// Check if logging should be disabled entirely
    pub fn logging_disabled(&self) -> bool {
        self.log_level == "off" || self.log_level == "none"
    }

    /// Determine if we should run in interactive mode
    pub fn is_interactive(&self) -> bool {
        // Non-interactive if we have a prompt from args
        if !self.prompt.is_empty() {
            return false;
        }

        // Non-interactive if stdin is not a terminal (piped input)
        if !io::stdin().is_terminal() {
            return false;
        }

        // Non-interactive if stdout is not a terminal (piped output)
        // This ensures we don't try to show TUI when output is redirected
        if !io::stdout().is_terminal() {
            return false;
        }

        // Otherwise, run in interactive mode
        true
    }

    /// Get the prompt to execute, from various sources
    /// Returns None if the input should be treated as actions JSON instead
    pub fn get_prompt(&self) -> Result<Option<String>> {
        // First priority: --load flag (will be handled separately)
        if self.load_file.is_some() {
            return Ok(None);
        }

        // Second priority: trailing arguments after command
        if !self.prompt.is_empty() {
            let joined = self.prompt.join(" ");
            // Check if it's actions JSON instead of a prompt
            if crate::utils::save_load::is_actions_json(&joined) {
                // This will be handled by get_actions_json() instead
                return Ok(None);
            }
            return Ok(Some(joined));
        }

        // Third priority: stdin if not a terminal (piped/redirected input)
        if !io::stdin().is_terminal() {
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer)?;
            let trimmed = buffer.trim();
            if !trimmed.is_empty() {
                // Check if it's actions JSON instead of a prompt
                if crate::utils::save_load::is_actions_json(trimmed) {
                    // This will be handled by get_actions_json() instead
                    return Ok(None);
                }
                return Ok(Some(trimmed.to_string()));
            }
        }

        // No prompt available
        Ok(None)
    }

    /// Get actions JSON to execute, from various sources
    /// Returns None if the input is a regular prompt or no input
    pub fn get_actions_json(&self) -> Result<Option<Vec<serde_json::Value>>> {
        use crate::utils::save_load;

        // First priority: --load flag
        if let Some(ref filename) = self.load_file {
            // This will fail if file doesn't exist, which is appropriate
            return Ok(Some(
                tokio::runtime::Runtime::new()?.block_on(save_load::load_actions(filename))?,
            ));
        }

        // Second priority: trailing arguments after command
        if !self.prompt.is_empty() {
            let joined = self.prompt.join(" ");
            if save_load::is_actions_json(&joined) {
                // Parse {"actions": [...]} format and extract the array
                let parsed: serde_json::Value = serde_json::from_str(&joined)?;
                let actions = parsed["actions"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("Invalid actions format"))?
                    .clone();
                return Ok(Some(actions));
            }
        }

        // Third priority: stdin if not a terminal (piped/redirected input)
        if !io::stdin().is_terminal() {
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer)?;
            let trimmed = buffer.trim();
            if !trimmed.is_empty() && save_load::is_actions_json(trimmed) {
                // Parse {"actions": [...]} format and extract the array
                let parsed: serde_json::Value = serde_json::from_str(trimmed)?;
                let actions = parsed["actions"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("Invalid actions format"))?
                    .clone();
                return Ok(Some(actions));
            }
        }

        // No actions JSON available
        Ok(None)
    }

    /// Check if the environment is suitable for the requested mode
    pub fn validate_mode(&self) -> Result<()> {
        if self.is_interactive() {
            // Interactive mode requires a terminal
            if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
                anyhow::bail!(
                    "Cannot start in interactive mode without a terminal.\n\
                     Please provide a prompt via arguments, stdin, or use --non-interactive."
                );
            }
        } else {
            // Non-interactive mode requires a prompt
            if self.get_prompt()?.is_none() {
                anyhow::bail!(
                    "Non-interactive mode requires a prompt.\n\
                     Provide a prompt via arguments, stdin, or use interactive mode."
                );
            }
        }
        Ok(())
    }

    /// Parse the scripting environment argument into a ScriptingMode
    pub fn parse_scripting_mode(&self) -> Result<Option<crate::state::app_state::ScriptingMode>> {
        // --no-scripts flag takes precedence
        if self.no_scripts {
            return Ok(Some(crate::state::app_state::ScriptingMode::Off));
        }

        match &self.scripting_env {
            None => Ok(None),
            Some(env) => {
                let mode = match env.to_lowercase().as_str() {
                    "on" | "auto" => crate::state::app_state::ScriptingMode::On,
                    "off" | "llm" => crate::state::app_state::ScriptingMode::Off,
                    "python" | "py" => crate::state::app_state::ScriptingMode::Python,
                    "javascript" | "js" | "node" => {
                        crate::state::app_state::ScriptingMode::JavaScript
                    }
                    "go" | "golang" => crate::state::app_state::ScriptingMode::Go,
                    "perl" => crate::state::app_state::ScriptingMode::Perl,
                    _ => {
                        anyhow::bail!(
                            "Invalid scripting environment: '{}'\n\
                             Valid options: on (auto), off (llm), python (py), javascript (js, node), go (golang), perl",
                            env
                        );
                    }
                };
                Ok(Some(mode))
            }
        }
    }

    /// Parse the event handler mode argument into an EventHandlerMode
    pub fn parse_event_handler_mode(
        &self,
    ) -> Result<Option<crate::state::app_state::EventHandlerMode>> {
        match &self.event_handler_mode {
            None => Ok(None),
            Some(mode) => {
                let parsed_mode = match mode.to_lowercase().as_str() {
                    "any" => crate::state::app_state::EventHandlerMode::Any,
                    "script" => crate::state::app_state::EventHandlerMode::Script,
                    "static" => crate::state::app_state::EventHandlerMode::Static,
                    "llm" => crate::state::app_state::EventHandlerMode::Llm,
                    _ => {
                        anyhow::bail!(
                            "Invalid event handler mode: '{}'\n\
                             Valid options: any, script, static, llm",
                            mode
                        );
                    }
                };
                Ok(Some(parsed_mode))
            }
        }
    }

    /// Instruction for a `--client`: the trailing text if any was given,
    /// otherwise the same default the MCP `start_client` tool uses.
    pub fn client_instruction(&self, protocol: &str, remote_addr: &str) -> String {
        let trailing = self.prompt.join(" ").trim().to_string();
        if trailing.is_empty() {
            format!(
                "You are a {} client connected to {}. Handle responses appropriately.",
                protocol, remote_addr
            )
        } else {
            trailing
        }
    }

    /// Parse `--client-params` into a JSON object
    pub fn parse_client_params(&self) -> Result<Option<serde_json::Value>> {
        match &self.client_params {
            None => Ok(None),
            Some(raw) => {
                let value: serde_json::Value = serde_json::from_str(raw)
                    .map_err(|e| anyhow::anyhow!("--client-params is not valid JSON: {}", e))?;
                if !value.is_object() {
                    anyhow::bail!("--client-params must be a JSON object, e.g. '{{\"db\": 0}}'");
                }
                Ok(Some(value))
            }
        }
    }

    /// Parse `--client-handlers` into a list of event handler definitions
    pub fn parse_client_handlers(&self) -> Result<Option<Vec<serde_json::Value>>> {
        match &self.client_handlers {
            None => Ok(None),
            Some(raw) => {
                let value: serde_json::Value = serde_json::from_str(raw)
                    .map_err(|e| anyhow::anyhow!("--client-handlers is not valid JSON: {}", e))?;
                match value {
                    serde_json::Value::Array(handlers) => Ok(Some(handlers)),
                    _ => anyhow::bail!(
                        "--client-handlers must be a JSON array of handler objects, e.g. '[{{\"event\": \"redis_response_received\", \"handler_type\": \"static\", \"response\": []}}]'"
                    ),
                }
            }
        }
    }

    /// Build a RateLimiterConfig from CLI arguments
    pub fn build_rate_limiter_config(&self) -> crate::llm::RateLimiterConfig {
        crate::llm::RateLimiterConfig {
            max_concurrent: self.llm_max_concurrent.unwrap_or(1),
            token_limit: self.llm_token_limit,
            token_window_secs: self.llm_token_window,
        }
    }
}
