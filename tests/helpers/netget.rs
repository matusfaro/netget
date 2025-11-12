// Core NetGet startup and parsing functionality

use netget::protocol::server_registry;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::timeout;

use super::common::*;

/// Represents a running NetGet process with 0+ servers and 0+ clients
pub struct NetGetInstance {
    /// The child process
    pub child: Child,
    /// Servers that were started
    pub servers: Vec<NetGetServer>,
    /// Clients that were started
    pub clients: Vec<NetGetClient>,
    /// Captured output lines (for verification)
    pub output_lines: std::sync::Arc<tokio::sync::Mutex<Vec<String>>>,
    /// Mock configuration (for verification)
    pub mock_config: Option<netget::testing::MockLlmConfig>,
}

/// Information about a server that was started
#[derive(Clone, Debug)]
pub struct NetGetServer {
    /// Server ID (e.g., "1")
    pub id: String,
    /// The port the server is listening on
    pub port: u16,
    /// The actual protocol stack that was started
    pub stack: String,
}

/// Information about a client that was started
#[derive(Clone, Debug)]
pub struct NetGetClient {
    /// Client ID (e.g., "1")
    pub id: String,
    /// Protocol name (e.g., "TCP", "HTTP")
    pub protocol: String,
    /// Remote address being connected to
    pub remote_addr: String,
    /// Local address (after connection succeeds)
    pub local_addr: Option<String>,
}

/// Configuration for starting netget (servers + clients)
///
/// ## Log Capture
///
/// By default, tests capture logs at "debug" level, which includes:
/// - LLM interaction summaries (but not full prompts/responses)
/// - Protocol-level summaries
/// - Connection lifecycle events
///
/// For debugging test failures, use `.with_log_level("trace")`
/// to capture full LLM prompts/responses and protocol wire data.
///
/// All logs (stdout + stderr) are captured to `output_lines` and accessible via:
/// - `get_output()` - Get all captured lines
/// - `output_contains("needle")` - Check if output contains text
/// - `count_in_output("pattern")` - Count occurrences
pub struct NetGetConfig {
    /// The prompt to send to the binary
    pub prompt: String,
    /// Optional model override
    pub model: Option<String>,
    /// Log level (default: "debug")
    ///
    /// Available levels:
    /// - "off" - No logging (fastest, but no debugging info)
    /// - "error" - Only critical errors
    /// - "warn" - Warnings and errors
    /// - "info" - Lifecycle events, warnings, and errors
    /// - "debug" - Default. Includes LLM/protocol summaries (recommended for most tests)
    /// - "trace" - Full detail including LLM prompts/responses and wire data (for debugging)
    pub log_level: String,
    /// Listen address (default: "127.0.0.1")
    pub listen_addr: String,
    /// Disable script generation (default: false)
    pub no_scripts: bool,
    /// Include disabled protocols (default: false)
    pub include_disabled_protocols: bool,
    /// Enable Ollama lock for concurrent test execution (default: true)
    pub ollama_lock: bool,
    /// Mock LLM configuration (for testing without Ollama)
    pub mock_config: Option<netget::testing::MockLlmConfig>,
}

/// Test mode for E2E tests
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestMode {
    /// Only use real Ollama (fail if unavailable)
    Real,
    /// Only use mocks (fail if no mocks configured)
    Mock,
    /// Prefer mock, fall back to Ollama (default)
    Auto,
}

impl TestMode {
    /// Detect mode from environment
    pub fn detect() -> Self {
        match std::env::var("NETGET_TEST_MODE").as_deref() {
            Ok("real") => TestMode::Real,
            Ok("mock") => TestMode::Mock,
            Ok("auto") => TestMode::Auto,
            Ok(other) => {
                eprintln!("⚠️  Unknown NETGET_TEST_MODE: '{}', using Auto", other);
                TestMode::Auto
            }
            Err(_) => TestMode::Auto, // Default
        }
    }
}

impl NetGetConfig {
    /// Create a new config with the given prompt
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            model: None,
            log_level: "debug".to_string(),
            listen_addr: "127.0.0.1".to_string(),
            no_scripts: false,
            include_disabled_protocols: false,
            ollama_lock: true, // Enable by default for concurrent testing
            mock_config: None,
        }
    }

    /// Create a new config with scripts disabled
    pub fn new_no_scripts(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            model: None,
            log_level: "debug".to_string(),
            listen_addr: "127.0.0.1".to_string(),
            no_scripts: true,
            include_disabled_protocols: false,
            ollama_lock: true,
            mock_config: None,
        }
    }

    /// Create a new config with trace logging enabled (for debugging test failures)
    ///
    /// Trace logging includes:
    /// - Full LLM prompts and responses
    /// - Protocol wire data (hex dumps, request/response bodies)
    /// - Detailed state transitions
    ///
    /// Use this when debugging LLM behavior or protocol-level issues.
    pub fn new_with_trace(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            model: None,
            log_level: "trace".to_string(),
            listen_addr: "127.0.0.1".to_string(),
            no_scripts: false,
            include_disabled_protocols: false,
            ollama_lock: true,
            mock_config: None,
        }
    }

    /// Set the model to use
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the log level
    pub fn with_log_level(mut self, level: impl Into<String>) -> Self {
        self.log_level = level.into();
        self
    }

    /// Set the listen address
    pub fn with_listen_addr(mut self, addr: impl Into<String>) -> Self {
        self.listen_addr = addr.into();
        self
    }

    /// Disable script generation
    pub fn with_no_scripts(mut self, no_scripts: bool) -> Self {
        self.no_scripts = no_scripts;
        self
    }

    /// Include disabled protocols (for testing honeypot-only protocols like IPSec, OpenVPN)
    pub fn with_include_disabled_protocols(mut self, include_disabled: bool) -> Self {
        self.include_disabled_protocols = include_disabled;
        self
    }

    /// Enable or disable Ollama lock for concurrent testing
    pub fn with_ollama_lock(mut self, enabled: bool) -> Self {
        self.ollama_lock = enabled;
        self
    }

    /// Configure mock LLM responses (for testing without Ollama)
    ///
    /// # Example
    /// ```ignore
    /// use netget::testing::MockLlmBuilder;
    ///
    /// let config = NetGetConfig::new("Start TCP server on port 0")
    ///     .with_mock(|mock| {
    ///         mock.on_event("tcp_connection_received")
    ///             .respond_with_actions(json!([
    ///                 {"type": "send_tcp_data", "data": "48656c6c6f"}
    ///             ]))
    ///             .expect_calls(1)
    ///     });
    /// ```
    pub fn with_mock<F>(mut self, builder_fn: F) -> Self
    where
        F: FnOnce(netget::testing::MockLlmBuilder) -> netget::testing::MockLlmBuilder,
    {
        let builder = netget::testing::MockLlmBuilder::new();
        self.mock_config = Some(builder_fn(builder).build());
        self
    }
}

/// Start a NetGet instance with the given configuration
/// Returns instance with 0+ servers and 0+ clients
pub async fn start_netget(config: NetGetConfig) -> E2EResult<NetGetInstance> {
    // Detect test mode
    let mode = TestMode::detect();
    let has_mocks = config.mock_config.is_some();

    // Enforce mode requirements
    match mode {
        TestMode::Real => {
            if has_mocks {
                println!("⚠️  Real mode: Ignoring configured mocks");
            }
            // Check Ollama availability
            if !check_ollama_available().await {
                return Err("Real mode requires Ollama, but Ollama is not available at http://localhost:11434".into());
            }
            println!("🤖 Real mode: Using Ollama");
            // Clear any mock environment variables
            std::env::remove_var("NETGET_MOCK_CONFIG_JSON");
        }
        TestMode::Mock => {
            if !has_mocks {
                return Err("Mock mode requires mocks to be configured via .with_mock()".into());
            }
            println!("🔧 Mock mode: Using configured mocks");
            // Set mock environment variable
            let config_json = serde_json::to_string(&config.mock_config)?;
            std::env::set_var("NETGET_MOCK_CONFIG_JSON", config_json);
        }
        TestMode::Auto => {
            if has_mocks {
                println!("🔧 Auto mode: Using mocks (available)");
                // Set mock environment variable
                let config_json = serde_json::to_string(&config.mock_config)?;
                std::env::set_var("NETGET_MOCK_CONFIG_JSON", config_json);
            } else if check_ollama_available().await {
                println!("🤖 Auto mode: Using real Ollama (no mocks configured)");
            } else {
                return Err("Auto mode: No mocks configured and Ollama not available at http://localhost:11434".into());
            }
        }
    }

    // Get the path to the binary
    let binary_path = get_netget_binary_path()?;

    // Replace {AVAILABLE_PORT} placeholders with actual available ports
    let processed_prompt = replace_port_placeholders(&config.prompt).await?;

    // Build command arguments
    let mut cmd = Command::new(binary_path);

    // Add model flag if specified, otherwise use the default
    if let Some(model) = &config.model {
        cmd.arg("--model").arg(model);
    } else {
        // Use the same default as the application
        // Note: Proxy tests require a capable model like qwen3-coder:30b
        cmd.arg("--model").arg("qwen3-coder:30b");
    }

    // Add log level
    cmd.arg("--log-level").arg(&config.log_level);

    // Add listen address
    cmd.arg("--listen-addr").arg(&config.listen_addr);

    // Add --no-scripts flag if enabled
    if config.no_scripts {
        cmd.arg("--no-scripts");
    }

    // Add --include-disabled-protocols flag if enabled
    if config.include_disabled_protocols {
        cmd.arg("--include-disabled-protocols");
    }

    // Add --ollama-lock flag if enabled (default: true for concurrent testing)
    if config.ollama_lock {
        cmd.arg("--ollama-lock");
    }

    // Add the prompt as a single argument (for non-interactive mode)
    // The binary will receive this as a single command-line argument
    cmd.arg(&processed_prompt);

    // Configure process
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    // Debug: print the command being executed
    println!("[DEBUG] Executing: {:?}", cmd);

    // Start the process
    let mut child = cmd.spawn()?;

    // Get both stdout and stderr for reading output
    let stdout = child.stdout.take().ok_or("Failed to get stdout")?;
    let stderr = child.stderr.take().ok_or("Failed to get stderr")?;

    let mut reader = BufReader::new(stdout).lines();

    // Create shared storage for output lines
    let output_lines = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let output_lines_clone = output_lines.clone();

    // Wait for startup and parse both servers and clients
    let (servers, clients) =
        wait_for_netget_startup_with_capture(&mut reader, output_lines_clone.clone()).await?;

    // IMPORTANT: Continue reading stdout in background to prevent pipe buffer from filling
    // Without this, the server will crash with "Broken pipe" when stdout buffer fills
    tokio::spawn(async move {
        while let Some(line) = reader.next_line().await.ok().flatten() {
            println!("[DEBUG] NetGet output: {}", line);
            output_lines_clone.lock().await.push(line);
        }
    });

    // Also spawn a task to read stderr and capture it
    let output_lines_stderr = output_lines.clone();
    tokio::spawn(async move {
        let mut stderr_reader = BufReader::new(stderr).lines();
        while let Some(line) = stderr_reader.next_line().await.ok().flatten() {
            println!("[STDERR] {}", line);
            output_lines_stderr.lock().await.push(line);
        }
    });

    Ok(NetGetInstance {
        child,
        servers,
        clients,
        output_lines,
        mock_config: config.mock_config.clone(),
    })
}

/// Parse server startup line and extract information
fn parse_server_startup(line: &str) -> Option<(String, String, u16)> {
    // Look for pattern: "[SERVER] Starting server #N (STACK) on ADDR:PORT"
    if !line.contains("[SERVER]") || !line.contains("Starting server") {
        return None;
    }

    let mut server_id = String::new();
    let mut stack = String::new();
    let mut port = 0u16;

    // Extract server ID from "server #N"
    if let Some(idx_start) = line.find("server #") {
        let after_hash = &line[idx_start + 8..];
        let id_str: String = after_hash
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if !id_str.is_empty() {
            server_id = id_str;
        }
    }

    // Extract stack from parentheses
    if let Some(start_paren) = line.find('(') {
        if let Some(end_paren) = line.find(')') {
            if start_paren < end_paren {
                let stack_str = &line[start_paren + 1..end_paren];
                // Parse using the protocol registry
                if let Some(parsed_protocol) = server_registry::registry().parse_from_str(stack_str)
                {
                    stack = parsed_protocol;
                }
            }
        }
    }

    // Extract port from "on ADDR:PORT"
    if let Some(addr_start) = line.rfind("on ") {
        let addr_part = &line[addr_start + 3..];
        if let Some(colon_pos) = addr_part.find(':') {
            let port_part = &addr_part[colon_pos + 1..];
            let port_str: String = port_part
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(p) = port_str.parse::<u16>() {
                port = p;
            }
        }
    }

    if !server_id.is_empty() && !stack.is_empty() && port != 0 {
        Some((server_id, stack, port))
    } else {
        None
    }
}

/// Parse client startup line and extract initial information
fn parse_client_startup(line: &str) -> Option<(String, String, String)> {
    // Look for pattern: "[CLIENT] Starting client #N (PROTOCOL) connecting to ADDR"
    if !line.contains("[CLIENT]") || !line.contains("Starting client") {
        return None;
    }

    let mut client_id = String::new();
    let mut protocol = String::new();
    let mut remote_addr = String::new();

    // Extract client ID from "client #N"
    if let Some(idx_start) = line.find("client #") {
        let after_hash = &line[idx_start + 8..];
        let id_str: String = after_hash
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if !id_str.is_empty() {
            client_id = id_str;
        }
    }

    // Extract protocol from parentheses
    if let Some(start_paren) = line.find('(') {
        if let Some(end_paren) = line.find(')') {
            if start_paren < end_paren {
                protocol = line[start_paren + 1..end_paren].to_string();
            }
        }
    }

    // Extract remote address from "connecting to ADDR"
    if let Some(addr_start) = line.find("connecting to ") {
        remote_addr = line[addr_start + 14..].trim().to_string();
    }

    if !client_id.is_empty() && !protocol.is_empty() && !remote_addr.is_empty() {
        Some((client_id, protocol, remote_addr))
    } else {
        None
    }
}

/// Parse client connected line to get local address
fn parse_client_connected(line: &str) -> Option<(String, String)> {
    // Look for pattern: "[CLIENT] PROTOCOL client #N connected to ... (local: ADDR:PORT)"
    if !line.contains("[CLIENT]") || !line.contains("connected to") {
        return None;
    }

    let mut client_id = String::new();
    let mut local_addr = String::new();

    // Extract client ID from "client #N"
    if let Some(idx_start) = line.find("client #") {
        let after_hash = &line[idx_start + 8..];
        let id_str: String = after_hash
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if !id_str.is_empty() {
            client_id = id_str;
        }
    }

    // Extract local address from "(local: ADDR)"
    if let Some(local_start) = line.find("(local: ") {
        let after_local = &line[local_start + 8..];
        if let Some(close_paren) = after_local.find(')') {
            local_addr = after_local[..close_paren].to_string();
        }
    }

    if !client_id.is_empty() && !local_addr.is_empty() {
        Some((client_id, local_addr))
    } else {
        None
    }
}

/// Wait for netget startup and extract server and client info
async fn wait_for_netget_startup_with_capture(
    reader: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    output_lines: std::sync::Arc<tokio::sync::Mutex<Vec<String>>>,
) -> E2EResult<(Vec<NetGetServer>, Vec<NetGetClient>)> {
    let wait_future = async {
        let mut servers = Vec::new();
        let mut clients: std::collections::HashMap<String, NetGetClient> =
            std::collections::HashMap::new();
        let mut server_confirmations: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut had_any_startup = false;

        while let Some(line) = reader.next_line().await? {
            println!("[DEBUG] NetGet output: {}", line);
            output_lines.lock().await.push(line.clone());

            // Parse server startup
            if let Some((id, stack, port)) = parse_server_startup(&line) {
                println!(
                    "[DEBUG] Parsed server startup: id={}, stack={}, port={}",
                    id, stack, port
                );
                servers.push(NetGetServer {
                    id: id.clone(),
                    port,
                    stack,
                });
                had_any_startup = true;
            }

            // Parse client startup
            if let Some((id, protocol, remote_addr)) = parse_client_startup(&line) {
                println!(
                    "[DEBUG] Parsed client startup: id={}, protocol={}, remote_addr={}",
                    id, protocol, remote_addr
                );
                clients.insert(
                    id.clone(),
                    NetGetClient {
                        id,
                        protocol,
                        remote_addr,
                        local_addr: None,
                    },
                );
                had_any_startup = true;
            }

            // Parse client connected confirmation
            if let Some((id, local_addr)) = parse_client_connected(&line) {
                println!(
                    "[DEBUG] Parsed client connected: id={}, local_addr={}",
                    id, local_addr
                );
                if let Some(client) = clients.get_mut(&id) {
                    client.local_addr = Some(local_addr);
                }
            }

            // Parse server listening confirmation
            if line.contains("listening on") {
                // Extract port from "listening on ADDR:PORT"
                if let Some(addr_start) = line.find("on ") {
                    let addr_part = &line[addr_start + 3..];
                    if let Some(colon_pos) = addr_part.rfind(':') {
                        let port_str: String = addr_part[colon_pos + 1..]
                            .chars()
                            .take_while(|c| c.is_ascii_digit())
                            .collect();
                        if let Ok(port) = port_str.parse::<u16>() {
                            server_confirmations.insert(port.to_string());
                        }
                    }
                }
            }

            // Check for "Server is running" or "advertising" as alternative confirmations
            if line.contains("Server is running") || line.contains("advertising") {
                server_confirmations.insert("confirmed".to_string());
            }

            // Heuristic: If we had startup messages and haven't seen a new one in a bit,
            // or if all servers are confirmed, we're likely done starting up
            if had_any_startup {
                // Simple heuristic: if we have startup messages and see confirmation messages,
                // or if the TUI prompt appears, we're probably done
                if line.contains("netget>")
                    || line.contains("Ready")
                    || !server_confirmations.is_empty()
                {
                    // Give a short time to capture any remaining startup messages
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    break;
                }
            }
        }

        if !had_any_startup {
            return Err("No servers or clients started in netget".into());
        }

        let final_servers = servers;
        let final_clients: Vec<NetGetClient> = clients.into_values().collect();

        println!(
            "[DEBUG] Startup complete. Servers: {}, Clients: {}",
            final_servers.len(),
            final_clients.len()
        );

        Ok((final_servers, final_clients))
    };

    timeout(Duration::from_secs(120), wait_future)
        .await
        .map_err(|_| "Timeout waiting for netget startup")?
}

/// Check if Ollama is available
async fn check_ollama_available() -> bool {
    // Try to connect to Ollama
    let client = reqwest::Client::new();
    match tokio::time::timeout(
        Duration::from_secs(2),
        client.get("http://localhost:11434/api/tags").send(),
    )
    .await
    {
        Ok(Ok(response)) => response.status().is_success(),
        _ => false,
    }
}
