// Core NetGet startup and parsing functionality

use netget::protocol::server_registry;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};
use super::common::*;
use super::mock_builder::MockLlmBuilder;
use super::mock_config::MockLlmConfig;

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
    /// Mock Ollama server (for verification and lifecycle)
    /// IMPORTANT: Must NOT have underscore prefix - field must be kept alive for entire test duration
    #[allow(dead_code)]
    pub mock_ollama_server: Option<super::mock_ollama::MockOllamaServer>,
    /// Temporary mock config file (kept alive for duration of test) - DEPRECATED, use mock_ollama_server
    /// IMPORTANT: Must NOT have underscore prefix - field must be kept alive for entire test duration
    #[allow(dead_code)]
    pub mock_temp_file: Option<tempfile::TempPath>,
    /// Mock configuration (DEPRECATED - kept for backward compat, use mock_ollama_server for verification)
    pub mock_config: Option<MockLlmConfig>,
    /// Abort handles for background reader tasks
    pub(crate) stdout_reader_handle: tokio::task::JoinHandle<()>,
    pub(crate) stderr_reader_handle: tokio::task::JoinHandle<()>,
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
#[allow(dead_code)]
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
    /// Maximum concurrent LLM requests (default: None, uses netget's default of 1)
    pub llm_max_concurrent: Option<usize>,
    /// Mock LLM configuration (for testing without Ollama)
    pub mock_config: Option<MockLlmConfig>,
}

/// Detect if --use-ollama flag is present (from test args or environment)
fn should_use_ollama() -> bool {
    // Check environment variable (set by test-e2e.sh)
    if std::env::var("NETGET_USE_OLLAMA").is_ok() {
        return true;
    }

    // Check command line args (for cargo test -- --use-ollama)
    std::env::args().any(|arg| arg == "--use-ollama")
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
            llm_max_concurrent: Some(1000), // High concurrency for E2E tests (effectively unlimited)
            mock_config: None,
        }
    }

    /// Create a new config with scripts disabled
    #[allow(dead_code)]
    pub fn new_no_scripts(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            model: None,
            log_level: "debug".to_string(),
            listen_addr: "127.0.0.1".to_string(),
            no_scripts: true,
            include_disabled_protocols: false,
            ollama_lock: true,
            llm_max_concurrent: Some(1000), // High concurrency for E2E tests (effectively unlimited)
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
    #[allow(dead_code)]
    pub fn new_with_trace(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            model: None,
            log_level: "trace".to_string(),
            listen_addr: "127.0.0.1".to_string(),
            no_scripts: false,
            include_disabled_protocols: false,
            ollama_lock: true,
            llm_max_concurrent: None,
            mock_config: None,
        }
    }

    /// Set the model to use
    #[allow(dead_code)]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the log level
    #[allow(dead_code)]
    pub fn with_log_level(mut self, level: impl Into<String>) -> Self {
        self.log_level = level.into();
        self
    }

    /// Set the listen address
    #[allow(dead_code)]
    pub fn with_listen_addr(mut self, addr: impl Into<String>) -> Self {
        self.listen_addr = addr.into();
        self
    }

    /// Disable script generation
    #[allow(dead_code)]
    pub fn with_no_scripts(mut self, no_scripts: bool) -> Self {
        self.no_scripts = no_scripts;
        self
    }

    /// Include disabled protocols (for testing honeypot-only protocols like IPSec, OpenVPN)
    #[allow(dead_code)]
    pub fn with_include_disabled_protocols(mut self, include_disabled: bool) -> Self {
        self.include_disabled_protocols = include_disabled;
        self
    }

    /// Enable or disable Ollama lock for concurrent testing
    #[allow(dead_code)]
    pub fn with_ollama_lock(mut self, enabled: bool) -> Self {
        self.ollama_lock = enabled;
        self
    }

    /// Set maximum concurrent LLM requests (for testing concurrent request handling)
    #[allow(dead_code)]
    pub fn with_llm_max_concurrent(mut self, max_concurrent: usize) -> Self {
        self.llm_max_concurrent = Some(max_concurrent);
        self
    }

    /// Configure mock LLM responses (for testing without Ollama)
    ///
    /// # Example
    /// ```ignore
    /// use crate::helpers::mock_config::MockLlmBuilder;
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
        F: FnOnce(MockLlmBuilder) -> MockLlmBuilder,
    {
        let builder = MockLlmBuilder::new();
        self.mock_config = Some(builder_fn(builder).build());
        self
    }
}

/// Start a NetGet instance with the given configuration
/// Returns instance with 0+ servers and 0+ clients
pub async fn start_netget(config: NetGetConfig) -> E2EResult<NetGetInstance> {
    let use_ollama = should_use_ollama();
    let has_mocks = config.mock_config.is_some();

    // Start mock Ollama server (if in mock mode with mocks configured)
    let mock_ollama_server = if use_ollama {
        // Real mode: Use Ollama
        if has_mocks {
            println!("⚠️  --use-ollama: Ignoring configured mocks");
        }
        // Check Ollama availability
        if !check_ollama_available().await {
            return Err("--use-ollama requires Ollama, but Ollama is not available at http://localhost:11434".into());
        }
        println!("🤖 Using real Ollama");
        None
    } else {
        // Mock mode (default): Use mock Ollama HTTP server
        if !has_mocks {
            // No mocks explicitly configured - use default empty mock
            // This allows tests to run without Ollama and see what LLM calls are made
            println!("🔧 Mock mode: Using default empty mock (returns generic responses)");
            println!("   → Configure specific mocks with .with_mock() for test assertions");

            // Create default empty mock config
            let mock_config = MockLlmBuilder::new().build();
            let server = super::mock_ollama::MockOllamaServer::start(mock_config).await?;
            println!("🔧 Mock Ollama server started on {}", server.base_url());
            Some(server)
        } else {
            println!("🔧 Using configured mock LLM responses");

            // Start mock Ollama HTTP server with user-configured mocks
            let mock_config = config.mock_config.clone().unwrap();
            let server = super::mock_ollama::MockOllamaServer::start(mock_config).await?;
            println!("🔧 Mock Ollama server started on {}", server.base_url());
            Some(server)
        }
    };

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

    // Add --llm-max-concurrent flag if specified (for testing concurrent request handling)
    if let Some(max_concurrent) = config.llm_max_concurrent {
        cmd.arg("--llm-max-concurrent")
            .arg(max_concurrent.to_string());
    }

    // Add --ollama-url if using mock server
    if let Some(ref mock_server) = mock_ollama_server {
        cmd.arg("--ollama-url").arg(mock_server.base_url());
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

    // IMPORTANT: Start stderr reader BEFORE waiting for startup
    // If netget crashes with an error on stderr during startup, we need to capture it!
    let output_lines_stderr = output_lines.clone();
    let stderr_reader_handle = tokio::spawn(async move {
        let mut stderr_reader = BufReader::new(stderr).lines();
        while let Some(line) = stderr_reader.next_line().await.ok().flatten() {
            println!("[STDERR] {}", line);
            output_lines_stderr.lock().await.push(line);
        }
        println!("[DEBUG] stderr reader task finished");
    });

    // Wait for startup and parse both servers and clients
    let (servers, clients) =
        wait_for_netget_startup_with_capture(&mut reader, output_lines_clone.clone()).await?;

    // IMPORTANT: Continue reading stdout in background to prevent pipe buffer from filling
    // Without this, the server will crash with "Broken pipe" when stdout buffer fills
    let stdout_reader_handle = tokio::spawn(async move {
        while let Some(line) = reader.next_line().await.ok().flatten() {
            println!("[DEBUG] NetGet output: {}", line);
            output_lines_clone.lock().await.push(line);
        }
        println!("[DEBUG] stdout reader task finished");
    });

    Ok(NetGetInstance {
        child,
        servers,
        clients,
        output_lines,
        mock_ollama_server,
        mock_temp_file: None,
        mock_config: config.mock_config.clone(),
        stdout_reader_handle,
        stderr_reader_handle,
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

    // Allow port 0 (server will bind to an available port)
    if !server_id.is_empty() && !stack.is_empty() {
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
    let wait_future = async move {
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

            // Parse server listening/ready confirmation and update port
            if line.contains("listening on") || line.contains("ready on") {
                // Extract port from "listening on ADDR:PORT" or "ready on ADDR:PORT"
                if let Some(addr_start) = line.rfind("on ") {
                    let addr_part = &line[addr_start + 3..];
                    if let Some(colon_pos) = addr_part.rfind(':') {
                        let port_str: String = addr_part[colon_pos + 1..]
                            .chars()
                            .take_while(|c| c.is_ascii_digit())
                            .collect();
                        if let Ok(port) = port_str.parse::<u16>() {
                            println!(
                                "[DEBUG] Parsed listening confirmation: port={}",
                                port
                            );
                            server_confirmations.insert(port.to_string());

                            // Update the most recent server with port 0
                            // Find the last server that has port 0 (requested ephemeral port)
                            if let Some(server) = servers.iter_mut().rev().find(|s| s.port == 0) {
                                println!(
                                    "[DEBUG] Updating server #{} port from 0 to {}",
                                    server.id, port
                                );
                                server.port = port;
                            }
                        }
                    }
                }
            }

            // Check for "Server is running" or "advertising" as alternative confirmations
            if line.contains("Server is running") || line.contains("advertising") {
                server_confirmations.insert("confirmed".to_string());
            }

            // Check for non-interactive mode completion
            if line.contains("Press Ctrl+C to stop") {
                server_confirmations.insert("non_interactive".to_string());
            }

            // Check for conversation state update (appears after server startup completes)
            if line.contains("Updated conversation state after server changes")
                || line.contains("Updated conversation state after client changes")
            {
                server_confirmations.insert("state_updated".to_string());
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
                    // Check if any servers still have port 0 (waiting for actual port assignment)
                    let has_port_zero = servers.iter().any(|s| s.port == 0);

                    if has_port_zero {
                        // Continue reading for up to 2 more seconds to catch "listening on" messages
                        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
                        while tokio::time::Instant::now() < deadline {
                            match tokio::time::timeout(
                                Duration::from_millis(100),
                                reader.next_line(),
                            )
                            .await
                            {
                                Ok(Ok(Some(line))) => {
                                    println!("[DEBUG] NetGet output (extended): {}", line);
                                    output_lines.lock().await.push(line.clone());

                                    // Try to parse "listening on" or "ready on" message
                                    if line.contains("listening on") || line.contains("ready on") {
                                        if let Some(addr_start) = line.rfind("on ") {
                                            let addr_part = &line[addr_start + 3..];
                                            if let Some(colon_pos) = addr_part.rfind(':') {
                                                let port_str: String = addr_part[colon_pos + 1..]
                                                    .chars()
                                                    .take_while(|c| c.is_ascii_digit())
                                                    .collect();
                                                if let Ok(port) = port_str.parse::<u16>() {
                                                    println!(
                                                        "[DEBUG] Parsed listening confirmation: port={}",
                                                        port
                                                    );
                                                    // Update the most recent server with port 0
                                                    if let Some(server) = servers.iter_mut().rev().find(|s| s.port == 0) {
                                                        println!(
                                                            "[DEBUG] Updating server #{} port from 0 to {}",
                                                            server.id, port
                                                        );
                                                        server.port = port;
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    // Break early if all port-0 servers are updated
                                    if !servers.iter().any(|s| s.port == 0) {
                                        break;
                                    }
                                }
                                _ => {
                                    // Timeout or error reading, continue waiting
                                }
                            }
                        }
                    } else {
                        // Give a short time to capture any remaining startup messages
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                    break;
                }
            }
        }

        if !had_any_startup {
            let captured_output = output_lines.lock().await;
            let output_str = if captured_output.is_empty() {
                "(no output captured)".to_string()
            } else {
                captured_output.join("\n")
            };
            return Err(format!(
                "No servers or clients started in netget\n\nCaptured output:\n{}",
                output_str
            ).into());
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

impl Drop for NetGetInstance {
    fn drop(&mut self) {
        // Abort background reader tasks to prevent hanging
        // These tasks will be waiting on pipes that may not close cleanly
        self.stdout_reader_handle.abort();
        self.stderr_reader_handle.abort();

        // Note: child.kill() is async, so we can't call it in Drop
        // However, the Child struct has kill_on_drop=true set (line 326),
        // so it will be killed automatically when dropped
        println!("[DEBUG] NetGetInstance dropped, background tasks aborted");
    }
}

impl NetGetInstance {
    /// Verify all mock expectations were met
    #[allow(dead_code)]
    pub async fn verify_mocks(&self) -> E2EResult<()> {
        // Use mock server verification if available (new approach)
        if let Some(ref mock_server) = self.mock_ollama_server {
            return mock_server.verify_calls().await;
        }

        // Fallback to deprecated mock_config verification (backward compat)
        let Some(ref mock_config) = self.mock_config else {
            // No mocks configured, nothing to verify
            return Ok(());
        };

        // Mark as verified
        mock_config.mark_verified();

        let mut errors = Vec::new();

        for (idx, rule) in mock_config.rules.iter().enumerate() {
            let actual = rule.actual_calls.load(std::sync::atomic::Ordering::SeqCst);

            // Check exact count
            if let Some(expected) = rule.expected_calls {
                if actual != expected {
                    errors.push(format!(
                        "Rule #{} ({}): Expected {} calls, got {}",
                        idx,
                        rule.describe(),
                        expected,
                        actual
                    ));
                }
            }

            // Check minimum
            if let Some(min) = rule.min_calls {
                if actual < min {
                    errors.push(format!(
                        "Rule #{} ({}): Expected at least {} calls, got {}",
                        idx,
                        rule.describe(),
                        min,
                        actual
                    ));
                }
            }

            // Check maximum
            if let Some(max) = rule.max_calls {
                if actual > max {
                    errors.push(format!(
                        "Rule #{} ({}): Expected at most {} calls, got {}",
                        idx,
                        rule.describe(),
                        max,
                        actual
                    ));
                }
            }
        }

        if !errors.is_empty() {
            // Print detailed diagnostics
            eprintln!("\n❌ Mock verification failed:");
            for error in &errors {
                eprintln!("  {}", error);
            }
            eprintln!("\nAll LLM call history:");
            let history = mock_config.call_history.lock().await;
            for (idx, call) in history.iter().enumerate() {
                eprintln!(
                    "  Call #{}: {} -> matched rule #{}",
                    idx + 1,
                    call.context.event_type.as_deref().unwrap_or("(none)"),
                    call.matched_rule_idx
                );
            }
            eprintln!();

            return Err(format!("Mock verification failed:\n{}", errors.join("\n")).into());
        }

        println!("✅ All mock expectations verified successfully");
        Ok(())
    }

    /// Wait for a specific log pattern to appear in the output
    /// Returns when the pattern is found or times out
    #[allow(dead_code)]
    pub async fn wait_for_log(&self, pattern: &str, timeout_secs: u64) -> E2EResult<()> {
        let start = std::time::Instant::now();
        let timeout_duration = Duration::from_secs(timeout_secs);

        loop {
            // Check if pattern exists in current output
            {
                let lines = self.output_lines.lock().await;
                if lines.iter().any(|line| line.contains(pattern)) {
                    println!("[DEBUG] Found log pattern: '{}'", pattern);
                    return Ok(());
                }
            }

            // Check timeout
            if start.elapsed() > timeout_duration {
                let lines = self.output_lines.lock().await;
                return Err(format!(
                    "Timeout waiting for log pattern '{}' after {}s\nCaptured output:\n{}",
                    pattern,
                    timeout_secs,
                    lines.join("\n")
                )
                .into());
            }

            // Sleep briefly before checking again
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Wait for a log pattern to appear N times
    #[allow(dead_code)]
    pub async fn wait_for_log_count(&self, pattern: &str, min_count: usize, timeout_secs: u64) -> E2EResult<()> {
        let start = std::time::Instant::now();
        let timeout_duration = Duration::from_secs(timeout_secs);

        loop {
            // Count occurrences in current output
            {
                let lines = self.output_lines.lock().await;
                let count = lines.iter().filter(|line| line.contains(pattern)).count();
                if count >= min_count {
                    println!("[DEBUG] Found log pattern '{}' {} times (needed {})", pattern, count, min_count);
                    return Ok(());
                }
            }

            // Check timeout
            if start.elapsed() > timeout_duration {
                let lines = self.output_lines.lock().await;
                let count = lines.iter().filter(|line| line.contains(pattern)).count();
                return Err(format!(
                    "Timeout waiting for log pattern '{}' to appear {} times (found {} times) after {}s",
                    pattern,
                    min_count,
                    count,
                    timeout_secs
                )
                .into());
            }

            // Sleep briefly before checking again
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Stop the NetGet instance gracefully
    #[allow(dead_code)]
    pub async fn stop(mut self) -> E2EResult<()> {
        // Try to stop gracefully with Ctrl+C
        #[cfg(unix)]
        {
            use nix::sys::signal::{self, Signal};
            use nix::unistd::Pid;

            if let Some(pid) = self.child.id() {
                let _ = signal::kill(Pid::from_raw(pid as i32), Signal::SIGINT);
            }
        }

        // Give it time to shutdown gracefully
        let shutdown = async {
            sleep(Duration::from_millis(500)).await;
            self.child.wait().await
        };

        let result = match tokio::time::timeout(Duration::from_secs(5), shutdown).await {
            Ok(Ok(_)) => Ok(()),
            _ => {
                // Force kill if graceful shutdown failed
                self.child.kill().await?;
                Ok(())
            }
        };

        // Abort background reader tasks to prevent hanging
        // (Do this after killing the child so pipes will close)
        self.stdout_reader_handle.abort();
        self.stderr_reader_handle.abort();

        // Wait briefly for tasks to abort
        let _ = tokio::time::timeout(
            Duration::from_millis(100),
            async {
                let _ = (&mut self.stdout_reader_handle).await;
                let _ = (&mut self.stderr_reader_handle).await;
            }
        ).await;

        result
    }

    /// Check if output contains a specific string
    #[allow(dead_code)]
    pub async fn output_contains(&self, needle: &str) -> bool {
        let lines = self.output_lines.lock().await;
        lines.iter().any(|line| line.contains(needle))
    }

    /// Wait for a log line containing the exact pattern with timeout
    ///
    /// Polls output_lines every 50ms until a line contains the pattern or timeout occurs.
    /// Returns the matching line on success.
    ///
    /// # Example
    /// ```ignore
    /// server.wait_for_pattern("TCP received 5 bytes", Duration::from_secs(5)).await?;
    /// ```
    #[allow(dead_code)]
    pub async fn wait_for_pattern(&self, pattern: &str, timeout: Duration) -> E2EResult<String> {
        let start = std::time::Instant::now();

        loop {
            // Check if pattern appears in output
            {
                let lines = self.output_lines.lock().await;
                if let Some(line) = lines.iter().find(|line| line.contains(pattern)) {
                    return Ok(line.clone());
                }
            }

            // Check timeout
            if start.elapsed() >= timeout {
                return Err(format!(
                    "Timeout waiting for pattern '{}' after {:?}",
                    pattern, timeout
                )
                .into());
            }

            // Wait before next check
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Wait for a log line matching a regex pattern with timeout
    ///
    /// Polls output_lines every 50ms until a line matches the regex or timeout occurs.
    /// Returns the matching line on success.
    ///
    /// # Example
    /// ```ignore
    /// let regex = regex::Regex::new(r"TCP received \d+ bytes").unwrap();
    /// server.wait_for_regex(&regex, Duration::from_secs(5)).await?;
    /// ```
    #[allow(dead_code)]
    pub async fn wait_for_regex(
        &self,
        regex: &regex::Regex,
        timeout: Duration,
    ) -> E2EResult<String> {
        let start = std::time::Instant::now();

        loop {
            // Check if regex matches any line
            {
                let lines = self.output_lines.lock().await;
                if let Some(line) = lines.iter().find(|line| regex.is_match(line)) {
                    return Ok(line.clone());
                }
            }

            // Check timeout
            if start.elapsed() >= timeout {
                return Err(format!(
                    "Timeout waiting for regex pattern '{}' after {:?}",
                    regex.as_str(),
                    timeout
                )
                .into());
            }

            // Wait before next check
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Wait for multiple patterns in order with timeout
    ///
    /// Each pattern must appear after the previous one. Returns all matching lines.
    ///
    /// # Example
    /// ```ignore
    /// server.wait_for_patterns(&[
    ///     "TCP client connected",
    ///     "TCP received 5 bytes",
    ///     "Sent response"
    /// ], Duration::from_secs(10)).await?;
    /// ```
    #[allow(dead_code)]
    pub async fn wait_for_patterns(
        &self,
        patterns: &[&str],
        timeout: Duration,
    ) -> E2EResult<Vec<String>> {
        let start = std::time::Instant::now();
        let mut results = Vec::new();
        let mut last_index = 0;

        for pattern in patterns {
            loop {
                // Check if pattern appears after last match
                {
                    let lines = self.output_lines.lock().await;
                    if let Some((idx, line)) = lines
                        .iter()
                        .enumerate()
                        .skip(last_index)
                        .find(|(_, line)| line.contains(pattern))
                    {
                        results.push(line.clone());
                        last_index = idx + 1;
                        break;
                    }
                }

                // Check timeout
                if start.elapsed() >= timeout {
                    return Err(format!(
                        "Timeout waiting for pattern '{}' (matched {} of {} patterns) after {:?}",
                        pattern,
                        results.len(),
                        patterns.len(),
                        timeout
                    )
                    .into());
                }

                // Wait before next check
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }

        Ok(results)
    }

    /// Get all output lines
    #[allow(dead_code)]
    pub async fn get_output(&self) -> Vec<String> {
        self.output_lines.lock().await.clone()
    }
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
