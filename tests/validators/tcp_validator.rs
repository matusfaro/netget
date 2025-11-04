//! TCP protocol validator for E2E tests

use anyhow::{Context, Result};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

/// TCP protocol validator with assertion helpers
pub struct TcpValidator {
    addr: String,
}

impl TcpValidator {
    /// Create a new TCP validator for the given port
    pub fn new(port: u16) -> Self {
        Self {
            addr: format!("127.0.0.1:{}", port),
        }
    }

    /// Connect to the TCP server
    pub async fn connect(&self) -> Result<TcpStream> {
        TcpStream::connect(&self.addr)
            .await
            .with_context(|| format!("Failed to connect to {}", self.addr))
    }

    /// Send data and receive response
    pub async fn send_receive(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut stream = self.connect().await?;

        // Send data
        stream.write_all(data).await
            .context("Failed to send data")?;

        // Read response (with timeout)
        let mut buffer = vec![0u8; 4096];
        let n = timeout(Duration::from_secs(5), stream.read(&mut buffer))
            .await
            .context("Read timeout")?
            .context("Failed to read response")?;

        buffer.truncate(n);
        Ok(buffer)
    }

    /// Send text and receive text response
    pub async fn send_receive_text(&self, text: &str) -> Result<String> {
        let response = self.send_receive(text.as_bytes()).await?;
        String::from_utf8(response)
            .context("Response is not valid UTF-8")
    }

    /// Assert that sending data gets expected response
    pub async fn expect_response(&self, send: &str, expected: &str) -> Result<()> {
        let actual = self.send_receive_text(send).await?;

        if actual != expected {
            anyhow::bail!(
                "Expected response '{}', got '{}'",
                expected,
                actual
            );
        }

        Ok(())
    }

    /// Assert that response contains expected text
    pub async fn expect_contains(&self, send: &str, expected: &str) -> Result<()> {
        let response = self.send_receive_text(send).await?;

        if !response.contains(expected) {
            anyhow::bail!(
                "Expected response to contain '{}', got '{}'",
                expected,
                response
            );
        }

        Ok(())
    }

    /// Check if server is reachable
    pub async fn is_reachable(&self) -> bool {
        TcpStream::connect(&self.addr).await.is_ok()
    }

    /// Wait for server to become reachable
    pub async fn wait_for_ready(&self, max_attempts: u32) -> Result<()> {
        for i in 0..max_attempts {
            if self.is_reachable().await {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;

            if i % 5 == 0 && i > 0 {
                println!("Still waiting for TCP server to be ready... (attempt {})", i);
            }
        }

        anyhow::bail!("TCP server not reachable after {} attempts", max_attempts)
    }

    /// Test echo functionality
    pub async fn test_echo(&self, data: &str) -> Result<()> {
        self.expect_response(data, data).await
    }

    /// Send multiple messages in sequence
    pub async fn send_sequence(&self, messages: &[&str]) -> Result<Vec<String>> {
        let mut stream = self.connect().await?;
        let mut responses = Vec::new();

        for msg in messages {
            // Send message
            stream.write_all(msg.as_bytes()).await
                .context("Failed to send message")?;

            // Read response
            let mut buffer = vec![0u8; 1024];
            let n = timeout(Duration::from_secs(2), stream.read(&mut buffer))
                .await
                .context("Read timeout")?
                .context("Failed to read response")?;

            buffer.truncate(n);
            let response = String::from_utf8(buffer)
                .context("Response is not valid UTF-8")?;
            responses.push(response);
        }

        Ok(responses)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validator_creation() {
        let validator = TcpValidator::new(8080);
        assert!(validator.addr.contains("8080"));
    }
}