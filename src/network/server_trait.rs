//! Common trait and utilities for network servers

use anyhow::Result;
use async_trait::async_trait;

/// Common trait for network servers that can be spawned
#[async_trait]
pub trait NetworkServer: Send + Sync + 'static {
    /// Start the server and run its main loop
    async fn start(self) -> Result<()>;

    /// Get the server name for logging
    fn name(&self) -> &str;
}