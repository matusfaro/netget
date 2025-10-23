//! E2E test helpers for NetGet binary testing
//!
//! This module provides utilities to test NetGet by spawning the actual binary
//! and interacting with it as a black-box system.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};

/// Result type for e2e tests
pub type E2EResult<T> = Result<T, Box<dyn std::error::Error>>;

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
}

impl ServerConfig {
    /// Create a new server config with the given prompt
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            model: None,
            log_level: "off".to_string(),
            listen_addr: "127.0.0.1".to_string(),
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
}

/// A running NetGet server process
pub struct NetGetServer {
    /// The child process
    child: Child,
    /// The port the server is listening on
    pub port: u16,
    /// The actual protocol stack that was started
    pub stack: String,
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
}

/// Start a NetGet server with the given configuration
pub async fn start_netget_server(config: ServerConfig) -> E2EResult<NetGetServer> {
    // Get the path to the binary
    let binary_path = get_netget_binary_path()?;

    // Build command arguments
    let mut cmd = Command::new(binary_path);

    // Add model flag if specified, otherwise use the default
    if let Some(model) = &config.model {
        cmd.arg("--model").arg(model);
    } else {
        // Use the same default as the application
        cmd.arg("--model").arg("qwen3-coder:30b");
    }

    // Add log level
    cmd.arg("--log-level").arg(&config.log_level);

    // Add listen address
    cmd.arg("--listen-addr").arg(&config.listen_addr);

    // Add the prompt as trailing arguments
    // We'll pass it as separate words to match how a user would type it
    for word in config.prompt.split_whitespace() {
        cmd.arg(word);
    }

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

    // Also spawn a task to read stderr for debugging
    tokio::spawn(async move {
        let mut stderr_reader = BufReader::new(stderr).lines();
        while let Some(line) = stderr_reader.next_line().await.ok().flatten() {
            println!("[DEBUG stderr] {}", line);
        }
    });

    // Parse the prompt to find the expected port and stack
    let expected_port = extract_port_from_prompt(&config.prompt);
    let expected_stack = extract_stack_from_prompt(&config.prompt);

    // Wait for the server to start and parse the actual configuration
    let (actual_port, actual_stack) = wait_for_server_startup(&mut reader).await?;

    // Validate that the server started with the expected stack
    if let Some(expected) = &expected_stack {
        if !actual_stack.to_lowercase().contains(&expected.to_lowercase()) {
            return Err(format!(
                "Server started with wrong stack! Expected: {}, Got: {}",
                expected, actual_stack
            ).into());
        }
    }

    // Validate port if a specific port was requested
    if expected_port != 0 && actual_port != expected_port {
        return Err(format!(
            "Server started on wrong port! Expected: {}, Got: {}",
            expected_port, actual_port
        ).into());
    }

    // Wait for the port to actually open
    // Determine if this is a UDP-based protocol
    let is_udp = actual_stack.to_lowercase().contains("snmp")
        || actual_stack.to_lowercase().contains("dns")
        || actual_stack.to_lowercase().contains("dhcp")
        || actual_stack.to_lowercase().contains("ntp")
        || actual_stack.to_lowercase().contains("udp");

    wait_for_port_open("127.0.0.1", actual_port, is_udp).await?;

    Ok(NetGetServer {
        child,
        port: actual_port,
        stack: actual_stack,
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

/// Extract expected stack from prompt
fn extract_stack_from_prompt(prompt: &str) -> Option<String> {
    let prompt_lower = prompt.to_lowercase();

    // Look for various stack patterns
    if prompt_lower.contains("http stack") || prompt_lower.contains("via http") {
        Some("HTTP".to_string())
    } else if prompt_lower.contains("tcp") || prompt_lower.contains("ftp") {
        Some("TCP".to_string())
    } else if prompt_lower.contains("udp") {
        Some("UDP".to_string())
    } else {
        None
    }
}

/// Wait for server startup and extract port and stack info
async fn wait_for_server_startup(
    reader: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
) -> E2EResult<(u16, String)> {
    let wait_future = async {
        while let Some(line) = reader.next_line().await? {
            println!("[DEBUG] Server output: {}", line); // Debug output

            // Look for STATUS message pattern: "[STATUS] Starting <STACK> server on <ADDRESS>:<PORT>"
            if line.contains("[STATUS]") && line.contains("Starting") && line.contains("server on") {
                let mut stack = "Unknown".to_string();
                let mut port = 0u16;

                // Extract stack type
                if line.contains("HTTP") {
                    stack = "HTTP".to_string();
                } else if line.contains("SNMP") {
                    stack = "SNMP".to_string();
                } else if line.contains("DNS") {
                    stack = "DNS".to_string();
                } else if line.contains("DHCP") {
                    stack = "DHCP".to_string();
                } else if line.contains("NTP") {
                    stack = "NTP".to_string();
                } else if line.contains("SSH") {
                    stack = "SSH".to_string();
                } else if line.contains("IRC") {
                    stack = "IRC".to_string();
                } else if line.contains("TCP") || line.contains("TCP/IP") {
                    stack = "TCP".to_string();
                } else if line.contains("UDP") {
                    stack = "UDP".to_string();
                } else if line.contains("FTP") {
                    stack = "FTP".to_string();
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

                if port > 0 {
                    println!("[DEBUG] Server started: {} stack on port {}", stack, port);
                    return Ok((port, stack));
                }
            }

            // Also check for "Server is running" as a backup
            if line.contains("Server is running") {
                // We might not have the exact details yet, keep reading
                continue;
            }
        }
        Err("Server did not output startup information".into())
    };

    timeout(Duration::from_secs(30), wait_future)  // Increased timeout for LLM processing
        .await
        .map_err(|_| "Timeout waiting for server startup")?
}


/// Wait for a port to be open and accepting connections
async fn wait_for_port_open(host: &str, port: u16, is_udp: bool) -> E2EResult<()> {
    let addr = format!("{}:{}", host, port);

    for _ in 0..100 {  // Try for up to 10 seconds (LLM processing can take time)
        if is_udp {
            // For UDP, we can't "connect" like TCP, so we just try to bind a socket to verify
            // the port is in use. If bind fails with "address in use", the server is running.
            // Or we can just wait a bit and assume the server message means it's ready.
            match std::net::UdpSocket::bind(&addr) {
                Ok(_) => {
                    // Port is free, server not started yet
                    sleep(Duration::from_millis(100)).await;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                    println!("[DEBUG] UDP port {} is now bound and ready", port);
                    return Ok(());
                }
                Err(_) => sleep(Duration::from_millis(100)).await,
            }
        } else {
            // For TCP, try to connect
            match tokio::net::TcpStream::connect(&addr).await {
                Ok(_) => {
                    println!("[DEBUG] TCP port {} is now open and accepting connections", port);
                    return Ok(());
                },
                Err(_) => sleep(Duration::from_millis(100)).await,
            }
        }
    }

    Err(format!("Port {} did not open in time", port).into())
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