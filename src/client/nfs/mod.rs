//! NFS client implementation
pub mod actions;

pub use actions::NfsClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::nfs::actions::{NFS_CLIENT_CONNECTED_EVENT, NFS_CLIENT_OPERATION_RESULT_EVENT};

#[cfg(feature = "nfs")]
use nfs3_client::{Client as Nfs3Client, MountClient};

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-client data for LLM handling
struct ClientData {
    state: ConnectionState,
    memory: String,
}

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

        info!("NFS client {} connecting to {} for export {}", client_id, server_addr, export_path);
        let _ = status_tx.send(format!("[CLIENT] NFS client {} connecting to {}", client_id, server_addr));

        // Connect TCP stream to server
        let tcp_stream = tokio::net::TcpStream::connect(&server_addr)
            .await
            .context(format!("Failed to connect to NFS server {}", server_addr))?;

        let local_addr = tcp_stream.local_addr()?;

        // Create mount client with TCP stream
        let mut mount_client = MountClient::new(tcp_stream);

        // Mount the export
        let mount_result = mount_client.mount(&export_path)
            .await
            .context(format!("Failed to mount NFS export {}", export_path))?;

        info!("NFS client {} mounted export {} successfully", client_id, export_path);
        let _ = status_tx.send(format!("[CLIENT] NFS client {} mounted export {}", client_id, export_path));

        // Connect another TCP stream for NFS operations
        let nfs_tcp_stream = tokio::net::TcpStream::connect(&server_addr)
            .await
            .context("Failed to create second connection for NFS operations")?;

        // Create NFS client with the file handle
        let nfs_client = Nfs3Client::new(nfs_tcp_stream, mount_result.fhandle);

        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] NFS client {} connected", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Initialize client data
        let client_data = Arc::new(Mutex::new(ClientData {
            state: ConnectionState::Idle,
            memory: String::new(),
        }));

        let nfs_client_arc = Arc::new(Mutex::new(nfs_client));
        let protocol = Arc::new(NfsClientProtocol::new());

        // Send initial connected event to LLM
        let mount_event = Event::new(
            &NFS_CLIENT_CONNECTED_EVENT,
            serde_json::json!({
                "export_path": export_path,
                "root_fh": hex::encode(&mount_result.fhandle),
            }),
        );

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &client_data.lock().await.memory,
                Some(&mount_event),
                protocol.as_ref(),
                &status_tx,
            ).await {
                Ok(ClientLlmResult { actions, memory_updates }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        client_data.lock().await.memory = mem;
                    }

                    // Execute initial actions
                    Self::execute_actions(
                        actions,
                        &nfs_client_arc,
                        &protocol,
                        &llm_client,
                        &app_state,
                        &status_tx,
                        client_id,
                        &client_data,
                    ).await;
                }
                Err(e) => {
                    error!("LLM error for NFS client {}: {}", client_id, e);
                }
            }
        }

        // Spawn action processing loop
        tokio::spawn(async move {
            info!("NFS client {} action processing loop started", client_id);

            // NFS client is command-driven, so we don't have a read loop
            // Instead, we periodically check for pending actions or wait for user commands
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

                // Check if client is still connected
                let client = app_state.get_client(client_id).await;
                if client.map(|c| c.status) != Some(ClientStatus::Connected) {
                    info!("NFS client {} disconnected", client_id);
                    break;
                }
            }

            info!("NFS client {} action processing loop ended", client_id);
        });

        Ok(local_addr)
    }

    /// Execute NFS client actions
    #[cfg(feature = "nfs")]
    async fn execute_actions(
        actions: Vec<serde_json::Value>,
        nfs_client: &Arc<Mutex<Nfs3Client>>,
        protocol: &Arc<NfsClientProtocol>,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        client_id: ClientId,
        client_data: &Arc<Mutex<ClientData>>,
    ) {
        use crate::llm::actions::client_trait::Client;

        for action in actions {
            match protocol.as_ref().execute_action(action.clone()) {
                Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data }) => {
                    // Handle NFS-specific operations
                    let result = match name.as_str() {
                        "nfs_lookup" => Self::handle_lookup(nfs_client, &data).await,
                        "nfs_read_file" => Self::handle_read_file(nfs_client, &data).await,
                        "nfs_write_file" => Self::handle_write_file(nfs_client, &data).await,
                        "nfs_list_dir" => Self::handle_list_dir(nfs_client, &data).await,
                        "nfs_get_attr" => Self::handle_get_attr(nfs_client, &data).await,
                        "nfs_create_file" => Self::handle_create_file(nfs_client, &data).await,
                        "nfs_mkdir" => Self::handle_mkdir(nfs_client, &data).await,
                        "nfs_remove" => Self::handle_remove(nfs_client, &data).await,
                        "nfs_rmdir" => Self::handle_rmdir(nfs_client, &data).await,
                        _ => {
                            error!("Unknown NFS action: {}", name);
                            continue;
                        }
                    };

                    // Send result back to LLM
                    if let Ok(result_data) = result {
                        let event = Event::new(
                            &NFS_CLIENT_OPERATION_RESULT_EVENT,
                            serde_json::json!({
                                "operation": name,
                                "result": result_data,
                            }),
                        );

                        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                            match call_llm_for_client(
                                llm_client,
                                app_state,
                                client_id.to_string(),
                                &instruction,
                                &client_data.lock().await.memory,
                                Some(&event),
                                protocol.as_ref(),
                                status_tx,
                            ).await {
                                Ok(ClientLlmResult { actions: new_actions, memory_updates }) => {
                                    // Update memory
                                    if let Some(mem) = memory_updates {
                                        client_data.lock().await.memory = mem;
                                    }

                                    // Recursively execute new actions
                                    Self::execute_actions(
                                        new_actions,
                                        nfs_client,
                                        protocol,
                                        llm_client,
                                        app_state,
                                        status_tx,
                                        client_id,
                                        client_data,
                                    ).await;
                                }
                                Err(e) => {
                                    error!("LLM error for NFS client {}: {}", client_id, e);
                                }
                            }
                        }
                    }
                }
                Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                    info!("NFS client {} disconnecting", client_id);
                    app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                    let _ = status_tx.send("__UPDATE_UI__".to_string());
                    break;
                }
                _ => {}
            }
        }
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


    // NFS operation handlers

    #[cfg(feature = "nfs")]
    async fn handle_lookup(
        nfs_client: &Arc<Mutex<Nfs3Client>>,
        action: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let path = action.get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' field")?;

        let client = nfs_client.lock().await;
        let fh = client.lookup_path(path).await
            .context(format!("Failed to lookup path: {}", path))?;

        trace!("NFS lookup succeeded for path: {}", path);
        Ok(serde_json::json!({
            "path": path,
            "fh": hex::encode(&fh),
        }))
    }

    #[cfg(feature = "nfs")]
    async fn handle_read_file(
        nfs_client: &Arc<Mutex<Nfs3Client>>,
        action: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let path = action.get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' field")?;

        let offset = action.get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let count = action.get("count")
            .and_then(|v| v.as_u64())
            .unwrap_or(4096) as u32;

        let client = nfs_client.lock().await;
        let fh = client.lookup_path(path).await
            .context(format!("Failed to lookup path: {}", path))?;

        let (data, eof) = client.read(&fh, offset, count).await
            .context(format!("Failed to read file: {}", path))?;

        let data_str = String::from_utf8_lossy(&data).to_string();
        debug!("NFS read {} bytes from {}, eof={}", data.len(), path, eof);

        Ok(serde_json::json!({
            "path": path,
            "data": data_str,
            "bytes_read": data.len(),
            "eof": eof,
        }))
    }

    #[cfg(feature = "nfs")]
    async fn handle_write_file(
        nfs_client: &Arc<Mutex<Nfs3Client>>,
        action: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let path = action.get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' field")?;

        let data_str = action.get("data")
            .and_then(|v| v.as_str())
            .context("Missing 'data' field")?;

        let offset = action.get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let data = data_str.as_bytes();

        let client = nfs_client.lock().await;
        let fh = client.lookup_path(path).await
            .context(format!("Failed to lookup path: {}", path))?;

        let _attrs = client.write(&fh, offset, data).await
            .context(format!("Failed to write file: {}", path))?;

        debug!("NFS wrote {} bytes to {}", data.len(), path);

        Ok(serde_json::json!({
            "path": path,
            "bytes_written": data.len(),
        }))
    }

    #[cfg(feature = "nfs")]
    async fn handle_list_dir(
        nfs_client: &Arc<Mutex<Nfs3Client>>,
        action: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let path = action.get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("/");

        let client = nfs_client.lock().await;
        let fh = client.lookup_path(path).await
            .context(format!("Failed to lookup path: {}", path))?;

        let entries = client.readdir(&fh, 0, 100).await
            .context(format!("Failed to read directory: {}", path))?;

        let entry_list: Vec<serde_json::Value> = entries.entries.iter().map(|e| {
            let name = String::from_utf8_lossy(&e.name).to_string();
            serde_json::json!({
                "name": name,
                "fileid": e.fileid,
            })
        }).collect();

        debug!("NFS listed {} entries in {}", entry_list.len(), path);

        Ok(serde_json::json!({
            "path": path,
            "entries": entry_list,
        }))
    }

    #[cfg(feature = "nfs")]
    async fn handle_get_attr(
        nfs_client: &Arc<Mutex<Nfs3Client>>,
        action: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let path = action.get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' field")?;

        let client = nfs_client.lock().await;
        let fh = client.lookup_path(path).await
            .context(format!("Failed to lookup path: {}", path))?;

        let attrs = client.getattr(&fh).await
            .context(format!("Failed to get attributes: {}", path))?;

        debug!("NFS got attributes for {}", path);

        Ok(serde_json::json!({
            "path": path,
            "size": attrs.size,
            "mode": attrs.mode,
            "uid": attrs.uid,
            "gid": attrs.gid,
            "mtime": attrs.mtime.seconds,
        }))
    }

    #[cfg(feature = "nfs")]
    async fn handle_create_file(
        nfs_client: &Arc<Mutex<Nfs3Client>>,
        action: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let path = action.get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' field")?;

        let mode = action.get("mode")
            .and_then(|v| v.as_u64())
            .unwrap_or(0o644) as u32;

        // Split path into directory and filename
        let (dir_path, filename) = Self::split_path(path)?;

        let client = nfs_client.lock().await;
        let dir_fh = client.lookup_path(&dir_path).await
            .context(format!("Failed to lookup directory: {}", dir_path))?;

        let (_fh, _attrs) = client.create(&dir_fh, filename.as_bytes(), mode).await
            .context(format!("Failed to create file: {}", path))?;

        debug!("NFS created file: {}", path);

        Ok(serde_json::json!({
            "path": path,
            "created": true,
        }))
    }

    #[cfg(feature = "nfs")]
    async fn handle_mkdir(
        nfs_client: &Arc<Mutex<Nfs3Client>>,
        action: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let path = action.get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' field")?;

        let mode = action.get("mode")
            .and_then(|v| v.as_u64())
            .unwrap_or(0o755) as u32;

        // Split path into parent directory and dirname
        let (dir_path, dirname) = Self::split_path(path)?;

        let client = nfs_client.lock().await;
        let dir_fh = client.lookup_path(&dir_path).await
            .context(format!("Failed to lookup directory: {}", dir_path))?;

        let (_fh, _attrs) = client.mkdir(&dir_fh, dirname.as_bytes(), mode).await
            .context(format!("Failed to create directory: {}", path))?;

        debug!("NFS created directory: {}", path);

        Ok(serde_json::json!({
            "path": path,
            "created": true,
        }))
    }

    #[cfg(feature = "nfs")]
    async fn handle_remove(
        nfs_client: &Arc<Mutex<Nfs3Client>>,
        action: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let path = action.get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' field")?;

        // Split path into directory and filename
        let (dir_path, filename) = Self::split_path(path)?;

        let client = nfs_client.lock().await;
        let dir_fh = client.lookup_path(&dir_path).await
            .context(format!("Failed to lookup directory: {}", dir_path))?;

        client.remove(&dir_fh, filename.as_bytes()).await
            .context(format!("Failed to remove file: {}", path))?;

        debug!("NFS removed file: {}", path);

        Ok(serde_json::json!({
            "path": path,
            "removed": true,
        }))
    }

    #[cfg(feature = "nfs")]
    async fn handle_rmdir(
        nfs_client: &Arc<Mutex<Nfs3Client>>,
        action: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let path = action.get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' field")?;

        // Split path into parent directory and dirname
        let (dir_path, dirname) = Self::split_path(path)?;

        let client = nfs_client.lock().await;
        let dir_fh = client.lookup_path(&dir_path).await
            .context(format!("Failed to lookup directory: {}", dir_path))?;

        client.rmdir(&dir_fh, dirname.as_bytes()).await
            .context(format!("Failed to remove directory: {}", path))?;

        debug!("NFS removed directory: {}", path);

        Ok(serde_json::json!({
            "path": path,
            "removed": true,
        }))
    }

    /// Split a path into parent directory and filename
    fn split_path(path: &str) -> Result<(String, String)> {
        let path = path.trim_start_matches('/');
        if let Some(pos) = path.rfind('/') {
            let dir = path[..pos].to_string();
            let name = path[pos + 1..].to_string();
            Ok((format!("/{}", dir), name))
        } else {
            // Path is in root directory
            Ok(("/".to_string(), path.to_string()))
        }
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
