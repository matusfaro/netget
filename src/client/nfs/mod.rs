//! NFS client implementation
pub mod actions;

pub use actions::NfsClientProtocol;

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::llm::ollama_client::OllamaClient;
use crate::state::app_state::AppState;
use crate::state::ClientId;

// Note: nfs3_client API is currently incompatible
// The crate exists but has different API than documented in CLIENT_PROTOCOL_FEASIBILITY.md
// TODO: Research actual nfs3_client v0.7 API and implement properly
// Issues:
// - Client type not exported at crate root (maybe in submodule?)
// - MountClient takes IO types with nfs3_client::io::AsyncRead/AsyncWrite (not tokio's)
// - Need to figure out correct imports and trait bridging
// nfs3_client dependency is available but not yet integrated

/// NFS client that connects to a remote NFS server
pub struct NfsClient;

impl NfsClient {
    /// Connect to an NFS server with integrated LLM actions
    #[cfg(feature = "nfs")]
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Parse remote_addr into server and export path
        // Format: server:port:/export/path or server:/export/path (default port 2049)
        let (server_addr, export_path) = Self::parse_nfs_address(&remote_addr)?;

        info!("NFS client {} attempting to connect to {} for export {}", client_id, server_addr, export_path);
        let _ = status_tx.send(format!("[CLIENT] NFS client {} connecting to {}", client_id, server_addr));

        // TODO: Implement actual nfs3_client integration once API is clarified
        // Current nfs3_client v0.7 has incompatible API with tokio and unclear exports
        // Need to research actual API or consider alternative approach:
        // 1. Figure out correct nfs3_client imports (Client might be in submodule)
        // 2. Implement AsyncRead/AsyncWrite trait bridging with tokio_util::compat
        // 3. Or consider libnfs-rs (C bindings) as alternative
        // 4. Or implement raw NFS/RPC from scratch (significant work)
        error!("NFS client not fully implemented - nfs3_client API needs clarification");
        let _ = status_tx.send("[ERROR] NFS client not fully implemented yet".to_string());

        return Err(anyhow::anyhow!(
            "NFS client implementation incomplete - nfs3_client v0.7 API incompatible with current approach. \
            Needs research into actual crate API or alternative implementation strategy."
        ));
    }

    /// Parse NFS address into server address and export path
    /// Format: server:port:/export/path or server:/export/path (default port 2049)
    fn parse_nfs_address(addr: &str) -> Result<(String, String)> {
        // Split by last ':' to separate path from server:port
        if let Some(pos) = addr.rfind(':') {
            let (server_part, path_part) = addr.split_at(pos);
            let path = path_part.trim_start_matches(':');

            // Check if path starts with '/' - if so, it's the export path
            if path.starts_with('/') {
                let server = if !server_part.contains(':') {
                    format!("{}:2049", server_part)
                } else {
                    server_part.to_string()
                };
                return Ok((server, path.to_string()));
            }
        }

        // Default format: assume everything before last ':' is server, rest is path
        Err(anyhow::anyhow!(
            "Invalid NFS address format. Expected: server:port:/export/path or server:/export/path"
        ))
    }

    /// Connect to NFS server without the nfs feature (fallback)
    #[cfg(not(feature = "nfs"))]
    pub async fn connect_with_llm_actions(
        _remote_addr: String,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        _client_id: ClientId,
    ) -> Result<SocketAddr> {
        let _ = status_tx.send("[ERROR] NFS feature not enabled at compile time".to_string());
        Err(anyhow::anyhow!("NFS feature not enabled"))
    }
}
