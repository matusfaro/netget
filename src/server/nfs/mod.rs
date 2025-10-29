//! NFS server implementation using nfsserve
pub mod actions;

use anyhow::{Result, Context};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

#[cfg(feature = "nfs")]
use nfsserve::vfs::{NFSFileSystem, DirEntry, ReadDirResult};
#[cfg(feature = "nfs")]
use nfsserve::nfs::{fattr3, fileid3, filename3, nfspath3, nfsstat3, nfstime3, ftype3, sattr3};
#[cfg(feature = "nfs")]
use async_trait::async_trait;

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use actions::NFS_OPERATION_EVENT;
use crate::server::NfsProtocol;
use crate::protocol::Event;
use crate::state::app_state::AppState;

/// NFS server that provides LLM-controlled file system
pub struct NfsServer;

impl NfsServer {
    /// Spawn NFS server with integrated LLM actions
    #[cfg(feature = "nfs")]
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        use nfsserve::tcp::{NFSTcp, NFSTcpListener};

        info!("NFS server (LLM-controlled) starting on {}", listen_addr);
        let _ = status_tx.send(format!("[INFO] NFS server starting on {}", listen_addr));

        let protocol = Arc::new(NfsProtocol::new());

        // Create LLM-controlled filesystem
        let filesystem = LlmNfsFileSystem::new(
            llm_client,
            app_state.clone(),
            server_id,
            protocol,
            status_tx.clone(),
        );

        // Bind NFS TCP listener with LLM filesystem
        let nfs_listener = NFSTcpListener::bind(&listen_addr.to_string(), filesystem)
            .await
            .context("Failed to bind NFS TCP listener")?;

        let actual_port = nfs_listener.get_listen_port();
        let actual_addr = SocketAddr::new(listen_addr.ip(), actual_port);

        info!("NFS server listening on {}", actual_addr);
        let _ = status_tx.send(format!("→ NFS server listening on {}", actual_addr));

        // Spawn server handler
        tokio::spawn(async move {
            info!("NFS server handler started");

            // Handle connections forever (nfsserve manages connections internally)
            if let Err(e) = nfs_listener.handle_forever().await {
                error!("NFS server error: {}", e);
                let _ = status_tx.send(format!("✗ NFS server error: {}", e));
            }
        });

        Ok(actual_addr)
    }

    /// Spawn NFS server without the nfs feature (fallback)
    #[cfg(not(feature = "nfs"))]
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        _server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let _ = status_tx.send("[ERROR] NFS feature not enabled at compile time".to_string());
        Err(anyhow::anyhow!("NFS feature not enabled"))
    }
}

/// LLM-controlled NFS filesystem implementation
#[cfg(feature = "nfs")]
pub struct LlmNfsFileSystem {
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    server_id: crate::state::ServerId,
    protocol: Arc<NfsProtocol>,
    status_tx: mpsc::UnboundedSender<String>,
}

#[cfg(feature = "nfs")]
impl LlmNfsFileSystem {
    /// Create a new LLM-controlled NFS filesystem
    pub fn new(
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        server_id: crate::state::ServerId,
        protocol: Arc<NfsProtocol>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Self {
        Self {
            llm_client,
            app_state,
            server_id,
            protocol,
            status_tx,
        }
    }

    /// Consult the LLM for NFS operations
    async fn consult_llm(&self, operation: &str, params: serde_json::Value) -> Result<Vec<serde_json::Value>> {
        debug!("Consulting LLM for NFS {} operation", operation);
        let _ = self.status_tx.send(format!("[DEBUG] NFS {}: {:?}", operation, params));

        // Create NFS operation event
        let event = Event::new(&NFS_OPERATION_EVENT, serde_json::json!({
            "operation": operation,
            "params": params
        }));

        trace!("Calling LLM for NFS {} operation", operation);
        let _ = self.status_tx.send(format!("[TRACE] Calling LLM for NFS {}", operation));

        // Call LLM with Event-based approach
        let execution_result = call_llm(
            &self.llm_client,
            &self.app_state,
            self.server_id,
            None,  // NFS doesn't use connection-specific context
            &event,
            self.protocol.as_ref(),
        ).await?;

        // Display messages from LLM
        for message in &execution_result.messages {
            info!("{}", message);
            let _ = self.status_tx.send(format!("[INFO] {}", message));
        }

        debug!("LLM returned {} actions for NFS {}", execution_result.raw_actions.len(), operation);

        // Return raw actions for manual processing
        Ok(execution_result.raw_actions)
    }

    /// Parse file type from LLM response
    fn parse_ftype(&self, file_type: &str) -> ftype3 {
        match file_type {
            "regular" | "file" => ftype3::NF3REG,
            "directory" | "dir" => ftype3::NF3DIR,
            "symlink" | "link" => ftype3::NF3LNK,
            "block" => ftype3::NF3BLK,
            "char" => ftype3::NF3CHR,
            "socket" => ftype3::NF3SOCK,
            "fifo" => ftype3::NF3FIFO,
            _ => ftype3::NF3REG, // Default to regular file
        }
    }

    /// Parse NFS timestamp
    fn parse_nfstime(&self, timestamp: Option<u64>) -> nfstime3 {
        let ts = timestamp.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        });
        nfstime3 {
            seconds: ts as u32,
            nseconds: 0,
        }
    }

    /// Build fattr3 from LLM response
    fn build_fattr3(&self, response: &serde_json::Value) -> Result<fattr3> {
        let file_type = response.get("file_type")
            .and_then(|v| v.as_str())
            .unwrap_or("regular");

        let mode = response.get("mode")
            .and_then(|v| v.as_u64())
            .unwrap_or(0o644) as u32;

        let size = response.get("size")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let uid = response.get("uid")
            .and_then(|v| v.as_u64())
            .unwrap_or(1000) as u32;

        let gid = response.get("gid")
            .and_then(|v| v.as_u64())
            .unwrap_or(1000) as u32;

        let atime = self.parse_nfstime(response.get("atime").and_then(|v| v.as_u64()));
        let mtime = self.parse_nfstime(response.get("mtime").and_then(|v| v.as_u64()));
        let ctime = self.parse_nfstime(response.get("ctime").and_then(|v| v.as_u64()));

        Ok(fattr3 {
            ftype: self.parse_ftype(file_type),
            mode,
            nlink: 1,
            uid,
            gid,
            size,
            used: size,
            rdev: nfsserve::nfs::specdata3 { specdata1: 0, specdata2: 0 },
            fsid: 0,
            fileid: response.get("fileid")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            atime,
            mtime,
            ctime,
        })
    }
}

#[cfg(feature = "nfs")]
#[async_trait]
impl NFSFileSystem for LlmNfsFileSystem {
    fn root_dir(&self) -> fileid3 {
        // Root directory is always fileid 1
        1
    }

    fn capabilities(&self) -> nfsserve::vfs::VFSCapabilities {
        // Enable all capabilities since LLM controls everything
        nfsserve::vfs::VFSCapabilities::ReadWrite
    }

    async fn lookup(&self, dirid: fileid3, filename: &filename3) -> Result<fileid3, nfsstat3> {
        // Convert filename to string
        let filename_str = String::from_utf8_lossy(filename).to_string();

        let params = serde_json::json!({
            "dirid": dirid,
            "filename": filename_str,
        });

        // Call async LLM consultation directly
        let result = self.consult_llm("lookup", params).await;

        match result {
            Ok(actions) => {
                // Find nfs_lookup_response action
                for action in actions {
                    if action.get("type").and_then(|v| v.as_str()) == Some("nfs_lookup_response") {
                        if let Some(error) = action.get("error").and_then(|v| v.as_str()) {
                            debug!("NFS lookup failed: {}", error);
                            return Err(nfsstat3::NFS3ERR_NOENT);
                        }

                        if let Some(fileid) = action.get("fileid").and_then(|v| v.as_u64()) {
                            debug!("NFS lookup found fileid: {}", fileid);
                            return Ok(fileid);
                        }
                    }
                }
                error!("No valid nfs_lookup_response action in LLM response");
                Err(nfsstat3::NFS3ERR_NOENT)
            }
            Err(e) => {
                error!("LLM consultation failed for lookup: {}", e);
                Err(nfsstat3::NFS3ERR_IO)
            }
        }
    }

    async fn getattr(&self, id: fileid3) -> Result<fattr3, nfsstat3> {
        let params = serde_json::json!({
            "fileid": id,
        });

        let result = self.consult_llm("getattr", params).await;

        match result {
            Ok(actions) => {
                for action in actions {
                    if action.get("type").and_then(|v| v.as_str()) == Some("nfs_getattr_response") {
                        if let Some(error) = action.get("error").and_then(|v| v.as_str()) {
                            debug!("NFS getattr failed: {}", error);
                            return Err(nfsstat3::NFS3ERR_NOENT);
                        }

                        match self.build_fattr3(&action) {
                            Ok(mut attrs) => {
                                attrs.fileid = id; // Ensure fileid matches request
                                debug!("NFS getattr succeeded for fileid {}", id);
                                return Ok(attrs);
                            }
                            Err(e) => {
                                error!("Failed to build fattr3: {}", e);
                                return Err(nfsstat3::NFS3ERR_IO);
                            }
                        }
                    }
                }
                error!("No valid nfs_getattr_response action in LLM response");
                Err(nfsstat3::NFS3ERR_NOENT)
            }
            Err(e) => {
                error!("LLM consultation failed for getattr: {}", e);
                Err(nfsstat3::NFS3ERR_IO)
            }
        }
    }

    async fn setattr(&self, id: fileid3, setattr: sattr3) -> Result<fattr3, nfsstat3> {
        use nfsserve::nfs::{set_mode3, set_uid3, set_gid3, set_size3};

        // Convert NFS optional enums to Option for JSON serialization
        let mode_val = match setattr.mode {
            set_mode3::mode(v) => Some(v as u64),
            set_mode3::Void => None,
        };
        let uid_val = match setattr.uid {
            set_uid3::uid(v) => Some(v as u64),
            set_uid3::Void => None,
        };
        let gid_val = match setattr.gid {
            set_gid3::gid(v) => Some(v as u64),
            set_gid3::Void => None,
        };
        let size_val = match setattr.size {
            set_size3::size(v) => Some(v),
            set_size3::Void => None,
        };

        let params = serde_json::json!({
            "fileid": id,
            "mode": mode_val,
            "uid": uid_val,
            "gid": gid_val,
            "size": size_val,
        });

        let result = self.consult_llm("setattr", params).await;

        match result {
            Ok(actions) => {
                for action in actions {
                    if action.get("type").and_then(|v| v.as_str()) == Some("nfs_setattr_response") {
                        if let Some(error) = action.get("error").and_then(|v| v.as_str()) {
                            debug!("NFS setattr failed: {}", error);
                            return Err(nfsstat3::NFS3ERR_ACCES);
                        }

                        match self.build_fattr3(&action) {
                            Ok(mut attrs) => {
                                attrs.fileid = id;
                                debug!("NFS setattr succeeded for fileid {}", id);
                                return Ok(attrs);
                            }
                            Err(e) => {
                                error!("Failed to build fattr3: {}", e);
                                return Err(nfsstat3::NFS3ERR_IO);
                            }
                        }
                    }
                }
                error!("No valid nfs_setattr_response action in LLM response");
                Err(nfsstat3::NFS3ERR_ACCES)
            }
            Err(e) => {
                error!("LLM consultation failed for setattr: {}", e);
                Err(nfsstat3::NFS3ERR_IO)
            }
        }
    }

    async fn read(&self, id: fileid3, offset: u64, count: u32) -> Result<(Vec<u8>, bool), nfsstat3> {
        let params = serde_json::json!({
            "fileid": id,
            "offset": offset,
            "count": count,
        });

        let result = self.consult_llm("read", params).await;

        match result {
            Ok(actions) => {
                for action in actions {
                    if action.get("type").and_then(|v| v.as_str()) == Some("nfs_read_response") {
                        if let Some(error) = action.get("error").and_then(|v| v.as_str()) {
                            debug!("NFS read failed: {}", error);
                            return Err(nfsstat3::NFS3ERR_ACCES);
                        }

                        let data = action.get("data")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .as_bytes()
                            .to_vec();

                        let eof = action.get("eof")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(true);

                        debug!("NFS read {} bytes from fileid {}, eof={}", data.len(), id, eof);
                        return Ok((data, eof));
                    }
                }
                error!("No valid nfs_read_response action in LLM response");
                Err(nfsstat3::NFS3ERR_IO)
            }
            Err(e) => {
                error!("LLM consultation failed for read: {}", e);
                Err(nfsstat3::NFS3ERR_IO)
            }
        }
    }

    async fn write(&self, id: fileid3, offset: u64, data: &[u8]) -> Result<fattr3, nfsstat3> {
        let data_str = String::from_utf8_lossy(data).to_string();
        let params = serde_json::json!({
            "fileid": id,
            "offset": offset,
            "data": data_str,
            "size": data.len(),
        });

        let result = self.consult_llm("write", params).await;

        match result {
            Ok(actions) => {
                for action in actions {
                    if action.get("type").and_then(|v| v.as_str()) == Some("nfs_write_response") {
                        if let Some(error) = action.get("error").and_then(|v| v.as_str()) {
                            debug!("NFS write failed: {}", error);
                            return Err(nfsstat3::NFS3ERR_ACCES);
                        }

                        match self.build_fattr3(&action) {
                            Ok(mut attrs) => {
                                attrs.fileid = id;
                                debug!("NFS write {} bytes to fileid {}", data.len(), id);
                                return Ok(attrs);
                            }
                            Err(e) => {
                                error!("Failed to build fattr3: {}", e);
                                return Err(nfsstat3::NFS3ERR_IO);
                            }
                        }
                    }
                }
                error!("No valid nfs_write_response action in LLM response");
                Err(nfsstat3::NFS3ERR_ACCES)
            }
            Err(e) => {
                error!("LLM consultation failed for write: {}", e);
                Err(nfsstat3::NFS3ERR_IO)
            }
        }
    }

    async fn create(&self, dirid: fileid3, filename: &filename3, attr: sattr3) -> Result<(fileid3, fattr3), nfsstat3> {
        use nfsserve::nfs::{set_mode3, set_uid3, set_gid3};

        let filename_str = String::from_utf8_lossy(filename).to_string();
        let mode_val = match attr.mode {
            set_mode3::mode(v) => Some(v as u64),
            set_mode3::Void => None,
        };
        let uid_val = match attr.uid {
            set_uid3::uid(v) => Some(v as u64),
            set_uid3::Void => None,
        };
        let gid_val = match attr.gid {
            set_gid3::gid(v) => Some(v as u64),
            set_gid3::Void => None,
        };

        let params = serde_json::json!({
            "dirid": dirid,
            "filename": filename_str,
            "mode": mode_val,
            "uid": uid_val,
            "gid": gid_val,
        });

        let result = self.consult_llm("create", params).await;

        match result {
            Ok(actions) => {
                for action in actions {
                    if action.get("type").and_then(|v| v.as_str()) == Some("nfs_create_response") {
                        if let Some(error) = action.get("error").and_then(|v| v.as_str()) {
                            debug!("NFS create failed: {}", error);
                            return Err(nfsstat3::NFS3ERR_ACCES);
                        }

                        let fileid = action.get("fileid")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);

                        match self.build_fattr3(&action) {
                            Ok(mut attrs) => {
                                attrs.fileid = fileid;
                                debug!("NFS create succeeded: {} with fileid {}", filename_str, fileid);
                                return Ok((fileid, attrs));
                            }
                            Err(e) => {
                                error!("Failed to build fattr3: {}", e);
                                return Err(nfsstat3::NFS3ERR_IO);
                            }
                        }
                    }
                }
                error!("No valid nfs_create_response action in LLM response");
                Err(nfsstat3::NFS3ERR_ACCES)
            }
            Err(e) => {
                error!("LLM consultation failed for create: {}", e);
                Err(nfsstat3::NFS3ERR_IO)
            }
        }
    }

    async fn create_exclusive(&self, dirid: fileid3, filename: &filename3) -> Result<fileid3, nfsstat3> {
        // Create exclusive is like create but fails if file exists
        let filename_str = String::from_utf8_lossy(filename).to_string();
        let params = serde_json::json!({
            "dirid": dirid,
            "filename": filename_str,
            "exclusive": true,
        });

        let result = self.consult_llm("create", params).await;

        match result {
            Ok(actions) => {
                for action in actions {
                    if action.get("type").and_then(|v| v.as_str()) == Some("nfs_create_response") {
                        if let Some(error) = action.get("error").and_then(|v| v.as_str()) {
                            debug!("NFS create_exclusive failed: {}", error);
                            return Err(nfsstat3::NFS3ERR_EXIST);
                        }

                        if let Some(fileid) = action.get("fileid").and_then(|v| v.as_u64()) {
                            debug!("NFS create_exclusive succeeded: {} with fileid {}", filename_str, fileid);
                            return Ok(fileid);
                        }
                    }
                }
                error!("No valid nfs_create_response action in LLM response");
                Err(nfsstat3::NFS3ERR_EXIST)
            }
            Err(e) => {
                error!("LLM consultation failed for create_exclusive: {}", e);
                Err(nfsstat3::NFS3ERR_IO)
            }
        }
    }

    async fn mkdir(&self, dirid: fileid3, dirname: &filename3) -> Result<(fileid3, fattr3), nfsstat3> {
        let dirname_str = String::from_utf8_lossy(dirname).to_string();
        let params = serde_json::json!({
            "dirid": dirid,
            "dirname": dirname_str,
        });

        let result = self.consult_llm("mkdir", params).await;

        match result {
            Ok(actions) => {
                for action in actions {
                    if action.get("type").and_then(|v| v.as_str()) == Some("nfs_mkdir_response") {
                        if let Some(error) = action.get("error").and_then(|v| v.as_str()) {
                            debug!("NFS mkdir failed: {}", error);
                            return Err(nfsstat3::NFS3ERR_ACCES);
                        }

                        let fileid = action.get("dirid")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);

                        match self.build_fattr3(&action) {
                            Ok(mut attrs) => {
                                attrs.fileid = fileid;
                                attrs.ftype = ftype3::NF3DIR; // Ensure it's a directory
                                debug!("NFS mkdir succeeded: {} with dirid {}", dirname_str, fileid);
                                return Ok((fileid, attrs));
                            }
                            Err(e) => {
                                error!("Failed to build fattr3: {}", e);
                                return Err(nfsstat3::NFS3ERR_IO);
                            }
                        }
                    }
                }
                error!("No valid nfs_mkdir_response action in LLM response");
                Err(nfsstat3::NFS3ERR_ACCES)
            }
            Err(e) => {
                error!("LLM consultation failed for mkdir: {}", e);
                Err(nfsstat3::NFS3ERR_IO)
            }
        }
    }

    async fn remove(&self, dirid: fileid3, filename: &filename3) -> Result<(), nfsstat3> {
        let filename_str = String::from_utf8_lossy(filename).to_string();
        let params = serde_json::json!({
            "dirid": dirid,
            "filename": filename_str,
        });

        let result = self.consult_llm("remove", params).await;

        match result {
            Ok(actions) => {
                for action in actions {
                    if action.get("type").and_then(|v| v.as_str()) == Some("nfs_remove_response") {
                        if let Some(error) = action.get("error").and_then(|v| v.as_str()) {
                            debug!("NFS remove failed: {}", error);
                            return Err(nfsstat3::NFS3ERR_NOENT);
                        }

                        debug!("NFS remove succeeded: {}", filename_str);
                        return Ok(());
                    }
                }
                error!("No valid nfs_remove_response action in LLM response");
                Err(nfsstat3::NFS3ERR_NOENT)
            }
            Err(e) => {
                error!("LLM consultation failed for remove: {}", e);
                Err(nfsstat3::NFS3ERR_IO)
            }
        }
    }

    async fn rename(&self, from_dirid: fileid3, from_filename: &filename3, to_dirid: fileid3, to_filename: &filename3) -> Result<(), nfsstat3> {
        let from_name = String::from_utf8_lossy(from_filename).to_string();
        let to_name = String::from_utf8_lossy(to_filename).to_string();
        let params = serde_json::json!({
            "from_dirid": from_dirid,
            "from_filename": from_name,
            "to_dirid": to_dirid,
            "to_filename": to_name,
        });

        let result = self.consult_llm("rename", params).await;

        match result {
            Ok(actions) => {
                for action in actions {
                    if action.get("type").and_then(|v| v.as_str()) == Some("nfs_rename_response") {
                        if let Some(error) = action.get("error").and_then(|v| v.as_str()) {
                            debug!("NFS rename failed: {}", error);
                            return Err(nfsstat3::NFS3ERR_ACCES);
                        }

                        debug!("NFS rename succeeded: {} -> {}", from_name, to_name);
                        return Ok(());
                    }
                }
                error!("No valid nfs_rename_response action in LLM response");
                Err(nfsstat3::NFS3ERR_ACCES)
            }
            Err(e) => {
                error!("LLM consultation failed for rename: {}", e);
                Err(nfsstat3::NFS3ERR_IO)
            }
        }
    }

    async fn readdir(&self, dirid: fileid3, start_after: fileid3, max_entries: usize) -> Result<ReadDirResult, nfsstat3> {
        let params = serde_json::json!({
            "dirid": dirid,
            "start_after": start_after,
            "max_entries": max_entries,
        });

        let result = self.consult_llm("readdir", params).await;

        match result {
            Ok(actions) => {
                for action in actions {
                    if action.get("type").and_then(|v| v.as_str()) == Some("nfs_readdir_response") {
                        if let Some(error) = action.get("error").and_then(|v| v.as_str()) {
                            debug!("NFS readdir failed: {}", error);
                            return Err(nfsstat3::NFS3ERR_NOTDIR);
                        }

                        let entries_json = action.get("entries")
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_default();

                        let mut entries = Vec::new();
                        for entry in entries_json {
                            let fileid = entry.get("fileid")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);

                            let name = entry.get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .as_bytes()
                                .to_vec();

                            // Build attributes, use defaults if LLM didn't provide them
                            let mut attr = match entry.get("attr").and_then(|v| self.build_fattr3(v).ok()) {
                                Some(a) => a,
                                None => {
                                    // Provide minimal default attributes
                                    fattr3 {
                                        ftype: ftype3::NF3REG,
                                        mode: 0o644,
                                        nlink: 1,
                                        uid: 1000,
                                        gid: 1000,
                                        size: 0,
                                        used: 0,
                                        rdev: nfsserve::nfs::specdata3 { specdata1: 0, specdata2: 0 },
                                        fsid: 0,
                                        fileid,
                                        atime: self.parse_nfstime(None),
                                        mtime: self.parse_nfstime(None),
                                        ctime: self.parse_nfstime(None),
                                    }
                                }
                            };
                            attr.fileid = fileid; // Ensure fileid matches

                            entries.push(DirEntry {
                                fileid,
                                name: nfsserve::nfs::nfsstring(name),
                                attr,
                            });
                        }

                        let end = action.get("end")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(true);

                        debug!("NFS readdir returned {} entries, end={}", entries.len(), end);
                        return Ok(ReadDirResult { entries, end });
                    }
                }
                error!("No valid nfs_readdir_response action in LLM response");
                Err(nfsstat3::NFS3ERR_NOTDIR)
            }
            Err(e) => {
                error!("LLM consultation failed for readdir: {}", e);
                Err(nfsstat3::NFS3ERR_IO)
            }
        }
    }

    async fn symlink(&self, dirid: fileid3, linkname: &filename3, symlink: &nfspath3, attr: &sattr3) -> Result<(fileid3, fattr3), nfsstat3> {
        use nfsserve::nfs::set_mode3;

        let linkname_str = String::from_utf8_lossy(linkname).to_string();
        let target_str = String::from_utf8_lossy(symlink).to_string();
        let mode_val = match attr.mode {
            set_mode3::mode(v) => Some(v as u64),
            set_mode3::Void => None,
        };

        let params = serde_json::json!({
            "dirid": dirid,
            "linkname": linkname_str,
            "target": target_str,
            "mode": mode_val,
        });

        let result = self.consult_llm("symlink", params).await;

        match result {
            Ok(actions) => {
                for action in actions {
                    if action.get("type").and_then(|v| v.as_str()) == Some("nfs_create_response") {
                        if let Some(error) = action.get("error").and_then(|v| v.as_str()) {
                            debug!("NFS symlink failed: {}", error);
                            return Err(nfsstat3::NFS3ERR_ACCES);
                        }

                        let fileid = action.get("fileid")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);

                        match self.build_fattr3(&action) {
                            Ok(mut attrs) => {
                                attrs.fileid = fileid;
                                attrs.ftype = ftype3::NF3LNK; // Ensure it's a symlink
                                debug!("NFS symlink succeeded: {} -> {}", linkname_str, target_str);
                                return Ok((fileid, attrs));
                            }
                            Err(e) => {
                                error!("Failed to build fattr3: {}", e);
                                return Err(nfsstat3::NFS3ERR_IO);
                            }
                        }
                    }
                }
                error!("No valid nfs_create_response action in LLM response");
                Err(nfsstat3::NFS3ERR_ACCES)
            }
            Err(e) => {
                error!("LLM consultation failed for symlink: {}", e);
                Err(nfsstat3::NFS3ERR_IO)
            }
        }
    }

    async fn readlink(&self, id: fileid3) -> Result<nfspath3, nfsstat3> {
        let params = serde_json::json!({
            "fileid": id,
        });

        let result = self.consult_llm("readlink", params).await;

        match result {
            Ok(actions) => {
                for action in actions {
                    // Reuse nfs_read_response for readlink (returns target path in data field)
                    if action.get("type").and_then(|v| v.as_str()) == Some("nfs_read_response") {
                        if let Some(error) = action.get("error").and_then(|v| v.as_str()) {
                            debug!("NFS readlink failed: {}", error);
                            return Err(nfsstat3::NFS3ERR_INVAL);
                        }

                        let target_bytes = action.get("data")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .as_bytes()
                            .to_vec();

                        debug!("NFS readlink for fileid {}: {}", id, String::from_utf8_lossy(&target_bytes));
                        return Ok(nfsserve::nfs::nfsstring(target_bytes));
                    }
                }
                error!("No valid nfs_read_response action in LLM response");
                Err(nfsstat3::NFS3ERR_IO)
            }
            Err(e) => {
                error!("LLM consultation failed for readlink: {}", e);
                Err(nfsstat3::NFS3ERR_IO)
            }
        }
    }
}
