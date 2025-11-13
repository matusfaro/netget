// Client-specific test helpers

use std::time::Duration;
use tokio::process::Child;
use tokio::time::sleep;

use super::common::*;
use super::netget::NetGetConfig;

/// A running NetGet client process
#[allow(dead_code)]
pub struct NetGetClient {
    /// The child process
    child: Child,
    /// Client ID
    pub id: String,
    /// Protocol name (e.g., "TCP", "HTTP")
    pub protocol: String,
    /// Remote address being connected to
    pub remote_addr: String,
    /// Local address (after connection succeeds)
    pub local_addr: Option<String>,
    /// Captured client output lines (for verification)
    pub output_lines: std::sync::Arc<tokio::sync::Mutex<Vec<String>>>,
    /// Mock configuration (if mocks were used)
    mock_config: Option<netget::testing::MockLlmConfig>,
}

impl NetGetClient {
    /// Create a new NetGetClient instance
    #[allow(dead_code)]
    pub(crate) fn new(
        child: Child,
        id: String,
        protocol: String,
        remote_addr: String,
        local_addr: Option<String>,
        output_lines: std::sync::Arc<tokio::sync::Mutex<Vec<String>>>,
        mock_config: Option<netget::testing::MockLlmConfig>,
    ) -> Self {
        Self {
            child,
            id,
            protocol,
            remote_addr,
            local_addr,
            output_lines,
            mock_config,
        }
    }

    /// Stop the client gracefully
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

    /// Check if the client is still running
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
    /// Must be called before dropping the client if mocks were configured.
    /// Fails if any expectation is not met.
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

            return Err(format!("Mock verification failed: {} errors", errors.len()).into());
        }

        Ok(())
    }
}

impl Drop for NetGetClient {
    fn drop(&mut self) {
        if let Some(ref mock_config) = self.mock_config {
            if !mock_config.is_verified() {
                eprintln!("\n⚠️  WARNING: Client dropped without calling .verify_mocks()!");
                eprintln!("   Mock expectations may not have been checked.");
                eprintln!("   Add `client.verify_mocks().await?;` before dropping the client.\n");
            }
        }
    }
}

/// Start a NetGet client with the given configuration
/// Asserts exactly 1 client and 0 servers were started
#[allow(dead_code)]
pub async fn start_netget_client(config: NetGetConfig) -> E2EResult<NetGetClient> {
    let instance = super::netget::start_netget(config).await?;

    // Validate expectations
    if !instance.servers.is_empty() {
        return Err(format!(
            "Expected 0 servers, got {}. Prompt started unexpected servers.",
            instance.servers.len()
        )
        .into());
    }

    if instance.clients.len() != 1 {
        return Err(format!(
            "Expected exactly 1 client, got {}. Use start_netget() for multiple clients.",
            instance.clients.len()
        )
        .into());
    }

    let client = instance.clients.into_iter().next().unwrap();

    Ok(NetGetClient::new(
        instance.child,
        client.id,
        client.protocol,
        client.remote_addr,
        client.local_addr,
        instance.output_lines,
        instance.mock_config,
    ))
}

/// Wait for client to be ready and responsive
///
/// This function waits for the client to have specific output indicating it's ready.
/// The client is considered ready when output contains "connected" or similar messages.
///
/// # Arguments
/// * `client` - The NetGetClient instance to check
/// * `timeout_duration` - Maximum time to wait for client readiness
///
/// # Returns
/// * `Ok(())` if client is ready within timeout
/// * `Err(_)` if timeout expires before client is ready
///
/// # Example
/// ```rust,ignore
/// let client = start_netget_client(config).await?;
/// wait_for_client_startup(&client, Duration::from_secs(10)).await?;
/// ```
#[allow(dead_code)]
pub async fn wait_for_client_startup(
    client: &NetGetClient,
    timeout_duration: Duration,
) -> E2EResult<()> {
    let start = std::time::Instant::now();

    while start.elapsed() < timeout_duration {
        // Check if client output contains connection confirmation
        if client.output_contains("connected").await {
            println!(
                "  [WAIT] Client {} ready after {:?}",
                client.protocol,
                start.elapsed()
            );
            return Ok(());
        }

        // Check for common ready indicators
        if client.output_contains("Client is connected").await
            || client.output_contains("Connection established").await
        {
            println!("  [WAIT] Client ready after {:?}", start.elapsed());
            return Ok(());
        }

        // Small delay before checking again
        sleep(Duration::from_millis(100)).await;
    }

    // Timeout - show output for debugging
    let output = client.get_output().await;
    eprintln!("  [ERROR] Client startup timeout. Last 20 lines of output:");
    for line in output.iter().rev().take(20).rev() {
        eprintln!("    {}", line);
    }

    Err(format!("Client did not become ready within {:?}", timeout_duration).into())
}

/// Assert that the client is using the expected protocol
///
/// # Arguments
/// * `client` - The NetGetClient instance to check
/// * `expected_protocol` - Expected protocol name (e.g., "TCP", "HTTP")
///
/// # Panics
/// * If the client is not using the expected protocol
///
/// # Example
/// ```rust,ignore
/// let client = start_netget_client(config).await?;
/// assert_protocol(&client, "TCP");
/// ```
#[allow(dead_code)]
pub fn assert_protocol(client: &NetGetClient, expected_protocol: &str) {
    assert_eq!(
        client.protocol, expected_protocol,
        "Expected protocol '{}' but got '{}'",
        expected_protocol, client.protocol
    );
}

/// Get all captured output lines from the client
///
/// This is a convenience function that returns owned Vec<String> instead of
/// requiring async/await for simple access patterns.
///
/// # Arguments
/// * `client` - The NetGetClient instance
///
/// # Returns
/// * Vector of output lines captured since client start
///
/// # Example
/// ```rust,ignore
/// let client = start_netget_client(config).await?;
/// tokio::time::sleep(Duration::from_secs(2)).await;
/// let output = get_client_output(&client).await;
/// assert!(output.iter().any(|line| line.contains("connected")));
/// ```
#[allow(dead_code)]
pub async fn get_client_output(client: &NetGetClient) -> Vec<String> {
    client.get_output().await
}
