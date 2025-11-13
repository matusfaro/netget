// Shared test utilities for NetGet E2E testing

use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::sleep;

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
#[allow(dead_code)]
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
                    println!(
                        "  [RETRY] Condition succeeded after {} attempts in {:?}",
                        attempts,
                        start.elapsed()
                    );
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
#[allow(dead_code)]
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

/// Replace {AVAILABLE_PORT} placeholders with actual available ports
pub async fn replace_port_placeholders(prompt: &str) -> E2EResult<String> {
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

/// Get the path to the NetGet binary
pub fn get_netget_binary_path() -> E2EResult<PathBuf> {
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

/// Kill all running netget processes (useful for cleanup)
#[allow(dead_code)]
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

/// Helper to build a simple test prompt
#[allow(dead_code)]
pub fn build_prompt(base_stack: &str, port: u16, instructions: &str) -> String {
    if port == 0 {
        format!("listen on port 0 via {}. {}", base_stack, instructions)
    } else {
        format!(
            "listen on port {} via {}. {}",
            port, base_stack, instructions
        )
    }
}
