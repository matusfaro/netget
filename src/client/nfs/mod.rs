//! NFS client implementation
pub mod actions;

pub use actions::NfsClientProtocol;

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, trace};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::{Client as ClientTrait, ClientActionResult};
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use serde_json::json;

#[cfg(feature = "nfs")]
use nfs3_client::{tokio::TokioConnector, Nfs3ConnectionBuilder};
#[cfg(feature = "nfs")]
use nfs3_types::nfs3::*;

use crate::client::nfs::actions::{NFS_CLIENT_CONNECTED_EVENT, NFS_CLIENT_OPERATION_RESULT_EVENT};

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

        info!(
            "NFS client {} attempting to connect to {} for export {}",
            client_id, server_addr, export_path
        );
        let _ = status_tx.send(format!(
            "[CLIENT] NFS client {} connecting to {}",
            client_id, server_addr
        ));

        // Extract just the server part (remove port if present)
        let server = server_addr.split(':').next().unwrap_or(&server_addr);

        // Mount the NFS export
        let connection = Nfs3ConnectionBuilder::new(TokioConnector, server, &export_path)
            .mount()
            .await
            .context("Failed to mount NFS export")?;

        info!(
            "NFS client {} successfully mounted {}",
            client_id, export_path
        );
        let _ = status_tx.send(format!(
            "[CLIENT] NFS client {} mounted export {}",
            client_id, export_path
        ));

        // Get root file handle
        let root_fh = connection.root_nfs_fh3();
        let root_fh_hex = hex::encode(&root_fh.data.0);

        // Update client status to connected
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Create file handle cache
        let fh_cache = Arc::new(Mutex::new(HashMap::new()));
        fh_cache
            .lock()
            .await
            .insert("/".to_string(), root_fh.clone());

        // Spawn NFS operation handler
        let connection_arc = Arc::new(Mutex::new(connection));
        let app_state_clone = app_state.clone();
        let llm_client_clone = llm_client.clone();
        let status_tx_clone = status_tx.clone();
        let fh_cache_clone = fh_cache.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::handle_nfs_operations(
                connection_arc,
                fh_cache_clone,
                client_id,
                export_path,
                root_fh_hex,
                llm_client_clone,
                app_state_clone,
                status_tx_clone,
            )
            .await
            {
                error!("NFS client {} handler error: {}", client_id, e);
            }
        });

        // Return a dummy socket address (NFS doesn't use direct sockets)
        Ok(format!("{}:2049", server).parse()?)
    }

    /// Handle NFS operations with LLM integration
    #[cfg(feature = "nfs")]
    async fn handle_nfs_operations(
        connection: Arc<
            Mutex<nfs3_client::Nfs3Connection<nfs3_client::tokio::TokioIo<tokio::net::TcpStream>>>,
        >,
        fh_cache: Arc<Mutex<HashMap<String, nfs_fh3>>>,
        client_id: ClientId,
        export_path: String,
        root_fh_hex: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get instruction
        let instruction = app_state
            .get_instruction_for_client(client_id)
            .await
            .unwrap_or_default();

        let protocol = Arc::new(NfsClientProtocol::new());

        // Send connected event to LLM
        let connected_event = Event::new(
            &NFS_CLIENT_CONNECTED_EVENT,
            json!({
                "export_path": export_path,
                "root_fh": root_fh_hex
            }),
        );

        let memory = app_state
            .get_memory_for_client(client_id)
            .await
            .unwrap_or_default();

        let result = call_llm_for_client(
            &llm_client,
            &app_state,
            client_id.to_string(),
            &instruction,
            &memory,
            Some(&connected_event),
            protocol.as_ref(),
            &status_tx,
        )
        .await?;

        // Update memory
        if let Some(new_memory) = result.memory_updates {
            app_state.set_memory_for_client(client_id, new_memory).await;
        }

        // Execute actions from LLM
        for action in result.actions {
            if let Err(e) = Self::execute_action(
                &connection,
                &fh_cache,
                action,
                client_id,
                &llm_client,
                &app_state,
                &status_tx,
                protocol.as_ref(),
            )
            .await
            {
                error!("NFS operation error: {}", e);
                let _ = status_tx.send(format!("[ERROR] NFS operation failed: {}", e));
            }
        }

        // Keep client alive - monitor for disconnection
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;

            if app_state.get_client(client_id).await.is_none() {
                info!("NFS client {} stopped", client_id);
                break;
            }
        }

        Ok(())
    }

    /// Execute a single NFS action
    #[cfg(feature = "nfs")]
    async fn execute_action(
        connection: &Arc<
            Mutex<nfs3_client::Nfs3Connection<nfs3_client::tokio::TokioIo<tokio::net::TcpStream>>>,
        >,
        fh_cache: &Arc<Mutex<HashMap<String, nfs_fh3>>>,
        action: serde_json::Value,
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: &dyn ClientTrait,
    ) -> Result<()> {
        let action_result = protocol.execute_action(action.clone())?;

        match action_result {
            ClientActionResult::Disconnect => {
                info!("NFS client {} disconnecting", client_id);
                app_state
                    .update_client_status(client_id, ClientStatus::Disconnected)
                    .await;
                return Ok(());
            }
            ClientActionResult::WaitForMore => {
                trace!("NFS client {} waiting", client_id);
                return Ok(());
            }
            ClientActionResult::Custom { name, data } => {
                // Execute NFS operation
                let result_data = match name.as_str() {
                    "nfs_lookup" => Self::op_lookup(connection, fh_cache, data).await?,
                    "nfs_read_file" => Self::op_read_file(connection, fh_cache, data).await?,
                    "nfs_write_file" => Self::op_write_file(connection, fh_cache, data).await?,
                    "nfs_list_dir" => Self::op_list_dir(connection, fh_cache, data).await?,
                    "nfs_get_attr" => Self::op_get_attr(connection, fh_cache, data).await?,
                    "nfs_create_file" => Self::op_create_file(connection, fh_cache, data).await?,
                    "nfs_mkdir" => Self::op_mkdir(connection, fh_cache, data).await?,
                    "nfs_remove" => Self::op_remove(connection, fh_cache, data).await?,
                    "nfs_rmdir" => Self::op_rmdir(connection, fh_cache, data).await?,
                    _ => return Err(anyhow::anyhow!("Unknown NFS operation: {}", name)),
                };

                info!("NFS client {} completed operation: {}", client_id, name);

                // Send result event to LLM
                let result_event = Event::new(
                    &NFS_CLIENT_OPERATION_RESULT_EVENT,
                    json!({
                        "operation": name,
                        "result": result_data
                    }),
                );

                let instruction = app_state
                    .get_instruction_for_client(client_id)
                    .await
                    .unwrap_or_default();
                let memory = app_state
                    .get_memory_for_client(client_id)
                    .await
                    .unwrap_or_default();

                let llm_result = call_llm_for_client(
                    llm_client,
                    app_state,
                    client_id.to_string(),
                    &instruction,
                    &memory,
                    Some(&result_event),
                    protocol,
                    status_tx,
                )
                .await?;

                // Update memory
                if let Some(new_memory) = llm_result.memory_updates {
                    app_state.set_memory_for_client(client_id, new_memory).await;
                }

                // Execute follow-up actions recursively
                for next_action in llm_result.actions {
                    Box::pin(Self::execute_action(
                        connection,
                        fh_cache,
                        next_action,
                        client_id,
                        llm_client,
                        app_state,
                        status_tx,
                        protocol,
                    ))
                    .await?;
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Resolve path to file handle
    #[cfg(feature = "nfs")]
    async fn resolve_path(
        connection: &Arc<
            Mutex<nfs3_client::Nfs3Connection<nfs3_client::tokio::TokioIo<tokio::net::TcpStream>>>,
        >,
        fh_cache: &Arc<Mutex<HashMap<String, nfs_fh3>>>,
        path: &str,
    ) -> Result<nfs_fh3> {
        // Check cache
        {
            let cache = fh_cache.lock().await;
            if let Some(fh) = cache.get(path) {
                return Ok(fh.clone());
            }
        }

        // Root directory
        let components: Vec<&str> = path
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        if components.is_empty() {
            let conn = connection.lock().await;
            return Ok(conn.root_nfs_fh3());
        }

        // Walk path
        let mut current_fh = {
            let conn = connection.lock().await;
            conn.root_nfs_fh3()
        };
        let mut current_path = String::from("/");

        for component in components {
            let lookup_args = LOOKUP3args {
                what: diropargs3 {
                    dir: current_fh.clone(),
                    name: component.as_bytes().into(),
                },
            };

            let lookup_res = connection.lock().await.lookup(&lookup_args).await?;

            match lookup_res {
                LOOKUP3res::Ok(ok_res) => {
                    current_fh = ok_res.object;
                    current_path = if current_path == "/" {
                        format!("/{}", component)
                    } else {
                        format!("{}/{}", current_path, component)
                    };
                    fh_cache
                        .lock()
                        .await
                        .insert(current_path.clone(), current_fh.clone());
                }
                LOOKUP3res::Err((stat, _)) => {
                    return Err(anyhow::anyhow!(
                        "Lookup failed for {}: {:?}",
                        component,
                        stat
                    ));
                }
            }
        }

        Ok(current_fh)
    }

    /// NFS lookup operation
    #[cfg(feature = "nfs")]
    async fn op_lookup(
        connection: &Arc<
            Mutex<nfs3_client::Nfs3Connection<nfs3_client::tokio::TokioIo<tokio::net::TcpStream>>>,
        >,
        fh_cache: &Arc<Mutex<HashMap<String, nfs_fh3>>>,
        data: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let path = data["path"].as_str().context("Missing 'path' parameter")?;
        let fh = Self::resolve_path(connection, fh_cache, path).await?;

        Ok(json!({
            "path": path,
            "file_handle": hex::encode(&fh.data.0),
            "success": true
        }))
    }

    /// NFS read file operation
    #[cfg(feature = "nfs")]
    async fn op_read_file(
        connection: &Arc<
            Mutex<nfs3_client::Nfs3Connection<nfs3_client::tokio::TokioIo<tokio::net::TcpStream>>>,
        >,
        fh_cache: &Arc<Mutex<HashMap<String, nfs_fh3>>>,
        data: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let path = data["path"].as_str().context("Missing 'path' parameter")?;
        let offset = data["offset"].as_u64().unwrap_or(0);
        let count = data["count"].as_u64().unwrap_or(4096) as u32;

        let fh = Self::resolve_path(connection, fh_cache, path).await?;

        let read_args = READ3args {
            file: fh,
            offset,
            count,
        };

        let read_res = connection.lock().await.read(&read_args).await?;

        match read_res {
            READ3res::Ok(ok_res) => {
                let data_str = String::from_utf8_lossy(&ok_res.data.0).to_string();
                Ok(json!({
                    "path": path,
                    "data": data_str,
                    "bytes_read": ok_res.count,
                    "eof": ok_res.eof,
                    "success": true
                }))
            }
            READ3res::Err((stat, _)) => Err(anyhow::anyhow!("Read failed: {:?}", stat)),
        }
    }

    /// NFS write file operation
    #[cfg(feature = "nfs")]
    async fn op_write_file(
        connection: &Arc<
            Mutex<nfs3_client::Nfs3Connection<nfs3_client::tokio::TokioIo<tokio::net::TcpStream>>>,
        >,
        fh_cache: &Arc<Mutex<HashMap<String, nfs_fh3>>>,
        data: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let path = data["path"].as_str().context("Missing 'path' parameter")?;
        let write_data = data["data"].as_str().context("Missing 'data' parameter")?;
        let offset = data["offset"].as_u64().unwrap_or(0);

        let fh = Self::resolve_path(connection, fh_cache, path).await?;

        let write_args = WRITE3args {
            file: fh,
            offset,
            count: write_data.len() as u32,
            stable: stable_how::FILE_SYNC,
            data: write_data.as_bytes().into(),
        };

        let write_res = connection.lock().await.write(&write_args).await?;

        match write_res {
            WRITE3res::Ok(ok_res) => Ok(json!({
                "path": path,
                "bytes_written": ok_res.count,
                "success": true
            })),
            WRITE3res::Err((stat, _)) => Err(anyhow::anyhow!("Write failed: {:?}", stat)),
        }
    }

    /// NFS list directory operation
    #[cfg(feature = "nfs")]
    async fn op_list_dir(
        connection: &Arc<
            Mutex<nfs3_client::Nfs3Connection<nfs3_client::tokio::TokioIo<tokio::net::TcpStream>>>,
        >,
        fh_cache: &Arc<Mutex<HashMap<String, nfs_fh3>>>,
        data: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let path = data["path"].as_str().unwrap_or("/");
        let fh = Self::resolve_path(connection, fh_cache, path).await?;

        let readdir_args = READDIR3args {
            dir: fh,
            cookie: 0,
            cookieverf: cookieverf3([0; 8]),
            count: 4096,
        };

        let readdir_res = connection.lock().await.readdir(&readdir_args).await?;

        match readdir_res {
            READDIR3res::Ok(ok_res) => {
                let mut entries = Vec::new();

                // List<T> is a wrapper around Vec<T>, access via .0
                for entry in &ok_res.reply.entries.0 {
                    entries.push(json!({
                        "name": String::from_utf8_lossy(entry.name.as_ref()),
                        "fileid": entry.fileid
                    }));
                }

                Ok(json!({
                    "path": path,
                    "entries": entries,
                    "success": true
                }))
            }
            READDIR3res::Err((stat, _)) => Err(anyhow::anyhow!("Readdir failed: {:?}", stat)),
        }
    }

    /// NFS get attributes operation
    #[cfg(feature = "nfs")]
    async fn op_get_attr(
        connection: &Arc<
            Mutex<nfs3_client::Nfs3Connection<nfs3_client::tokio::TokioIo<tokio::net::TcpStream>>>,
        >,
        fh_cache: &Arc<Mutex<HashMap<String, nfs_fh3>>>,
        data: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let path = data["path"].as_str().context("Missing 'path' parameter")?;
        let fh = Self::resolve_path(connection, fh_cache, path).await?;

        let getattr_args = GETATTR3args { object: fh };

        let getattr_res = connection.lock().await.getattr(&getattr_args).await?;

        match getattr_res {
            GETATTR3res::Ok(ok_res) => Ok(json!({
                "path": path,
                "type": format!("{:?}", ok_res.obj_attributes.type_),
                "size": ok_res.obj_attributes.size,
                "mode": ok_res.obj_attributes.mode,
                "success": true
            })),
            GETATTR3res::Err(stat) => Err(anyhow::anyhow!("Getattr failed: {:?}", stat)),
        }
    }

    /// NFS create file operation
    #[cfg(feature = "nfs")]
    async fn op_create_file(
        connection: &Arc<
            Mutex<nfs3_client::Nfs3Connection<nfs3_client::tokio::TokioIo<tokio::net::TcpStream>>>,
        >,
        fh_cache: &Arc<Mutex<HashMap<String, nfs_fh3>>>,
        data: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let path = data["path"].as_str().context("Missing 'path' parameter")?;
        let mode = data["mode"].as_u64().unwrap_or(0o644) as u32;

        let (dir_path, filename) = path.rsplit_once('/').unwrap_or(("/", path));
        let dir_fh = Self::resolve_path(connection, fh_cache, dir_path).await?;

        let create_args = CREATE3args {
            where_: diropargs3 {
                dir: dir_fh,
                name: filename.as_bytes().into(),
            },
            how: createhow3::UNCHECKED(sattr3 {
                mode: Nfs3Option::Some(mode),
                uid: Nfs3Option::None,
                gid: Nfs3Option::None,
                size: Nfs3Option::None,
                atime: set_atime::DONT_CHANGE,
                mtime: set_mtime::DONT_CHANGE,
            }),
        };

        let create_res = connection.lock().await.create(&create_args).await?;

        match create_res {
            CREATE3res::Ok(ok_res) => {
                if let Nfs3Option::Some(obj) = ok_res.obj {
                    fh_cache.lock().await.insert(path.to_string(), obj);
                }
                Ok(json!({
                    "path": path,
                    "created": true,
                    "success": true
                }))
            }
            CREATE3res::Err((stat, _)) => Err(anyhow::anyhow!("Create failed: {:?}", stat)),
        }
    }

    /// NFS mkdir operation
    #[cfg(feature = "nfs")]
    async fn op_mkdir(
        connection: &Arc<
            Mutex<nfs3_client::Nfs3Connection<nfs3_client::tokio::TokioIo<tokio::net::TcpStream>>>,
        >,
        fh_cache: &Arc<Mutex<HashMap<String, nfs_fh3>>>,
        data: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let path = data["path"].as_str().context("Missing 'path' parameter")?;
        let mode = data["mode"].as_u64().unwrap_or(0o755) as u32;

        let (dir_path, dirname) = path.rsplit_once('/').unwrap_or(("/", path));
        let dir_fh = Self::resolve_path(connection, fh_cache, dir_path).await?;

        let mkdir_args = MKDIR3args {
            where_: diropargs3 {
                dir: dir_fh,
                name: dirname.as_bytes().into(),
            },
            attributes: sattr3 {
                mode: Nfs3Option::Some(mode),
                uid: Nfs3Option::None,
                gid: Nfs3Option::None,
                size: Nfs3Option::None,
                atime: set_atime::DONT_CHANGE,
                mtime: set_mtime::DONT_CHANGE,
            },
        };

        let mkdir_res = connection.lock().await.mkdir(&mkdir_args).await?;

        match mkdir_res {
            MKDIR3res::Ok(ok_res) => {
                if let Nfs3Option::Some(obj) = ok_res.obj {
                    fh_cache.lock().await.insert(path.to_string(), obj);
                }
                Ok(json!({
                    "path": path,
                    "created": true,
                    "success": true
                }))
            }
            MKDIR3res::Err((stat, _)) => Err(anyhow::anyhow!("Mkdir failed: {:?}", stat)),
        }
    }

    /// NFS remove file operation
    #[cfg(feature = "nfs")]
    async fn op_remove(
        connection: &Arc<
            Mutex<nfs3_client::Nfs3Connection<nfs3_client::tokio::TokioIo<tokio::net::TcpStream>>>,
        >,
        fh_cache: &Arc<Mutex<HashMap<String, nfs_fh3>>>,
        data: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let path = data["path"].as_str().context("Missing 'path' parameter")?;

        let (dir_path, filename) = path.rsplit_once('/').unwrap_or(("/", path));
        let dir_fh = Self::resolve_path(connection, fh_cache, dir_path).await?;

        let remove_args = REMOVE3args {
            object: diropargs3 {
                dir: dir_fh,
                name: filename.as_bytes().into(),
            },
        };

        let remove_res = connection.lock().await.remove(&remove_args).await?;

        match remove_res {
            REMOVE3res::Ok(_) => {
                fh_cache.lock().await.remove(path);
                Ok(json!({
                    "path": path,
                    "removed": true,
                    "success": true
                }))
            }
            REMOVE3res::Err((stat, _)) => Err(anyhow::anyhow!("Remove failed: {:?}", stat)),
        }
    }

    /// NFS rmdir operation
    #[cfg(feature = "nfs")]
    async fn op_rmdir(
        connection: &Arc<
            Mutex<nfs3_client::Nfs3Connection<nfs3_client::tokio::TokioIo<tokio::net::TcpStream>>>,
        >,
        fh_cache: &Arc<Mutex<HashMap<String, nfs_fh3>>>,
        data: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let path = data["path"].as_str().context("Missing 'path' parameter")?;

        let (dir_path, dirname) = path.rsplit_once('/').unwrap_or(("/", path));
        let dir_fh = Self::resolve_path(connection, fh_cache, dir_path).await?;

        let rmdir_args = RMDIR3args {
            object: diropargs3 {
                dir: dir_fh,
                name: dirname.as_bytes().into(),
            },
        };

        let rmdir_res = connection.lock().await.rmdir(&rmdir_args).await?;

        match rmdir_res {
            RMDIR3res::Ok(_) => {
                fh_cache.lock().await.remove(path);
                Ok(json!({
                    "path": path,
                    "removed": true,
                    "success": true
                }))
            }
            RMDIR3res::Err((stat, _)) => Err(anyhow::anyhow!("Rmdir failed: {:?}", stat)),
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
