// Server-specific test helpers and backward-compatible wrappers

use std::time::Duration;
use tokio::process::Child;
use tokio::time::sleep;

use super::common::*;

/// A running NetGet server process (backward compatible)
/// This wrapper maintains compatibility with the original NetGetServer struct
pub struct NetGetServer {
    /// The child process
    child: Child,
    /// The port the server is listening on
    pub port: u16,
    /// The actual protocol stack that was started
    pub stack: String,
    /// Captured server output lines (for verification)
    pub output_lines: std::sync::Arc<tokio::sync::Mutex<Vec<String>>>,
    /// Mock configuration (for verification)
    mock_config: Option<netget::testing::MockLlmConfig>,
}

impl NetGetServer {
    /// Create a new NetGetServer instance
    pub(crate) fn new(
        child: Child,
        port: u16,
        stack: String,
        output_lines: std::sync::Arc<tokio::sync::Mutex<Vec<String>>>,
        mock_config: Option<netget::testing::MockLlmConfig>,
    ) -> Self {
        Self {
            child,
            port,
            stack,
            output_lines,
            mock_config,
        }
    }

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

        match tokio::time::timeout(Duration::from_secs(5), shutdown).await {
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

    /// Verify all mock expectations were met
    ///
    /// Must be called before dropping the server if mocks were configured.
    /// Fails if any expectation is not met.
    ///
    /// # Example
    /// ```ignore
    /// let server = start_netget_server(config).await?;
    /// // ... test logic ...
    /// server.verify_mocks().await?;  // MANDATORY if mocks configured
    /// server.stop().await?;
    /// ```
    pub async fn verify_mocks(&self) -> E2EResult<()> {
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
}

impl Drop for NetGetServer {
    fn drop(&mut self) {
        if let Some(ref mock_config) = self.mock_config {
            // Check if verify was called
            if !mock_config.is_verified() {
                eprintln!("⚠️  WARNING: Mock expectations not verified!");
                eprintln!("   Call server.verify_mocks().await? before dropping");

                // Print unmet expectations
                for (idx, rule) in mock_config.rules.iter().enumerate() {
                    let actual = rule.actual_calls.load(std::sync::atomic::Ordering::SeqCst);
                    if let Some(expected) = rule.expected_calls {
                        if actual != expected {
                            eprintln!(
                                "   Rule #{}: Expected {} calls, got {} - {}",
                                idx,
                                expected,
                                actual,
                                rule.describe()
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Configuration for a server test (re-export for backward compatibility)
pub use super::netget::NetGetConfig as ServerConfig;


/// Start a NetGet server with the given configuration (backward compatible wrapper)
/// Asserts exactly 1 server and 0 clients were started
pub async fn start_netget_server(config: ServerConfig) -> E2EResult<NetGetServer> {
    let instance = super::netget::start_netget(config).await?;

    // Validate expectations
    if instance.servers.len() != 1 {
        return Err(format!(
            "Expected exactly 1 server, got {}. Use start_netget() for multiple servers.",
            instance.servers.len()
        )
        .into());
    }

    if !instance.clients.is_empty() {
        return Err(format!(
            "Expected 0 clients, got {}. Prompt started unexpected clients.",
            instance.clients.len()
        )
        .into());
    }

    let server = instance.servers.into_iter().next().unwrap();

    Ok(NetGetServer::new(
        instance.child,
        server.port,
        server.stack,
        instance.output_lines,
        instance.mock_config,
    ))
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
            println!(
                "  [WAIT] Server ready with {} protocol after {:?}",
                protocol_name,
                start.elapsed()
            );
            return Ok(());
        }

        // Check for common ready indicators
        if server.output_contains("Server is running").await
            || server.output_contains("listening on").await
            || server.output_contains("advertising").await
        {
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
    )
    .into())
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

/// Extract base_stack value from open_server prompt
pub fn extract_base_stack_from_prompt(prompt: &str) -> Option<String> {
    // Look for "base_stack <value>" pattern in the prompt
    let prompt_lower = prompt.to_lowercase();

    if let Some(stack_pos) = prompt_lower.find("base_stack ") {
        let after_stack = &prompt[stack_pos + "base_stack ".len()..];
        // Extract until we hit a period, newline, or other terminator
        let stack_value: String = after_stack
            .chars()
            .take_while(|c| !matches!(c, '.' | '\n' | ',' | ';'))
            .collect();

        let trimmed = stack_value.trim();
        if !trimmed.is_empty() {
            // Use the protocol registry to parse the stack name
            return netget::protocol::server_registry::registry().parse_from_str(trimmed);
        }
    }

    None
}

/// Extract port number from prompt
pub fn extract_port_from_prompt(prompt: &str) -> u16 {
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
