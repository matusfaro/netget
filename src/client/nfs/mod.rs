//! NFS client implementation
pub mod actions;

pub use actions::NfsClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use serde_json::json;

#[cfg(feature = "nfs")]
use nfs3_client::tokio::TokioConnector;
#[cfg(feature = "nfs")]
use nfs3_client::Nfs3ConnectionBuilder;

use crate::client::nfs::actions::NFS_CLIENT_CONNECTED_EVENT;

/// NFS client that connects to a remote NFS server
pub struct NfsClient;

impl NfsClient {
    /// Connect to an NFS server with integrated LLM actions
    #[cfg(feature = "nfs")]
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Parse remote_addr into server and export path
        // Format: server:port:/export/path or server:/export/path (default port 2049)
        let (server_addr, export_path) = Self::parse_nfs_address(&remote_addr)?;

        info!("NFS client {} attempting to connect to {} for export {}", client_id, server_addr, export_path);
        let _ = status_tx.send(format!("[CLIENT] NFS client {} connecting to {}", client_id, server_addr));

        // Extract just the server part (remove port if present)
        let server = server_addr.split(':').next().unwrap_or(&server_addr);

        // Mount the NFS export
        let connection = Nfs3ConnectionBuilder::new(TokioConnector, server, &export_path)
            .mount()
            .await
            .context("Failed to mount NFS export")?;

        info!("NFS client {} successfully mounted {}", client_id, export_path);
        let _ = status_tx.send(format!("[CLIENT] NFS client {} mounted export {}", client_id, export_path));

        // Get root file handle - nfs3_client uses root_nfs_fh3()
        let root_fh = connection.root_nfs_fh3();
        let root_fh_hex = hex::encode(&root_fh.data.0);

        // Update client status to connected
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Get instruction for initial LLM call
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(NfsClientProtocol::new());

            // Send connected event to LLM
            let connected_event = Event::new(
                &NFS_CLIENT_CONNECTED_EVENT,
                json!({
                    "export_path": export_path,
                    "root_fh": root_fh_hex
                }),
            );

            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&connected_event),
                protocol.as_ref(),
                &status_tx,
            )
            .await {
                Ok(result) => {
                    // Update memory if provided
                    if let Some(new_memory) = result.memory_updates {
                        app_state.set_memory_for_client(client_id, new_memory).await;
                    }

                    // Note: Actions from the LLM would need to be executed here
                    // For now, NFS operations are not yet implemented
                    info!("NFS client {} received {} actions from LLM", client_id, result.actions.len());
                }
                Err(e) => {
                    info!("NFS client {} LLM call failed: {}", client_id, e);
                }
            }
        }

        // Spawn monitoring task
        let app_state_clone = app_state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state_clone.get_client(client_id).await.is_none() {
                    info!("NFS client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return a dummy socket address (NFS doesn't use direct sockets)
        Ok(format!("{}:2049", server).parse()?)
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
