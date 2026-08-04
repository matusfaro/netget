//! SFTP handler with LLM integration
//!
//! This module implements an SFTP server handler that delegates all
//! filesystem operations to the LLM, creating a virtual filesystem
//! entirely controlled by the AI.

use super::actions::SFTP_OPERATION_EVENT;
use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::server::SshProtocol;
use crate::state::app_state::AppState;
use anyhow::{anyhow, Result};
use russh_sftp::protocol::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};

/// LLM-controlled SFTP handler
///
/// This handler implements the russh_sftp::server::Handler trait
/// but delegates all filesystem decisions to the LLM instead of
/// using a real filesystem.
pub struct LlmSftpHandler {
    connection_id: ConnectionId,
    server_id: crate::state::ServerId,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    protocol: Arc<SshProtocol>,
    status_tx: mpsc::UnboundedSender<String>,
    /// Track handles that the LLM creates
    handles: Arc<Mutex<HashMap<String, HandleInfo>>>,
    /// SFTP protocol version
    version: Option<u32>,
}

/// The action names that count as an answer to an `sftp_operation` event.
fn is_sftp_reply_action(name: &str) -> bool {
    matches!(
        name,
        "sftp_handle"
            | "sftp_directory_listing"
            | "sftp_file_content"
            | "sftp_file_attributes"
            | "sftp_error"
    )
}

/// If the handler answered with `sftp_error`, map its `code` to an SFTP status code.
///
/// Returns `None` when the response is not an error, so callers can carry on.
fn sftp_error_status(response: &serde_json::Value) -> Option<StatusCode> {
    if response.get("type").and_then(|v| v.as_str()) != Some("sftp_error") {
        return None;
    }
    let code = response
        .get("code")
        .and_then(|v| v.as_str())
        .unwrap_or("no_such_file");
    Some(match code {
        "no_such_file" => StatusCode::NoSuchFile,
        "permission_denied" => StatusCode::PermissionDenied,
        "op_unsupported" => StatusCode::OpUnsupported,
        "eof" => StatusCode::Eof,
        _ => StatusCode::Failure,
    })
}

/// Information about an open handle
#[derive(Debug, Clone)]
struct HandleInfo {
    path: String,
    #[allow(dead_code)] // Tracked for correctness but not currently used in logic
    is_directory: bool,
    /// For directories, track if we've completed reading
    dir_read_done: bool,
}

impl LlmSftpHandler {
    /// Create a new LLM-controlled SFTP handler
    pub fn new(
        connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        protocol: Arc<SshProtocol>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Self {
        Self {
            connection_id,
            server_id,
            llm_client,
            app_state,
            protocol,
            status_tx,
            handles: Arc::new(Mutex::new(HashMap::new())),
            version: None,
        }
    }

    /// Ask the handler/LLM to answer an SFTP operation.
    ///
    /// `params` carries structured fields (`path`, `handle`, `offset`, `length`) that are
    /// merged into the event alongside `operation`. They used to be flattened into a single
    /// `params` string like `"path='/x', id=3"`, which a script handler had to re-parse and
    /// which violated the structured-data rule for event payloads.
    async fn llm_sftp_operation(
        &self,
        operation: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        // DEBUG: LLM request summary
        debug!(
            "SFTP LLM request: operation={}, params={}",
            operation, params
        );
        let _ = self.status_tx.send(format!(
            "[DEBUG] SFTP LLM request: operation={}, params={}",
            operation, params
        ));

        // Create SFTP operation event
        let mut event_data = serde_json::json!({ "operation": operation });
        if let Some(fields) = params.as_object() {
            for (key, value) in fields {
                event_data[key] = value.clone();
            }
        }
        let event = Event::new(&SFTP_OPERATION_EVENT, event_data);

        // TRACE: Event details
        trace!("SFTP calling LLM for operation: {}", operation);
        let _ = self.status_tx.send(format!(
            "[TRACE] SFTP calling LLM for operation: {}",
            operation
        ));

        // Call LLM with Event-based approach
        match call_llm(
            &self.llm_client,
            &self.app_state,
            self.server_id,
            Some(self.connection_id),
            &event,
            self.protocol.as_ref(),
        )
        .await
        {
            Ok(execution_result) => {
                // Display messages from LLM
                for message in &execution_result.messages {
                    info!("{}", message);
                    let _ = self.status_tx.send(format!("[INFO] {}", message));
                }

                // DEBUG: LLM response summary
                debug!(
                    "SFTP LLM returned {} actions for operation: {}",
                    execution_result.raw_actions.len(),
                    operation
                );
                let _ = self.status_tx.send(format!(
                    "[DEBUG] SFTP LLM returned {} actions for operation: {}",
                    execution_result.raw_actions.len(),
                    operation
                ));

                // TRACE: Full response
                if !execution_result.raw_actions.is_empty() {
                    let pretty = serde_json::to_string_pretty(&execution_result.raw_actions[0])
                        .unwrap_or_else(|_| format!("{:?}", execution_result.raw_actions[0]));
                    trace!("SFTP LLM response ({}) JSON:\n{}", operation, pretty);
                    let _ = self.status_tx.send(format!(
                        "[TRACE] SFTP LLM response ({}) JSON:\r\n{}",
                        operation,
                        pretty.replace('\n', "\r\n")
                    ));
                }

                // SFTP expects exactly one reply per request. Prefer the first action that is
                // actually an SFTP reply, so a leading show_message/set_memory does not get
                // mistaken for the answer. Fall back to the first action for handlers that
                // return a bare object with no "type".
                let reply = execution_result
                    .raw_actions
                    .iter()
                    .find(|a| {
                        a.get("type")
                            .and_then(|v| v.as_str())
                            .is_some_and(is_sftp_reply_action)
                    })
                    .or_else(|| execution_result.raw_actions.first())
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));

                Ok(reply)
            }
            Err(e) => {
                error!("LLM error for SFTP {}: {}", operation, e);
                let _ = self
                    .status_tx
                    .send(format!("[ERROR] LLM error for SFTP {}: {}", operation, e));
                Err(anyhow!("LLM error: {}", e))
            }
        }
    }
}

impl russh_sftp::server::Handler for LlmSftpHandler {
    type Error = StatusCode;

    async fn init(
        &mut self,
        version: u32,
        extensions: HashMap<String, String>,
    ) -> Result<Version, Self::Error> {
        info!(
            "SFTP init: version={}, extensions={:?}",
            version, extensions
        );
        self.version = Some(version);

        // Send status update
        let _ = self
            .status_tx
            .send(format!("SFTP session initialized (v{})", version));

        // Return version 3 (most widely supported)
        Ok(Version::new())
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        // DEBUG: SFTP request summary
        debug!("SFTP request: SSH_FXP_OPENDIR id={}, path={}", id, path);
        let _ = self.status_tx.send(format!(
            "[DEBUG] SFTP request: SSH_FXP_OPENDIR id={}, path={}",
            id, path
        ));

        // TRACE: Full SFTP request
        trace!("SFTP SSH_FXP_OPENDIR request: id={}, path='{}'", id, path);
        let _ = self.status_tx.send(format!(
            "[TRACE] SFTP SSH_FXP_OPENDIR request: id={}, path='{}'",
            id, path
        ));

        let params = serde_json::json!({ "path": path });
        match self.llm_sftp_operation("opendir", params).await {
            Ok(response) => {
                if let Some(status) = sftp_error_status(&response) {
                    debug!("SFTP opendir refused by handler for {}: {:?}", path, status);
                    let _ = self
                        .status_tx
                        .send(format!("[DEBUG] SFTP opendir refused for {path}"));
                    return Err(status);
                }

                // LLM should return a handle for this directory
                let handle_str = response
                    .get("handle")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&path)
                    .to_string();

                // Store handle info
                self.handles.lock().await.insert(
                    handle_str.clone(),
                    HandleInfo {
                        path: path.clone(),
                        is_directory: true,
                        dir_read_done: false,
                    },
                );

                let _ = self
                    .status_tx
                    .send(format!("→ SFTP opened directory: {}", path));

                // DEBUG: SFTP response summary
                debug!(
                    "SFTP response: SSH_FXP_HANDLE id={}, handle_len={} bytes",
                    id,
                    handle_str.len()
                );
                let _ = self.status_tx.send(format!(
                    "[DEBUG] SFTP response: SSH_FXP_HANDLE id={}, handle_len={} bytes",
                    id,
                    handle_str.len()
                ));

                // TRACE: Full SFTP response
                trace!(
                    "SFTP SSH_FXP_HANDLE response: id={}, handle='{}'",
                    id,
                    handle_str
                );
                let _ = self.status_tx.send(format!(
                    "[TRACE] SFTP SSH_FXP_HANDLE response: id={}, handle='{}'",
                    id, handle_str
                ));

                Ok(Handle {
                    id,
                    handle: handle_str,
                })
            }
            Err(_) => {
                error!("SFTP opendir failed for path: {}", path);
                let _ = self
                    .status_tx
                    .send(format!("[ERROR] SFTP opendir failed for path: {}", path));

                // DEBUG: SFTP error response
                debug!(
                    "SFTP response: SSH_FXP_STATUS id={}, status=NO_SUCH_FILE",
                    id
                );
                let _ = self.status_tx.send(format!(
                    "[DEBUG] SFTP response: SSH_FXP_STATUS id={}, status=NO_SUCH_FILE",
                    id
                ));

                Err(StatusCode::NoSuchFile)
            }
        }
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        // DEBUG: SFTP request summary
        debug!(
            "SFTP request: SSH_FXP_READDIR id={}, handle_len={} bytes",
            id,
            handle.len()
        );
        let _ = self.status_tx.send(format!(
            "[DEBUG] SFTP request: SSH_FXP_READDIR id={}, handle_len={} bytes",
            id,
            handle.len()
        ));

        // TRACE: Full SFTP request
        trace!(
            "SFTP SSH_FXP_READDIR request: id={}, handle='{}'",
            id,
            handle
        );
        let _ = self.status_tx.send(format!(
            "[TRACE] SFTP SSH_FXP_READDIR request: id={}, handle='{}'",
            id, handle
        ));

        // Check if we've already read this directory
        let mut handles = self.handles.lock().await;
        if let Some(handle_info) = handles.get_mut(&handle) {
            if handle_info.dir_read_done {
                // DEBUG: SFTP EOF response
                debug!(
                    "SFTP response: SSH_FXP_STATUS id={}, status=EOF (directory already read)",
                    id
                );
                let _ = self.status_tx.send(format!("[DEBUG] SFTP response: SSH_FXP_STATUS id={}, status=EOF (directory already read)", id));

                return Err(StatusCode::Eof);
            }

            let path = handle_info.path.clone();
            drop(handles); // Release lock before async operation

            let params = serde_json::json!({ "path": path, "handle": handle });
            match self.llm_sftp_operation("readdir", params).await {
                Ok(response) => {
                    if let Some(status) = sftp_error_status(&response) {
                        debug!("SFTP readdir refused by handler for {}: {:?}", path, status);
                        return Err(status);
                    }

                    // Parse file list from LLM
                    let files = response
                        .get("entries")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|entry| {
                                    // Only 'name' is load-bearing. is_dir/size used to be
                                    // fetched with `?`, so an entry that omitted either was
                                    // silently dropped from the listing.
                                    let name = entry.get("name")?.as_str()?.to_string();
                                    let is_dir = entry
                                        .get("is_dir")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false);
                                    let size =
                                        entry.get("size").and_then(|v| v.as_u64()).unwrap_or(0);

                                    // Set permissions: 0755 for dirs, 0644 for files
                                    let attrs = FileAttributes {
                                        size: Some(size),
                                        permissions: Some(if is_dir { 0o40755 } else { 0o100644 }),
                                        ..Default::default()
                                    };

                                    Some(File::new(name, attrs))
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();

                    // Mark directory as read
                    if let Some(h) = self.handles.lock().await.get_mut(&handle) {
                        h.dir_read_done = true;
                    }

                    let _ = self.status_tx.send(format!(
                        "→ SFTP listed {} items in {}",
                        files.len(),
                        path
                    ));

                    // DEBUG: SFTP response summary
                    debug!(
                        "SFTP response: SSH_FXP_NAME id={}, file_count={}",
                        id,
                        files.len()
                    );
                    let _ = self.status_tx.send(format!(
                        "[DEBUG] SFTP response: SSH_FXP_NAME id={}, file_count={}",
                        id,
                        files.len()
                    ));

                    // TRACE: Full SFTP response
                    let file_names: Vec<&str> = files.iter().map(|f| f.filename.as_str()).collect();
                    trace!(
                        "SFTP SSH_FXP_NAME response: id={}, files={:?}",
                        id,
                        file_names
                    );
                    let _ = self.status_tx.send(format!(
                        "[TRACE] SFTP SSH_FXP_NAME response: id={}, files={:?}",
                        id, file_names
                    ));

                    Ok(Name { id, files })
                }
                Err(_) => {
                    error!("SFTP readdir LLM error for handle: {}", handle);
                    let _ = self.status_tx.send(format!(
                        "[ERROR] SFTP readdir LLM error for handle: {}",
                        handle
                    ));

                    // DEBUG: SFTP error response
                    debug!("SFTP response: SSH_FXP_STATUS id={}, status=FAILURE", id);
                    let _ = self.status_tx.send(format!(
                        "[DEBUG] SFTP response: SSH_FXP_STATUS id={}, status=FAILURE",
                        id
                    ));

                    Err(StatusCode::Failure)
                }
            }
        } else {
            error!("SFTP readdir: invalid handle {}", handle);
            let _ = self
                .status_tx
                .send(format!("[ERROR] SFTP readdir: invalid handle {}", handle));

            // DEBUG: SFTP error response
            debug!(
                "SFTP response: SSH_FXP_STATUS id={}, status=BAD_MESSAGE",
                id
            );
            let _ = self.status_tx.send(format!(
                "[DEBUG] SFTP response: SSH_FXP_STATUS id={}, status=BAD_MESSAGE",
                id
            ));

            Err(StatusCode::BadMessage)
        }
    }

    async fn open(
        &mut self,
        id: u32,
        path: String,
        pflags: OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        // DEBUG: SFTP request summary
        debug!(
            "SFTP request: SSH_FXP_OPEN id={}, path={}, flags={:?}",
            id, path, pflags
        );
        let _ = self.status_tx.send(format!(
            "[DEBUG] SFTP request: SSH_FXP_OPEN id={}, path={}, flags={:?}",
            id, path, pflags
        ));

        // TRACE: Full SFTP request
        trace!(
            "SFTP SSH_FXP_OPEN request: id={}, path='{}', flags={:?}",
            id,
            path,
            pflags
        );
        let _ = self.status_tx.send(format!(
            "[TRACE] SFTP SSH_FXP_OPEN request: id={}, path='{}', flags={:?}",
            id, path, pflags
        ));

        let params = serde_json::json!({ "path": path });
        match self.llm_sftp_operation("open", params).await {
            Ok(response) => {
                if let Some(status) = sftp_error_status(&response) {
                    debug!("SFTP open refused by handler for {}: {:?}", path, status);
                    let _ = self
                        .status_tx
                        .send(format!("[DEBUG] SFTP open refused for {path}"));
                    return Err(status);
                }

                let handle_str = response
                    .get("handle")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&path)
                    .to_string();

                // Store handle info
                self.handles.lock().await.insert(
                    handle_str.clone(),
                    HandleInfo {
                        path: path.clone(),
                        is_directory: false,
                        dir_read_done: false,
                    },
                );

                let _ = self.status_tx.send(format!("→ SFTP opened file: {}", path));

                // DEBUG: SFTP response summary
                debug!(
                    "SFTP response: SSH_FXP_HANDLE id={}, handle_len={} bytes",
                    id,
                    handle_str.len()
                );
                let _ = self.status_tx.send(format!(
                    "[DEBUG] SFTP response: SSH_FXP_HANDLE id={}, handle_len={} bytes",
                    id,
                    handle_str.len()
                ));

                // TRACE: Full SFTP response
                trace!(
                    "SFTP SSH_FXP_HANDLE response: id={}, handle='{}'",
                    id,
                    handle_str
                );
                let _ = self.status_tx.send(format!(
                    "[TRACE] SFTP SSH_FXP_HANDLE response: id={}, handle='{}'",
                    id, handle_str
                ));

                Ok(Handle {
                    id,
                    handle: handle_str,
                })
            }
            Err(_) => {
                error!("SFTP open failed for path: {}", path);
                let _ = self
                    .status_tx
                    .send(format!("[ERROR] SFTP open failed for path: {}", path));

                // DEBUG: SFTP error response
                debug!(
                    "SFTP response: SSH_FXP_STATUS id={}, status=NO_SUCH_FILE",
                    id
                );
                let _ = self.status_tx.send(format!(
                    "[DEBUG] SFTP response: SSH_FXP_STATUS id={}, status=NO_SUCH_FILE",
                    id
                ));

                Err(StatusCode::NoSuchFile)
            }
        }
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        // DEBUG: SFTP request summary
        debug!(
            "SFTP request: SSH_FXP_READ id={}, offset={}, len={} bytes",
            id, offset, len
        );
        let _ = self.status_tx.send(format!(
            "[DEBUG] SFTP request: SSH_FXP_READ id={}, offset={}, len={} bytes",
            id, offset, len
        ));

        // TRACE: Full SFTP request
        trace!(
            "SFTP SSH_FXP_READ request: id={}, handle='{}', offset={}, len={}",
            id,
            handle,
            offset,
            len
        );
        let _ = self.status_tx.send(format!(
            "[TRACE] SFTP SSH_FXP_READ request: id={}, handle='{}', offset={}, len={}",
            id, handle, offset, len
        ));

        let handles = self.handles.lock().await;
        if let Some(handle_info) = handles.get(&handle) {
            let path = handle_info.path.clone();
            drop(handles);

            let params = serde_json::json!({
                "path": path,
                "handle": handle,
                "offset": offset,
                "length": len,
            });
            match self.llm_sftp_operation("read", params).await {
                Ok(response) => {
                    if let Some(status) = sftp_error_status(&response) {
                        debug!("SFTP read refused by handler for {}: {:?}", path, status);
                        return Err(status);
                    }

                    // Get file content from LLM
                    let content = response
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    // The handler returns the WHOLE file; apply the requested window here.
                    // Returning the full content for every request made a client that reads
                    // in chunks receive the file repeatedly and never see EOF.
                    let full = content.as_bytes();
                    let start = usize::try_from(offset).unwrap_or(usize::MAX);
                    if start >= full.len() {
                        debug!(
                            "SFTP response: SSH_FXP_STATUS id={}, status=EOF (offset {} >= size {})",
                            id,
                            offset,
                            full.len()
                        );
                        return Err(StatusCode::Eof);
                    }
                    let end = start.saturating_add(len as usize).min(full.len());
                    let data = full[start..end].to_vec();
                    let actual_len = data.len();

                    let _ = self
                        .status_tx
                        .send(format!("→ SFTP read {} bytes from {}", actual_len, path));

                    // DEBUG: SFTP response summary
                    debug!(
                        "SFTP response: SSH_FXP_DATA id={}, data_len={} bytes",
                        id, actual_len
                    );
                    let _ = self.status_tx.send(format!(
                        "[DEBUG] SFTP response: SSH_FXP_DATA id={}, data_len={} bytes",
                        id, actual_len
                    ));

                    // TRACE: Full SFTP response (truncate if too long)
                    if actual_len <= 256 {
                        trace!(
                            "SFTP SSH_FXP_DATA response: id={}, data={:?}",
                            id,
                            String::from_utf8_lossy(&data)
                        );
                        let _ = self.status_tx.send(format!(
                            "[TRACE] SFTP SSH_FXP_DATA response: id={}, data={:?}",
                            id,
                            String::from_utf8_lossy(&data)
                        ));
                    } else {
                        trace!(
                            "SFTP SSH_FXP_DATA response: id={}, data_len={} bytes (truncated)",
                            id,
                            actual_len
                        );
                        let _ = self.status_tx.send(format!("[TRACE] SFTP SSH_FXP_DATA response: id={}, data_len={} bytes (truncated)", id, actual_len));
                    }

                    Ok(Data { id, data })
                }
                Err(_) => {
                    error!("SFTP read LLM error for handle: {}", handle);
                    let _ = self.status_tx.send(format!(
                        "[ERROR] SFTP read LLM error for handle: {}",
                        handle
                    ));

                    // DEBUG: SFTP error response
                    debug!("SFTP response: SSH_FXP_STATUS id={}, status=EOF", id);
                    let _ = self.status_tx.send(format!(
                        "[DEBUG] SFTP response: SSH_FXP_STATUS id={}, status=EOF",
                        id
                    ));

                    Err(StatusCode::Eof)
                }
            }
        } else {
            error!("SFTP read: invalid handle {}", handle);
            let _ = self
                .status_tx
                .send(format!("[ERROR] SFTP read: invalid handle {}", handle));

            // DEBUG: SFTP error response
            debug!(
                "SFTP response: SSH_FXP_STATUS id={}, status=BAD_MESSAGE",
                id
            );
            let _ = self.status_tx.send(format!(
                "[DEBUG] SFTP response: SSH_FXP_STATUS id={}, status=BAD_MESSAGE",
                id
            ));

            Err(StatusCode::BadMessage)
        }
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        // DEBUG: SFTP request summary
        debug!(
            "SFTP request: SSH_FXP_CLOSE id={}, handle_len={} bytes",
            id,
            handle.len()
        );
        let _ = self.status_tx.send(format!(
            "[DEBUG] SFTP request: SSH_FXP_CLOSE id={}, handle_len={} bytes",
            id,
            handle.len()
        ));

        // TRACE: Full SFTP request
        trace!("SFTP SSH_FXP_CLOSE request: id={}, handle='{}'", id, handle);
        let _ = self.status_tx.send(format!(
            "[TRACE] SFTP SSH_FXP_CLOSE request: id={}, handle='{}'",
            id, handle
        ));

        // Remove handle from tracking
        if let Some(handle_info) = self.handles.lock().await.remove(&handle) {
            let _ = self
                .status_tx
                .send(format!("→ SFTP closed: {}", handle_info.path));

            // DEBUG: SFTP response summary
            debug!(
                "SFTP response: SSH_FXP_STATUS id={}, status=OK (closed '{}')",
                id, handle_info.path
            );
            let _ = self.status_tx.send(format!(
                "[DEBUG] SFTP response: SSH_FXP_STATUS id={}, status=OK (closed '{}')",
                id, handle_info.path
            ));

            // TRACE: Full SFTP response
            trace!(
                "SFTP SSH_FXP_STATUS response: id={}, status=OK, path='{}'",
                id,
                handle_info.path
            );
            let _ = self.status_tx.send(format!(
                "[TRACE] SFTP SSH_FXP_STATUS response: id={}, status=OK, path='{}'",
                id, handle_info.path
            ));
        } else {
            // DEBUG: Unknown handle closed (still return OK per SFTP spec)
            debug!(
                "SFTP response: SSH_FXP_STATUS id={}, status=OK (unknown handle)",
                id
            );
            let _ = self.status_tx.send(format!(
                "[DEBUG] SFTP response: SSH_FXP_STATUS id={}, status=OK (unknown handle)",
                id
            ));
        }

        Ok(Status {
            id,
            status_code: StatusCode::Ok,
            error_message: "".to_string(),
            language_tag: "en-US".to_string(),
        })
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        // DEBUG: SFTP request summary
        debug!("SFTP request: SSH_FXP_LSTAT id={}, path={}", id, path);
        let _ = self.status_tx.send(format!(
            "[DEBUG] SFTP request: SSH_FXP_LSTAT id={}, path={}",
            id, path
        ));

        // TRACE: Full SFTP request
        trace!("SFTP SSH_FXP_LSTAT request: id={}, path='{}'", id, path);
        let _ = self.status_tx.send(format!(
            "[TRACE] SFTP SSH_FXP_LSTAT request: id={}, path='{}'",
            id, path
        ));

        let params = serde_json::json!({ "path": path });
        match self.llm_sftp_operation("lstat", params).await {
            Ok(response) => {
                if let Some(status) = sftp_error_status(&response) {
                    debug!("SFTP lstat refused by handler for {}: {:?}", path, status);
                    let _ = self
                        .status_tx
                        .send(format!("[DEBUG] SFTP lstat refused for {path}"));
                    return Err(status);
                }

                let mut attrs = FileAttributes::default();

                // Parse attributes from LLM response
                if let Some(size) = response.get("size").and_then(|v| v.as_u64()) {
                    attrs.size = Some(size);
                }
                if let Some(perms) = response.get("permissions").and_then(|v| v.as_u64()) {
                    attrs.permissions = Some(perms as u32);
                }
                if let Some(is_dir) = response.get("is_dir").and_then(|v| v.as_bool()) {
                    // Set directory bit if needed
                    if is_dir {
                        attrs.permissions = Some(attrs.permissions.unwrap_or(0o644) | 0o40000);
                    }
                }

                // DEBUG: SFTP response summary
                debug!(
                    "SFTP response: SSH_FXP_ATTRS id={}, size={:?}, perms={:?}",
                    id, attrs.size, attrs.permissions
                );
                let _ = self.status_tx.send(format!(
                    "[DEBUG] SFTP response: SSH_FXP_ATTRS id={}, size={:?}, perms={:?}",
                    id, attrs.size, attrs.permissions
                ));

                // TRACE: Full SFTP response
                trace!(
                    "SFTP SSH_FXP_ATTRS response: id={}, path='{}', attrs={:?}",
                    id,
                    path,
                    attrs
                );
                let _ = self.status_tx.send(format!(
                    "[TRACE] SFTP SSH_FXP_ATTRS response: id={}, path='{}', attrs={:?}",
                    id, path, attrs
                ));

                Ok(Attrs { id, attrs })
            }
            Err(_) => {
                error!("SFTP lstat failed for path: {}", path);
                let _ = self
                    .status_tx
                    .send(format!("[ERROR] SFTP lstat failed for path: {}", path));

                // DEBUG: SFTP error response
                debug!(
                    "SFTP response: SSH_FXP_STATUS id={}, status=NO_SUCH_FILE",
                    id
                );
                let _ = self.status_tx.send(format!(
                    "[DEBUG] SFTP response: SSH_FXP_STATUS id={}, status=NO_SUCH_FILE",
                    id
                ));

                Err(StatusCode::NoSuchFile)
            }
        }
    }

    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, Self::Error> {
        // DEBUG: SFTP request summary
        debug!(
            "SFTP request: SSH_FXP_FSTAT id={}, handle_len={} bytes",
            id,
            handle.len()
        );
        let _ = self.status_tx.send(format!(
            "[DEBUG] SFTP request: SSH_FXP_FSTAT id={}, handle_len={} bytes",
            id,
            handle.len()
        ));

        // TRACE: Full SFTP request
        trace!("SFTP SSH_FXP_FSTAT request: id={}, handle='{}'", id, handle);
        let _ = self.status_tx.send(format!(
            "[TRACE] SFTP SSH_FXP_FSTAT request: id={}, handle='{}'",
            id, handle
        ));

        let handles = self.handles.lock().await;
        if let Some(handle_info) = handles.get(&handle) {
            let path = handle_info.path.clone();
            drop(handles);

            // Delegate to lstat (which will log the lstat request/response)
            trace!("SFTP fstat delegating to lstat for path: '{}'", path);
            let _ = self.status_tx.send(format!(
                "[TRACE] SFTP fstat delegating to lstat for path: '{}'",
                path
            ));
            self.lstat(id, path).await
        } else {
            error!("SFTP fstat: invalid handle {}", handle);
            let _ = self
                .status_tx
                .send(format!("[ERROR] SFTP fstat: invalid handle {}", handle));

            // DEBUG: SFTP error response
            debug!(
                "SFTP response: SSH_FXP_STATUS id={}, status=BAD_MESSAGE",
                id
            );
            let _ = self.status_tx.send(format!(
                "[DEBUG] SFTP response: SSH_FXP_STATUS id={}, status=BAD_MESSAGE",
                id
            ));

            Err(StatusCode::BadMessage)
        }
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        // DEBUG: SFTP request summary
        debug!("SFTP request: SSH_FXP_REALPATH id={}, path={}", id, path);
        let _ = self.status_tx.send(format!(
            "[DEBUG] SFTP request: SSH_FXP_REALPATH id={}, path={}",
            id, path
        ));

        // TRACE: Full SFTP request
        trace!("SFTP SSH_FXP_REALPATH request: id={}, path='{}'", id, path);
        let _ = self.status_tx.send(format!(
            "[TRACE] SFTP SSH_FXP_REALPATH request: id={}, path='{}'",
            id, path
        ));

        // Answered locally - the handler is not consulted. OpenSSH opens every session with
        // realpath("."), and echoing "." straight back leaves the client with a relative
        // working directory that every later path is resolved against; map it to "/".
        let path = if path.is_empty() || path == "." {
            "/".to_string()
        } else {
            path
        };

        let attrs = FileAttributes::default();
        let file = File::new(path.clone(), attrs);

        // DEBUG: SFTP response summary
        debug!(
            "SFTP response: SSH_FXP_NAME id={}, resolved_path={}",
            id, path
        );
        let _ = self.status_tx.send(format!(
            "[DEBUG] SFTP response: SSH_FXP_NAME id={}, resolved_path={}",
            id, path
        ));

        // TRACE: Full SFTP response
        trace!("SFTP SSH_FXP_NAME response: id={}, path='{}'", id, path);
        let _ = self.status_tx.send(format!(
            "[TRACE] SFTP SSH_FXP_NAME response: id={}, path='{}'",
            id, path
        ));

        Ok(Name {
            id,
            files: vec![file],
        })
    }

    fn unimplemented(&self) -> Self::Error {
        error!("SFTP unimplemented packet received");
        let _ = self
            .status_tx
            .send("[ERROR] SFTP unimplemented packet received".to_string());
        StatusCode::OpUnsupported
    }
}
