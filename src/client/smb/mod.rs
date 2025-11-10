//! SMB client implementation
pub mod actions;

pub use actions::SmbClientProtocol;

use anyhow::{Context, Result};
use crate::llm::actions::client_trait::Client;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, debug};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::{Event, StartupParams};
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::smb::actions::{
    SMB_CLIENT_CONNECTED_EVENT, SMB_CLIENT_DIR_LISTED_EVENT,
    SMB_CLIENT_FILE_READ_EVENT, SMB_CLIENT_FILE_WRITTEN_EVENT,
    SMB_CLIENT_ERROR_EVENT,
};

use pavao::{SmbClient as PavaoSmbClient, SmbCredentials, SmbOptions, SmbDirent, SmbDirentType, SmbOpenOptions};
use std::io::{Read as IoRead, Write as IoWrite};

/// SMB client that connects to an SMB/CIFS server
pub struct SmbClient;

impl SmbClient {
    /// Connect to an SMB server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<StartupParams>,
    ) -> Result<SocketAddr> {
        info!("SMB client {} initializing connection to {}", client_id, remote_addr);

        // Parse startup parameters for credentials
        let (username, password, domain, workgroup) = if let Some(params) = startup_params {
            let username = params
                .get_optional_string("username")
                .unwrap_or_else(|| "guest".to_string());
            let password = params
                .get_optional_string("password")
                .unwrap_or_else(|| "".to_string());
            let domain = params.get_optional_string("domain");
            let workgroup = params.get_optional_string("workgroup");

            (username, password, domain, workgroup)
        } else {
            ("guest".to_string(), "".to_string(), None, None)
        };

        info!(
            "SMB client {} using credentials - username: {}, domain: {:?}, workgroup: {:?}",
            client_id, username, domain, workgroup
        );

        // Create SMB credentials using builder pattern
        let mut creds = SmbCredentials::default()
            .server(&remote_addr)
            .username(&username)
            .password(&password);

        // Note: pavao API uses workgroup() for both domain and workgroup
        if let Some(w) = workgroup.or(domain) {
            creds = creds.workgroup(&w);
        }

        // Create SMB client
        let smb_client = PavaoSmbClient::new(creds, SmbOptions::default())
            .context("Failed to create SMB client")?;

        // For SMB, we use a dummy local address since it's a library-based client
        // The actual connection happens per-operation
        let local_addr = "127.0.0.1:0".parse::<SocketAddr>()?;

        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] SMB client {} connected", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        info!("SMB client {} connected to {}", client_id, remote_addr);

        // Spawn task to handle LLM interactions
        let app_state_clone = app_state.clone();
        let status_tx_clone = status_tx.clone();
        let remote_addr_clone = remote_addr.clone();

        tokio::spawn(async move {
            // Send initial connected event to LLM
            if let Some(instruction) = app_state_clone.get_instruction_for_client(client_id).await {
                let protocol = Arc::new(SmbClientProtocol::new());
                let event = Event::new(
                    &SMB_CLIENT_CONNECTED_EVENT,
                    serde_json::json!({
                        "share_url": format!("smb://{}", remote_addr_clone),
                    }),
                );

                let memory = app_state_clone.get_memory_for_client(client_id).await.unwrap_or_default();

                match call_llm_for_client(
                    &llm_client,
                    &app_state_clone,
                    client_id.to_string(),
                    &instruction,
                    &memory,
                    Some(&event),
                    protocol.as_ref(),
                    &status_tx_clone,
                ).await {
                    Ok(ClientLlmResult { actions, memory_updates }) => {
                        // Update memory
                        if let Some(mem) = memory_updates {
                            app_state_clone.set_memory_for_client(client_id, mem).await;
                        }

                        // Execute actions
                        for action in actions {
                            if let Err(e) = Self::execute_smb_action(
                                &smb_client,
                                action,
                                client_id,
                                &protocol,
                                &llm_client,
                                &app_state_clone,
                                &status_tx_clone,
                            ).await {
                                error!("SMB client {} action error: {}", client_id, e);

                                // Send error event to LLM
                                let error_event = Event::new(
                                    &SMB_CLIENT_ERROR_EVENT,
                                    serde_json::json!({
                                        "error": e.to_string(),
                                        "operation": "action_execution",
                                    }),
                                );

                                let _ = Self::call_llm_with_event(
                                    &error_event,
                                    client_id,
                                    &protocol,
                                    &llm_client,
                                    &app_state_clone,
                                    &status_tx_clone,
                                    &smb_client,
                                ).await;
                            }
                        }
                    }
                    Err(e) => {
                        error!("LLM error for SMB client {}: {}", client_id, e);
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Execute an SMB action and call LLM with result
    async fn execute_smb_action(
        smb_client: &PavaoSmbClient,
        action: serde_json::Value,
        client_id: ClientId,
        protocol: &Arc<SmbClientProtocol>,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        match protocol.execute_action(action)? {
            crate::llm::actions::client_trait::ClientActionResult::Custom { name, data } => {
                match name.as_str() {
                    "smb_list_dir" => {
                        let path = data
                            .get("path")
                            .and_then(|v| v.as_str())
                            .context("Missing path")?;

                        debug!("SMB client {} listing directory: {}", client_id, path);

                        // List directory using pavao
                        match smb_client.list_dir(path) {
                            Ok(entries) => {
                                let entry_list: Vec<serde_json::Value> = entries
                                    .iter()
                                    .map(|entry| {
                                        let dirent: &SmbDirent = entry;
                                        serde_json::json!({
                                            "name": dirent.name(),
                                            "type": match dirent.get_type() {
                                                SmbDirentType::Workgroup => "workgroup",
                                                SmbDirentType::Server => "server",
                                                SmbDirentType::FileShare => "file_share",
                                                SmbDirentType::PrinterShare => "printer_share",
                                                SmbDirentType::CommsShare => "comms_share",
                                                SmbDirentType::IpcShare => "ipc_share",
                                                SmbDirentType::Dir => "dir",
                                                SmbDirentType::File => "file",
                                                SmbDirentType::Link => "link",
                                            },
                                            "comment": dirent.comment(),
                                        })
                                    })
                                    .collect();

                                info!("SMB client {} listed {} entries in {}", client_id, entry_list.len(), path);

                                let event = Event::new(
                                    &SMB_CLIENT_DIR_LISTED_EVENT,
                                    serde_json::json!({
                                        "path": path,
                                        "entries": entry_list,
                                    }),
                                );

                                Self::call_llm_with_event(
                                    &event,
                                    client_id,
                                    protocol,
                                    llm_client,
                                    app_state,
                                    status_tx,
                                    smb_client,
                                ).await?;
                            }
                            Err(e) => {
                                error!("SMB client {} list_dir error: {}", client_id, e);
                                let error_event = Event::new(
                                    &SMB_CLIENT_ERROR_EVENT,
                                    serde_json::json!({
                                        "error": e.to_string(),
                                        "operation": "list_directory",
                                    }),
                                );

                                Self::call_llm_with_event(
                                    &error_event,
                                    client_id,
                                    protocol,
                                    llm_client,
                                    app_state,
                                    status_tx,
                                    smb_client,
                                ).await?;
                            }
                        }
                    }
                    "smb_read_file" => {
                        let path = data
                            .get("path")
                            .and_then(|v| v.as_str())
                            .context("Missing path")?;

                        debug!("SMB client {} reading file: {}", client_id, path);

                        // Open file for reading and immediately read contents
                        // We need to close the file before any await (SmbFile contains raw pointer, not Send)
                        let read_result = smb_client.open_with(
                            path,
                            SmbOpenOptions::default().read(true)
                        ).and_then(|mut file| {
                            let mut content_bytes = Vec::new();
                            file.read_to_end(&mut content_bytes)?;
                            Ok(content_bytes)
                        });

                        match read_result {
                            Ok(content_bytes) => {
                                let size = content_bytes.len();

                                // Try to convert to UTF-8 string, fallback to base64 for binary
                                let content = if let Ok(text) = String::from_utf8(content_bytes.clone()) {
                                    text
                                } else {
                                    use base64::{Engine as _, engine::general_purpose};
                                    format!("base64:{}", general_purpose::STANDARD.encode(&content_bytes))
                                };

                                info!("SMB client {} read {} bytes from {}", client_id, size, path);

                                let event = Event::new(
                                    &SMB_CLIENT_FILE_READ_EVENT,
                                    serde_json::json!({
                                        "path": path,
                                        "content": content,
                                        "size": size,
                                    }),
                                );

                                Self::call_llm_with_event(
                                    &event,
                                    client_id,
                                    protocol,
                                    llm_client,
                                    app_state,
                                    status_tx,
                                    smb_client,
                                ).await?;
                            }
                            Err(e) => {
                                error!("SMB client {} read_file error: {}", client_id, e);
                                let error_event = Event::new(
                                    &SMB_CLIENT_ERROR_EVENT,
                                    serde_json::json!({
                                        "error": e.to_string(),
                                        "operation": "read_file",
                                    }),
                                );

                                Self::call_llm_with_event(
                                    &error_event,
                                    client_id,
                                    protocol,
                                    llm_client,
                                    app_state,
                                    status_tx,
                                    smb_client,
                                ).await?;
                            }
                        }
                    }
                    "smb_write_file" => {
                        let path = data
                            .get("path")
                            .and_then(|v| v.as_str())
                            .context("Missing path")?;
                        let content = data
                            .get("content")
                            .and_then(|v| v.as_str())
                            .context("Missing content")?;

                        debug!("SMB client {} writing file: {}", client_id, path);

                        let content_bytes = content.as_bytes();

                        // Open file for writing and immediately write contents
                        // We need to close the file before any await (SmbFile contains raw pointer, not Send)
                        let write_result = smb_client.open_with(
                            path,
                            SmbOpenOptions::default().write(true).create(true).truncate(true)
                        ).and_then(|mut file| {
                            file.write_all(content_bytes)?;
                            Ok(content_bytes.len())
                        });

                        match write_result {
                            Ok(bytes_written) => {
                                info!("SMB client {} wrote {} bytes to {}", client_id, bytes_written, path);

                                let event = Event::new(
                                    &SMB_CLIENT_FILE_WRITTEN_EVENT,
                                    serde_json::json!({
                                        "path": path,
                                        "bytes_written": bytes_written,
                                    }),
                                );

                                Self::call_llm_with_event(
                                    &event,
                                    client_id,
                                    protocol,
                                    llm_client,
                                    app_state,
                                    status_tx,
                                    smb_client,
                                ).await?;
                            }
                            Err(e) => {
                                error!("SMB client {} write_file error: {}", client_id, e);
                                let error_event = Event::new(
                                    &SMB_CLIENT_ERROR_EVENT,
                                    serde_json::json!({
                                        "error": e.to_string(),
                                        "operation": "write_file",
                                    }),
                                );

                                Self::call_llm_with_event(
                                    &error_event,
                                    client_id,
                                    protocol,
                                    llm_client,
                                    app_state,
                                    status_tx,
                                    smb_client,
                                ).await?;
                            }
                        }
                    }
                    "smb_create_dir" => {
                        let path = data
                            .get("path")
                            .and_then(|v| v.as_str())
                            .context("Missing path")?;

                        debug!("SMB client {} creating directory: {}", client_id, path);

                        match smb_client.mkdir(path, pavao::SmbMode::from(0o755)) {
                            Ok(()) => {
                                info!("SMB client {} created directory {}", client_id, path);
                                let _ = status_tx.send(format!("[CLIENT] SMB client {} created directory: {}", client_id, path));
                            }
                            Err(e) => {
                                error!("SMB client {} mkdir error: {}", client_id, e);
                                let error_event = Event::new(
                                    &SMB_CLIENT_ERROR_EVENT,
                                    serde_json::json!({
                                        "error": e.to_string(),
                                        "operation": "create_directory",
                                    }),
                                );

                                Self::call_llm_with_event(
                                    &error_event,
                                    client_id,
                                    protocol,
                                    llm_client,
                                    app_state,
                                    status_tx,
                                    smb_client,
                                ).await?;
                            }
                        }
                    }
                    "smb_delete_file" => {
                        let path = data
                            .get("path")
                            .and_then(|v| v.as_str())
                            .context("Missing path")?;

                        debug!("SMB client {} deleting file: {}", client_id, path);

                        match smb_client.unlink(path) {
                            Ok(()) => {
                                info!("SMB client {} deleted file {}", client_id, path);
                                let _ = status_tx.send(format!("[CLIENT] SMB client {} deleted file: {}", client_id, path));
                            }
                            Err(e) => {
                                error!("SMB client {} unlink error: {}", client_id, e);
                                let error_event = Event::new(
                                    &SMB_CLIENT_ERROR_EVENT,
                                    serde_json::json!({
                                        "error": e.to_string(),
                                        "operation": "delete_file",
                                    }),
                                );

                                Self::call_llm_with_event(
                                    &error_event,
                                    client_id,
                                    protocol,
                                    llm_client,
                                    app_state,
                                    status_tx,
                                    smb_client,
                                ).await?;
                            }
                        }
                    }
                    "smb_delete_dir" => {
                        let path = data
                            .get("path")
                            .and_then(|v| v.as_str())
                            .context("Missing path")?;

                        debug!("SMB client {} deleting directory: {}", client_id, path);

                        match smb_client.rmdir(path) {
                            Ok(()) => {
                                info!("SMB client {} deleted directory {}", client_id, path);
                                let _ = status_tx.send(format!("[CLIENT] SMB client {} deleted directory: {}", client_id, path));
                            }
                            Err(e) => {
                                error!("SMB client {} rmdir error: {}", client_id, e);
                                let error_event = Event::new(
                                    &SMB_CLIENT_ERROR_EVENT,
                                    serde_json::json!({
                                        "error": e.to_string(),
                                        "operation": "delete_directory",
                                    }),
                                );

                                Self::call_llm_with_event(
                                    &error_event,
                                    client_id,
                                    protocol,
                                    llm_client,
                                    app_state,
                                    status_tx,
                                    smb_client,
                                ).await?;
                            }
                        }
                    }
                    _ => {
                        error!("SMB client {} unknown action: {}", client_id, name);
                    }
                }
            }
            crate::llm::actions::client_trait::ClientActionResult::Disconnect => {
                info!("SMB client {} disconnecting", client_id);
                app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                let _ = status_tx.send(format!("[CLIENT] SMB client {} disconnected", client_id));
                let _ = status_tx.send("__UPDATE_UI__".to_string());
            }
            crate::llm::actions::client_trait::ClientActionResult::WaitForMore => {
                debug!("SMB client {} waiting for more", client_id);
            }
            _ => {}
        }

        Ok(())
    }

    /// Call LLM with an event and execute resulting actions
    async fn call_llm_with_event(
        event: &Event,
        client_id: ClientId,
        protocol: &Arc<SmbClientProtocol>,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        smb_client: &PavaoSmbClient,
    ) -> Result<()> {
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

            match call_llm_for_client(
                llm_client,
                app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(event),
                protocol.as_ref(),
                status_tx,
            ).await {
                Ok(ClientLlmResult { actions, memory_updates }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }

                    // Execute actions
                    for action in actions {
                        Box::pin(Self::execute_smb_action(
                            smb_client,
                            action,
                            client_id,
                            protocol,
                            llm_client,
                            app_state,
                            status_tx,
                        )).await?;
                    }
                }
                Err(e) => {
                    error!("LLM error for SMB client {}: {}", client_id, e);
                }
            }
        }

        Ok(())
    }
}
