// E2E test helpers for NetGet binary testing
//
// This module provides utilities to test NetGet by spawning the actual binary
// and interacting with it as a black-box system.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};
use std::future::Future;
use netget::protocol::{base_stack::BaseStack, registry};

/// Result type for e2e tests
pub type E2EResult<T> = Result<T, Box<dyn std::error::Error>>;

/// Retry a condition with exponential backoff until it succeeds or times out
///
/// # Arguments
/// * `condition` - A closure that returns Ok(T) when successful, Err otherwise
/// * `initial_delay` - Initial delay between retries (default: 50ms)
/// * `max_delay` - Maximum delay between retries (default: 1s)
/// * `timeout_duration` - Total timeout for all retries (default: 10s)
///
/// # Returns
/// * `Ok(T)` - The successful result from the condition
/// * `Err(_)` - If timeout is reached before condition succeeds
///
/// # Example
/// ```rust,ignore
/// // Wait for server to be ready
/// retry_with_backoff(
///     || async {
///         match TcpStream::connect(addr).await {
///             Ok(stream) => Ok(stream),
///             Err(e) => Err(e.into()),
///         }
///     },
///     Duration::from_millis(50),
///     Duration::from_secs(1),
///     Duration::from_secs(5),
/// ).await?;
/// ```
pub async fn retry_with_backoff<F, Fut, T, E>(
    mut condition: F,
    initial_delay: Duration,
    max_delay: Duration,
    timeout_duration: Duration,
) -> Result<T, Box<dyn std::error::Error>>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::error::Error + 'static,
{
    let start = std::time::Instant::now();
    let mut delay = initial_delay;
    let mut attempts = 0;

    loop {
        attempts += 1;

        match condition().await {
            Ok(result) => {
                if attempts > 1 {
                    println!("  [RETRY] Condition succeeded after {} attempts in {:?}", attempts, start.elapsed());
                }
                return Ok(result);
            }
            Err(e) => {
                if start.elapsed() >= timeout_duration {
                    return Err(format!(
                        "Retry timeout after {:?} ({} attempts). Last error: {}",
                        timeout_duration, attempts, e
                    )
                    .into());
                }

                // Sleep with exponential backoff
                sleep(delay).await;
                delay = (delay * 2).min(max_delay);
            }
        }
    }
}

/// Retry a condition with default settings (50ms initial, 1s max, 10s timeout)
pub async fn retry<F, Fut, T, E>(condition: F) -> Result<T, Box<dyn std::error::Error>>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::error::Error + 'static,
{
    retry_with_backoff(
        condition,
        Duration::from_millis(50),
        Duration::from_secs(1),
        Duration::from_secs(10),
    )
    .await
}

/// Get an available port for testing
pub async fn get_available_port() -> E2EResult<u16> {
    use tokio::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

/// Configuration for an e2e test server
pub struct ServerConfig {
    /// The prompt to send to the binary
    pub prompt: String,
    /// Optional model override
    pub model: Option<String>,
    /// Log level (default: "off")
    pub log_level: String,
    /// Listen address (default: "127.0.0.1")
    pub listen_addr: String,
    /// Disable script generation (default: false)
    pub no_scripts: bool,
    /// Include disabled protocols (default: false)
    pub include_disabled_protocols: bool,
    /// Enable Ollama lock for concurrent test execution (default: true)
    pub ollama_lock: bool,
}

impl ServerConfig {
    /// Create a new server config with the given prompt
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            model: None,
            log_level: "off".to_string(),
            listen_addr: "127.0.0.1".to_string(),
            no_scripts: false,
            include_disabled_protocols: false,
            ollama_lock: true, // Enable by default for concurrent testing
        }
    }

    /// Create a new server config with scripts disabled
    pub fn new_no_scripts(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            model: None,
            log_level: "off".to_string(),
            listen_addr: "127.0.0.1".to_string(),
            no_scripts: true,
            include_disabled_protocols: false,
            ollama_lock: true, // Enable by default for concurrent testing
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
}

/// Replace {AVAILABLE_PORT} placeholders with actual available ports
async fn replace_port_placeholders(prompt: &str) -> E2EResult<String> {
    const PLACEHOLDER: &str = "{AVAILABLE_PORT}";

    // Count how many placeholders we need to replace
    let placeholder_count = prompt.matches(PLACEHOLDER).count();

    if placeholder_count == 0 {
        // No placeholders to replace, return original prompt
        return Ok(prompt.to_string());
    }

    // Allocate unique available ports
    let mut ports = Vec::with_capacity(placeholder_count);
    for _ in 0..placeholder_count {
        let port = get_available_port().await?;
        ports.push(port);
    }

    // Replace placeholders one by one
    let mut result = prompt.to_string();
    for port in ports {
        result = result.replacen(PLACEHOLDER, &port.to_string(), 1);
    }

    Ok(result)
}

/// A running NetGet server process
pub struct NetGetServer {
    /// The child process
    child: Child,
    /// The port the server is listening on
    pub port: u16,
    /// The actual protocol stack that was started
    pub stack: String,
    /// Captured server output lines (for verification)
    pub output_lines: std::sync::Arc<tokio::sync::Mutex<Vec<String>>>,
}

impl NetGetServer {
    /// Stop the server gracefully
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

        match timeout(Duration::from_secs(5), shutdown).await {
            Ok(Ok(_)) => Ok(()),
            _ => {
                // Force kill if graceful shutdown failed
                self.child.kill().await?;
                Ok(())
            }
        }
    }

    /// Check if the server is still running
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Check if output contains a specific string
    pub async fn output_contains(&self, needle: &str) -> bool {
        let lines = self.output_lines.lock().await;
        lines.iter().any(|line| line.contains(needle))
    }

    /// Count occurrences of a pattern in output
    pub async fn count_in_output(&self, needle: &str) -> usize {
        let lines = self.output_lines.lock().await;
        lines.iter().filter(|line| line.contains(needle)).count()
    }

    /// Get all output lines
    pub async fn get_output(&self) -> Vec<String> {
        self.output_lines.lock().await.clone()
    }
}

/// Start a NetGet server with the given configuration
pub async fn start_netget_server(config: ServerConfig) -> E2EResult<NetGetServer> {
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

    // For non-interactive mode, output might go to stdout or stderr
    // We'll check stdout first
    let mut reader = BufReader::new(stdout).lines();

    // Parse the prompt to find the expected port and stack
    let expected_port = extract_port_from_prompt(&processed_prompt);
    // Use the protocol registry to parse the expected stack from the prompt
    let expected_base_stack = registry::registry().parse_from_str(&processed_prompt);

    // Create shared storage for output lines (BEFORE reading so we capture everything)
    let output_lines = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let output_lines_clone = output_lines.clone();

    // Wait for the server to start and parse the actual configuration
    let (actual_port, actual_base_stack) = wait_for_server_startup_with_capture(&mut reader, output_lines_clone.clone()).await?;

    // IMPORTANT: Continue reading stdout in background to prevent pipe buffer from filling
    // Without this, the server will crash with "Broken pipe" when stdout buffer fills
    tokio::spawn(async move {
        while let Some(line) = reader.next_line().await.ok().flatten() {
            println!("[DEBUG] Server output: {}", line);
            output_lines_clone.lock().await.push(line);
        }
    });

    // Also spawn a task to read stderr for debugging
    tokio::spawn(async move {
        let mut stderr_reader = BufReader::new(stderr).lines();
        while let Some(line) = stderr_reader.next_line().await.ok().flatten() {
            println!("[DEBUG stderr] {}", line);
        }
    });

    // Validate that the server started with the expected stack
    if let Some(expected_stack) = expected_base_stack {
        if expected_stack != actual_base_stack {
            // Get the stack names for error message
            let expected_name = registry::registry()
                .stack_name(&expected_stack)
                .unwrap_or("Unknown");
            let actual_name = registry::registry()
                .stack_name(&actual_base_stack)
                .unwrap_or("Unknown");
            return Err(format!(
                "Server started with wrong stack! Expected: {} ({:?}), Got: {} ({:?})",
                expected_name, expected_stack, actual_name, actual_base_stack
            )
            .into());
        }
    }

    // Validate port if a specific port was requested
    if expected_port != 0 && actual_port != expected_port {
        return Err(format!(
            "Server started on wrong port! Expected: {}, Got: {}",
            expected_port, actual_port
        )
        .into());
    }

    // No need to check port availability - we already confirmed the server is listening
    // by waiting for the "listening on" message in wait_for_server_startup()

    // Get the actual stack name for the NetGetServer struct
    let actual_stack_name = registry::registry()
        .stack_name(&actual_base_stack)
        .unwrap_or("Unknown")
        .to_string();

    Ok(NetGetServer {
        child,
        port: actual_port,
        stack: actual_stack_name,
        output_lines,
    })
}

/// Get the path to the NetGet binary
fn get_netget_binary_path() -> E2EResult<PathBuf> {
    // First try release build
    let release_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("release")
        .join("netget");

    if release_path.exists() {
        return Ok(release_path);
    }

    // Fall back to debug build
    let debug_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("debug")
        .join("netget");

    if debug_path.exists() {
        return Ok(debug_path);
    }

    Err("NetGet binary not found. Please run 'cargo build --release' first.".into())
}

/// Extract port number from prompt
fn extract_port_from_prompt(prompt: &str) -> u16 {
    let prompt_lower = prompt.to_lowercase();

    // Look for "port <number>" pattern
    if let Some(port_pos) = prompt_lower.find("port ") {
        let after_port = &prompt_lower[port_pos + 5..];
        if let Some(end_pos) = after_port.find(|c: char| !c.is_ascii_digit()) {
            if let Ok(port) = after_port[..end_pos].parse::<u16>() {
                return port;
            }
        } else if let Ok(port) = after_port.parse::<u16>() {
            return port;
        }
    }

    0 // Default to 0 for dynamic allocation
}

/// Wait for server startup and extract port and stack info
async fn wait_for_server_startup_with_capture(
    reader: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    output_lines: std::sync::Arc<tokio::sync::Mutex<Vec<String>>>,
) -> E2EResult<(u16, BaseStack)> {
    let wait_future = async {
        let mut port = 0u16;
        let mut base_stack: Option<BaseStack> = None;
        let mut found_starting_message = false;

        while let Some(line) = reader.next_line().await? {
            println!("[DEBUG] Server output: {}", line); // Debug output
            output_lines.lock().await.push(line.clone()); // Capture for assertions

            // Look for SERVER message pattern: "[SERVER] Starting server #N (<STACK>) on <ADDRESS>:<PORT>"
            if line.contains("[SERVER]") && line.contains("Starting server") && line.contains("on ") {
                // Extract stack type from parentheses
                // Format: "[SERVER] Starting server #1 (ETH>IP>TCP>HTTP) on 127.0.0.1:8080"
                if let Some(start_paren) = line.find('(') {
                    if let Some(end_paren) = line.find(')') {
                        if start_paren < end_paren {
                            let stack_str = &line[start_paren + 1..end_paren];
                            println!("[DEBUG] Extracted stack string from server output: '{}'", stack_str);

                            // Parse using the protocol registry
                            if let Some(parsed_stack) = registry::registry().parse_from_str(stack_str) {
                                base_stack = Some(parsed_stack);
                                println!("[DEBUG] Parsed stack: {:?}", parsed_stack);
                            } else {
                                return Err(format!(
                                    "Failed to parse stack from server output: '{}'. This stack is not recognized by the registry.",
                                    stack_str
                                ).into());
                            }
                        }
                    }
                }

                // Extract port from address
                if let Some(addr_start) = line.rfind("on ") {
                    let addr_part = &line[addr_start + 3..];
                    if let Some(colon_pos) = addr_part.find(':') {
                        let port_part = &addr_part[colon_pos + 1..];
                        let port_str: String = port_part.chars()
                            .take_while(|c| c.is_ascii_digit())
                            .collect();
                        if let Ok(p) = port_str.parse::<u16>() {
                            port = p;
                        }
                    }
                }

                // Set found_starting_message if we successfully parsed the stack
                if base_stack.is_some() {
                    println!("[DEBUG] Server starting: {:?} stack (requested port: {})", base_stack, port);
                    found_starting_message = true;
                    // Don't return yet - wait for "listening on" message
                }
            }

            // Wait for the "listening on" message which means the server is ACTUALLY ready
            // This prevents issues where we connect before the server is fully initialized
            if found_starting_message && line.contains("listening on") {
                // Extract the actual port from the "listening on" message
                // Format: "[INFO] Elasticsearch server listening on 127.0.0.1:61146"
                if let Some(addr_start) = line.find(" on ") {
                    let addr_part = &line[addr_start + 4..];
                    if let Some(colon_pos) = addr_part.rfind(':') {
                        let port_str: String = addr_part[colon_pos + 1..]
                            .chars()
                            .take_while(|c| c.is_ascii_digit())
                            .collect();
                        if let Ok(actual_port) = port_str.parse::<u16>() {
                            port = actual_port;
                            println!("[DEBUG] Server is now listening and ready for connections on port {}", port);
                            return Ok((port, base_stack.unwrap()));
                        }
                    }
                }
                // Fallback: if port extraction fails but message contains the port we expect
                if line.contains(&port.to_string()) {
                    println!("[DEBUG] Server is now listening and ready for connections on port {}", port);
                    return Ok((port, base_stack.unwrap()));
                }
            }

            // For some protocols (mDNS, MySQL, IPP), the "listening on" message might differ
            // Also accept "Server is running" or "advertising" as confirmation after seeing the starting message
            if found_starting_message && (line.contains("Server is running") || line.contains("advertising")) {
                println!("[DEBUG] Server confirmed running on port {}", port);
                return Ok((port, base_stack.unwrap()));
            }
        }
        Err("Server did not output startup information".into())
    };

    timeout(Duration::from_secs(120), wait_future)  // Increased timeout for LLM processing under load
        .await
        .map_err(|_| "Timeout waiting for server startup")?
}


/// Helper to build a simple test prompt
pub fn build_prompt(base_stack: &str, port: u16, instructions: &str) -> String {
    if port == 0 {
        format!("listen on port 0 via {}. {}", base_stack, instructions)
    } else {
        format!("listen on port {} via {}. {}", port, base_stack, instructions)
    }
}

/// Kill all running netget processes (useful for cleanup)
pub async fn cleanup_stray_processes() {
    #[cfg(unix)]
    {
        let _ = Command::new("pkill")
            .arg("-f")
            .arg("target/.*/netget")
            .output()
            .await;
    }
}

/// Wait for server to be ready and responsive
///
/// This function waits for the server to have specific output indicating it's ready.
/// The server is considered ready when output contains the protocol name.
///
/// # Arguments
/// * `server` - The NetGetServer instance to check
/// * `timeout_duration` - Maximum time to wait for server readiness
/// * `protocol_name` - Expected protocol name to find in output (e.g., "IMAP", "HTTP")
///
/// # Returns
/// * `Ok(())` if server is ready within timeout
/// * `Err(_)` if timeout expires before server is ready
///
/// # Example
/// ```rust,ignore
/// let server = start_netget_server(config).await?;
/// wait_for_server_startup(&server, Duration::from_secs(10), "IMAP").await?;
/// ```
pub async fn wait_for_server_startup(
    server: &NetGetServer,
    timeout_duration: Duration,
    protocol_name: &str,
) -> E2EResult<()> {
    let start = std::time::Instant::now();

    while start.elapsed() < timeout_duration {
        // Check if server output contains the protocol name
        if server.output_contains(protocol_name).await {
            println!("  [WAIT] Server ready with {} protocol after {:?}", protocol_name, start.elapsed());
            return Ok(());
        }

        // Check for common ready indicators
        if server.output_contains("Server is running").await ||
           server.output_contains("listening on").await ||
           server.output_contains("advertising").await {
            println!("  [WAIT] Server ready after {:?}", start.elapsed());
            return Ok(());
        }

        // Small delay before checking again
        sleep(Duration::from_millis(100)).await;
    }

    // Timeout - show output for debugging
    let output = server.get_output().await;
    eprintln!("  [ERROR] Server startup timeout. Last 20 lines of output:");
    for line in output.iter().rev().take(20).rev() {
        eprintln!("    {}", line);
    }

    Err(format!(
        "Server did not become ready within {:?}. Expected to see '{}' in output.",
        timeout_duration, protocol_name
    ).into())
}

/// Assert that the server is using the expected protocol stack
///
/// # Arguments
/// * `server` - The NetGetServer instance to check
/// * `expected_stack` - Expected stack name (e.g., "TCP", "HTTP", "IMAP")
///
/// # Panics
/// * If the server is not using the expected stack
///
/// # Example
/// ```rust,ignore
/// let server = start_netget_server(config).await?;
/// assert_stack_name(&server, "HTTP");
/// ```
pub fn assert_stack_name(server: &NetGetServer, expected_stack: &str) {
    assert_eq!(
        server.stack, expected_stack,
        "Expected stack '{}' but got '{}'",
        expected_stack, server.stack
    );
}

/// Get all captured output lines from the server
///
/// This is a convenience function that returns owned Vec<String> instead of
/// requiring async/await for simple access patterns.
///
/// # Arguments
/// * `server` - The NetGetServer instance
///
/// # Returns
/// * Vector of output lines captured since server start
///
/// # Example
/// ```rust,ignore
/// let server = start_netget_server(config).await?;
/// tokio::time::sleep(Duration::from_secs(2)).await;
/// let output = get_server_output(&server).await;
/// assert!(output.iter().any(|line| line.contains("ready")));
/// ```
pub async fn get_server_output(server: &NetGetServer) -> Vec<String> {
    server.get_output().await
}