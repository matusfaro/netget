//! Official Tor Client Wrapper for E2E Testing
//!
//! This module provides a wrapper around the official `tor` binary for testing
//! NetGet's Tor Directory and Relay implementations with a real Tor client.

use anyhow::Result;
use std::path::PathBuf;
use tempfile::TempDir;

/// Tor client instance with configuration and process management
pub struct TorClient {
    /// Temporary directory for Tor data and configuration
    temp_dir: TempDir,
    /// Path to torrc configuration file
    torrc_path: PathBuf,
    /// Tor process handle (None if not started)
    process: Option<tokio::process::Child>,
    /// SOCKS5 proxy port
    pub socks_port: u16,
}

impl TorClient {
    /// Create a new Tor client configuration
    ///
    /// # Arguments
    /// * `dir_port` - NetGet Tor Directory port
    /// * `relay_port` - NetGet Tor Relay OR port
    /// * `v3_ident` - Authority v3 identity fingerprint (40 hex chars)
    /// * `fingerprint` - Authority fingerprint (40 hex chars)
    ///
    /// # Returns
    /// Configured TorClient (not yet started)
    pub fn new(
        dir_port: u16,
        relay_port: u16,
        v3_ident: &str,
        fingerprint: &str,
    ) -> Result<Self> {
        // Create temporary directory
        let temp_dir = TempDir::new()?;
        let data_dir = temp_dir.path().join("data");
        std::fs::create_dir_all(&data_dir)?;

        // Find available SOCKS port
        let socks_port = find_available_port()?;

        // Generate torrc configuration
        let torrc_content = format!(
            r#"# NetGet Tor Integration Test Configuration
# Generated for testing with custom Tor directory and relay

# Testing mode
TestingTorNetwork 1
AssumeReachable 1

# Ports
SocksPort 127.0.0.1:{socks_port}
ControlPort 0

# Directories
DataDirectory {data_dir}

# Logging
Log notice stdout

# Network settings
AddressDisableIPv6 1

# Custom Directory Authority
# Format: nickname orport=PORT no-v2 v3ident=FINGERPRINT IP:DirPort FINGERPRINT
DirAuthority netget orport={relay_port} no-v2 v3ident={v3_ident} 127.0.0.1:{dir_port} {fingerprint}

# Speed up testing
PathsNeededToBuildCircuits 0.25
TestingDirAuthVoteExit *
TestingDirAuthVoteHSDir *
V3AuthNIntervalsValid 2

# Disable network timeout
LearnCircuitBuildTimeout 0
CircuitBuildTimeout 60
"#,
            socks_port = socks_port,
            data_dir = data_dir.display(),
            relay_port = relay_port,
            v3_ident = v3_ident,
            dir_port = dir_port,
            fingerprint = fingerprint,
        );

        let torrc_path = temp_dir.path().join("torrc");
        std::fs::write(&torrc_path, torrc_content)?;

        println!("[TOR CLIENT] Configuration written to: {}", torrc_path.display());
        println!("[TOR CLIENT] SOCKS port: {}", socks_port);

        Ok(Self {
            temp_dir,
            torrc_path,
            process: None,
            socks_port,
        })
    }

    /// Check if tor binary is available in PATH
    pub fn is_tor_available() -> bool {
        std::process::Command::new("tor")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
    }

    /// Start the Tor client process
    ///
    /// # Returns
    /// Ok if tor started successfully
    pub async fn start(&mut self) -> Result<()> {
        if self.process.is_some() {
            anyhow::bail!("Tor client already started");
        }

        println!("[TOR CLIENT] Starting tor with config: {}", self.torrc_path.display());

        let process = tokio::process::Command::new("tor")
            .arg("-f")
            .arg(&self.torrc_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        println!("[TOR CLIENT] Tor process started with PID: {:?}", process.id());
        self.process = Some(process);

        Ok(())
    }

    /// Wait for Tor to bootstrap (become ready to proxy connections)
    ///
    /// Waits up to the specified timeout for bootstrap completion.
    /// Monitors stdout for "Bootstrapped 100%: Done" message.
    pub async fn wait_for_bootstrap(&mut self, timeout: std::time::Duration) -> Result<()> {
        use tokio::io::{AsyncBufReadExt, BufReader};
        use tokio::time::timeout as tokio_timeout;

        let process = self.process.as_mut()
            .ok_or_else(|| anyhow::anyhow!("Tor process not started"))?;

        let stdout = process.stdout.take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture stdout"))?;

        let mut reader = BufReader::new(stdout).lines();

        println!("[TOR CLIENT] Waiting for bootstrap (timeout: {:?})...", timeout);

        let bootstrap_future = async {
            while let Some(line) = reader.next_line().await? {
                println!("[TOR] {}", line);

                if line.contains("Bootstrapped 100%") {
                    println!("[TOR CLIENT] ✓ Bootstrap complete!");
                    return Ok::<(), anyhow::Error>(());
                }

                // Check for fatal errors
                if line.contains("[err]") || line.contains("ERROR") {
                    if line.contains("directory information") {
                        println!("[TOR CLIENT] ⚠ Directory fetch issue (may be expected)");
                    } else {
                        println!("[TOR CLIENT] ⚠ Error detected: {}", line);
                    }
                }
            }
            anyhow::bail!("Tor stdout ended before bootstrap complete")
        };

        match tokio_timeout(timeout, bootstrap_future).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => anyhow::bail!("Timeout waiting for Tor bootstrap after {:?}", timeout),
        }
    }

    /// Get the SOCKS5 proxy address for this Tor client
    pub fn socks_addr(&self) -> String {
        format!("127.0.0.1:{}", self.socks_port)
    }

    /// Stop the Tor client process
    pub async fn stop(&mut self) -> Result<()> {
        if let Some(mut process) = self.process.take() {
            println!("[TOR CLIENT] Stopping tor process...");
            process.kill().await?;
            process.wait().await?;
            println!("[TOR CLIENT] ✓ Tor process stopped");
        }
        Ok(())
    }
}

impl Drop for TorClient {
    fn drop(&mut self) {
        if let Some(mut process) = self.process.take() {
            // Best effort kill in Drop (synchronous)
            let _ = process.start_kill();
            println!("[TOR CLIENT] Cleanup initiated");
        }
    }
}

/// Find an available TCP port for SOCKS proxy
fn find_available_port() -> Result<u16> {
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener); // Release immediately
    Ok(port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tor_binary_check() {
        // Just verify the check runs (may pass or fail depending on environment)
        let _available = TorClient::is_tor_available();
    }

    #[test]
    fn test_tor_client_creation() {
        let client = TorClient::new(
            9030,
            9001,
            "0123456789abcdef0123456789abcdef01234567",
            "0123456789abcdef0123456789abcdef01234567",
        );
        assert!(client.is_ok());
    }
}
