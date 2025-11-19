//! SMB/CIFS server implementation
//!
//! Provides an SMB2 file server where the LLM controls the virtual filesystem.
//! Uses guest-only authentication (no password required).

pub mod actions;

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace, warn};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::server::SmbProtocol;
use crate::state::app_state::AppState;
use crate::state::server::{
    ConnectionState as ServerConnectionState, ConnectionStatus, ProtocolConnectionInfo,
};
use crate::state::ServerId;

use crate::{console_info, console_warn};
use actions::SMB_OPERATION_EVENT;

/// SMB server that provides LLM-controlled file system
pub struct SmbServer;

/// SMB2 session state
#[derive(Debug, Clone)]
struct SmbSession {
    session_id: u64,
    username: String,
    _authenticated: bool,
}

/// SMB2 tree connection state
#[derive(Debug, Clone)]
struct SmbTreeConnect {
    _tree_id: u32,
    _share_name: String,
}

/// SMB2 file handle state
#[derive(Debug, Clone)]
struct SmbFileHandle {
    _file_id: Vec<u8>, // 16-byte GUID
    path: String,
    _is_directory: bool,
}

/// Per-connection SMB state
struct SmbConnectionState {
    sessions: HashMap<u64, SmbSession>,
    trees: HashMap<u32, SmbTreeConnect>,
    files: HashMap<Vec<u8>, SmbFileHandle>,
    next_session_id: u64,
    next_tree_id: u32,
}

impl SmbConnectionState {
    fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            trees: HashMap::new(),
            files: HashMap::new(),
            next_session_id: 1,
            next_tree_id: 1,
        }
    }
}

impl SmbServer {
    /// Spawn SMB server with integrated LLM actions
    #[cfg(feature = "smb")]
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: ServerId,
    ) -> Result<SocketAddr> {
        info!(
            "SMB server (LLM-controlled, guest-only) starting on {}",
            listen_addr
        );
        let _ = status_tx.send(format!("[INFO] SMB server starting on {}", listen_addr));

        let protocol = Arc::new(SmbProtocol::new());

        // Bind TCP listener
        let listener = TcpListener::bind(listen_addr)
            .await
            .context("Failed to bind SMB TCP listener")?;

        let actual_addr = listener.local_addr()?;
        info!("SMB server listening on {}", actual_addr);
        let _ = status_tx.send(format!("→ SMB server listening on {}", actual_addr));

        // Spawn connection acceptor
        tokio::spawn(async move {
            info!("SMB server connection acceptor started");

            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        error!("SMB_DEBUG: Connection ACCEPTED from {}", peer_addr);
                        let _ =
                            status_tx.send(format!("[DEBUG] SMB connection from {}", peer_addr));

                        // Spawn per-connection handler
                        let llm_client = llm_client.clone();
                        let app_state = app_state.clone();
                        let protocol = protocol.clone();
                        let status_tx = status_tx.clone();

                        error!("SMB_DEBUG: About to spawn handle_connection task for {}", peer_addr);
                        tokio::spawn(async move {
                            error!("SMB_DEBUG: handle_connection task STARTED for {}", peer_addr);
                            if let Err(e) = Self::handle_connection(
                                stream,
                                peer_addr,
                                llm_client,
                                app_state,
                                server_id,
                                protocol,
                                status_tx.clone(),
                            )
                            .await
                            {
                                error!("SMB connection error from {}: {}", peer_addr, e);
                                let _ = status_tx.send(format!(
                                    "✗ SMB connection error from {}: {}",
                                    peer_addr, e
                                ));
                            }
                            error!("SMB_DEBUG: handle_connection task COMPLETED for {}", peer_addr);
                        });
                    }
                    Err(e) => {
                        error!("SMB accept error: {}", e);
                        let _ = status_tx.send(format!("✗ SMB accept error: {}", e));
                    }
                }
            }
        });

        Ok(actual_addr)
    }

    /// Spawn SMB server without the smb feature (fallback)
    #[cfg(not(feature = "smb"))]
    pub async fn spawn_with_llm_actions(
        _listen_addr: SocketAddr,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        _status_tx: mpsc::UnboundedSender<String>,
        _server_id: ServerId,
    ) -> Result<SocketAddr> {
        Err(anyhow!("SMB feature not enabled"))
    }

    /// Handle a single SMB connection
    #[cfg(feature = "smb")]
    async fn handle_connection(
        mut stream: TcpStream,
        peer_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        server_id: ServerId,
        protocol: Arc<SmbProtocol>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Generate connection ID
        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);

        console_info!(
            status_tx,
            "SMB connection {} from {}",
            connection_id,
            peer_addr
        );

        // Get local address for tracking
        let local_addr = stream
            .local_addr()
            .unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap());

        // Track connection in app state
        let now = std::time::Instant::now();
        let conn_state = ServerConnectionState {
            id: connection_id,
            remote_addr: peer_addr,
            local_addr,
            bytes_sent: 0,
            bytes_received: 0,
            packets_sent: 0,
            packets_received: 0,
            last_activity: now,
            status: ConnectionStatus::Active,
            status_changed_at: now,
            protocol_info: ProtocolConnectionInfo::empty(),
        };

        app_state
            .add_connection_to_server(server_id, conn_state)
            .await;
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        let state = Arc::new(Mutex::new(SmbConnectionState::new()));

        // SMB2 protocol handling loop
        loop {
            // Read SMB2 message
            // SMB2 header is 64 bytes minimum
            let mut header_buf = vec![0u8; 64];

            warn!("SMB_DEBUG: Waiting for next message from {}", peer_addr);

            match stream.read_exact(&mut header_buf).await {
                Ok(_) => {
                    warn!("SMB_DEBUG: Received 64-byte header from {}", peer_addr);

                    // Update connection stats for received data
                    app_state
                        .update_connection_stats(
                            server_id,
                            connection_id,
                            Some(header_buf.len() as u64),
                            None,
                            Some(1),
                            None,
                        )
                        .await;

                    // Parse SMB2 header
                    if &header_buf[0..4] != b"\xFESMB" {
                        warn!("Invalid SMB2 signature from {}", peer_addr);
                        let _ = status_tx
                            .send(format!("[WARN] Invalid SMB2 signature from {}", peer_addr));
                        break;
                    }

                    // Extract command from header (offset 12-13, little-endian)
                    let command = u16::from_le_bytes([header_buf[12], header_buf[13]]);
                    warn!("SMB_DEBUG: SMB2 command 0x{:04x} from {}", command, peer_addr);

                    // Handle SMB2 command
                    let response = Self::handle_smb2_command(
                        command,
                        &header_buf,
                        &mut stream,
                        &llm_client,
                        &app_state,
                        server_id,
                        connection_id,
                        &protocol,
                        &state,
                        &status_tx,
                    )
                    .await?;

                    // Send response
                    if let Some(response_data) = response {
                        stream.write_all(&response_data).await?;
                        trace!(
                            "SMB2 response sent to {}, {} bytes",
                            peer_addr,
                            response_data.len()
                        );

                        // Update connection stats for sent data
                        app_state
                            .update_connection_stats(
                                server_id,
                                connection_id,
                                None,
                                Some(response_data.len() as u64),
                                None,
                                Some(1),
                            )
                            .await;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    console_info!(status_tx, "SMB client {} disconnected", peer_addr);
                    break;
                }
                Err(e) => {
                    error!("SMB read error from {}: {}", peer_addr, e);
                    let _ = status_tx.send(format!("✗ SMB read error from {}: {}", peer_addr, e));
                    break;
                }
            }
        }

        // Mark connection as closed
        app_state
            .update_connection_status(server_id, connection_id, ConnectionStatus::Closed)
            .await;
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        console_info!(status_tx, "SMB connection {} closed", connection_id);

        Ok(())
    }

    /// Handle SMB2 command
    #[cfg(feature = "smb")]
    #[allow(clippy::too_many_arguments)]
    async fn handle_smb2_command(
        command: u16,
        _header: &[u8],
        _stream: &mut TcpStream,
        _llm_client: &OllamaClient,
        _app_state: &Arc<AppState>,
        _server_id: ServerId,
        _connection_id: ConnectionId,
        _protocol: &Arc<SmbProtocol>,
        _state: &Arc<Mutex<SmbConnectionState>>,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<Option<Vec<u8>>> {
        // SMB2 command codes
        const SMB2_NEGOTIATE: u16 = 0x0000;
        const SMB2_SESSION_SETUP: u16 = 0x0001;
        const SMB2_TREE_CONNECT: u16 = 0x0003;
        const SMB2_CREATE: u16 = 0x0005;
        const SMB2_CLOSE: u16 = 0x0006;
        const SMB2_READ: u16 = 0x0008;
        const SMB2_WRITE: u16 = 0x0009;
        const SMB2_QUERY_INFO: u16 = 0x0010;
        const SMB2_QUERY_DIRECTORY: u16 = 0x000E;

        match command {
            SMB2_NEGOTIATE => {
                debug!("SMB2 NEGOTIATE request");
                let _ = status_tx.send("[DEBUG] SMB2 NEGOTIATE - offering SMB 2.1".to_string());

                // Consume NEGOTIATE request body from the stream
                // NEGOTIATE request body is 36 bytes (structure) + 2 bytes (dialect) = 38 bytes total
                // We read exactly 38 bytes to prevent consuming part of the next message
                let mut body_buf = [0u8; 38];
                match _stream.read_exact(&mut body_buf).await {
                    Ok(_) => {
                        error!("SMB_DEBUG: NEGOTIATE body read SUCCESS - 38 bytes consumed");
                    }
                    Err(e) => {
                        // ERROR: If we can't read the body, the stream is now out of sync!
                        // This will cause the next header read to fail
                        error!("SMB_DEBUG: NEGOTIATE body read FAILED: {} - THIS WILL BREAK THE CONNECTION!", e);
                        return Err(e.into());
                    }
                }

                // Build SMB2 Negotiate Response
                // For simplicity, we'll offer SMB 2.1 dialect (0x0210)
                let response = Self::build_negotiate_response(_header)?;
                Ok(Some(response))
            }
            SMB2_SESSION_SETUP => {
                warn!("SMB_DEBUG: SESSION_SETUP request received");

                // Read SESSION_SETUP request body (exactly 24 bytes for guest auth)
                let mut body_buf = [0u8; 24];
                match _stream.read_exact(&mut body_buf).await {
                    Ok(_) => {
                        debug!("SESSION_SETUP body: 24 bytes consumed");
                        let _ = status_tx.send("[DEBUG] SESSION_SETUP body read successfully".to_string());
                    }
                    Err(e) => {
                        warn!("Error reading SESSION_SETUP body: {} - continuing anyway", e);
                        let _ = status_tx.send(format!("[WARN] SESSION_SETUP body read error: {}", e));
                    }
                }
                let bytes_read = body_buf.len();

                // Try to extract username from security blob (simplified)
                // In real SMB2, this would be in the NTLMSSP blob
                // For simplicity, we'll check for a text username or use "guest"
                let username = Self::parse_smb2_username(&body_buf[..bytes_read])
                    .unwrap_or_else(|| "guest".to_string());

                info!("SMB2 SESSION_SETUP for user: {}", username);
                let _ = status_tx.send(format!("[INFO] SMB auth attempt: {}", username));

                // Consult LLM to check if this user should be authenticated
                let actions = Self::consult_llm(
                    _llm_client,
                    _app_state,
                    _server_id,
                    _protocol,
                    "session_setup",
                    serde_json::json!({
                        "operation": "authenticate",
                        "username": username,
                        "auth_type": if username == "guest" { "guest" } else { "password" }
                    }),
                    status_tx,
                )
                .await?;

                // Check if LLM allowed the authentication
                let auth_allowed = actions.iter().any(|a| {
                    a.get("type").and_then(|t| t.as_str()) == Some("smb_auth_success")
                        || a.get("type").and_then(|t| t.as_str()) == Some("allow_auth")
                });

                if !auth_allowed {
                    warn!("SMB authentication denied for user: {}", username);
                    let _ = status_tx.send(format!("✗ SMB auth denied: {}", username));

                    // Return ACCESS_DENIED response
                    let response = Self::build_auth_denied_response(_header)?;
                    return Ok(Some(response));
                }

                info!("SMB authentication successful for user: {}", username);
                let _ = status_tx.send(format!("→ SMB auth success: {}", username));

                // Build successful session setup response
                let response = Self::build_session_setup_response_with_user(
                    _header,
                    _state,
                    username.clone(),
                )?;

                // Get the session info from state to update connection tracking
                let (session_id, _auth_username) = {
                    let s = _state.lock().await;
                    if let Some(session) = s.sessions.values().last() {
                        (Some(session.session_id), Some(session.username.clone()))
                    } else {
                        (None, None)
                    }
                };

                // TODO: Update connection tracking with authentication info
                // Note: update_connection_protocol_info method doesn't exist yet
                if let Some(sid) = session_id {
                    // Future: add method to update SMB connection state
                    // For now, connection is tracked with initial protocol info
                    let _ = status_tx.send("__UPDATE_UI__".to_string());

                    info!(
                        "SMB session {} established for connection {}",
                        sid, _connection_id
                    );
                }

                Ok(Some(response))
            }
            SMB2_TREE_CONNECT => {
                debug!("SMB2 TREE_CONNECT request");
                let _ = status_tx.send("[DEBUG] SMB2 TREE_CONNECT - accepting share".to_string());

                // For simplicity, accept any tree connect with share name "share"
                let response =
                    Self::build_tree_connect_response(_header, _state, "share".to_string())?;
                Ok(Some(response))
            }
            SMB2_CREATE => {
                debug!("SMB2 CREATE request");

                // Read CREATE request body (variable length)
                // Structure size is at offset 0-1 of body (should be 57)
                let mut body_buf = vec![0u8; 512]; // Sufficient for most paths
                let bytes_read = _stream.read(&mut body_buf).await?;

                // Extract file path from request (simplified parsing)
                // Path is UTF-16LE encoded starting at offset 120 in the CREATE request
                let path = Self::parse_smb2_path(&body_buf[..bytes_read])
                    .unwrap_or_else(|| "/unknown".to_string());

                info!("SMB2 CREATE request for: {}", path);
                let _ = status_tx.send(format!("[INFO] SMB CREATE: {}", path));

                // Consult LLM to check if file exists and get info
                let _actions = Self::consult_llm(
                    _llm_client,
                    _app_state,
                    _server_id,
                    _protocol,
                    "create",
                    serde_json::json!({
                        "path": path,
                        "operation": "open_or_create"
                    }),
                    status_tx,
                )
                .await?;

                // Generate file handle (16-byte GUID)
                let file_id = Self::generate_file_handle();

                // Store file handle in state
                {
                    let mut s = _state.lock().await;
                    s.files.insert(
                        file_id.clone(),
                        SmbFileHandle {
                            _file_id: file_id.clone(),
                            path: path.clone(),
                            _is_directory: false, // TODO: Parse from LLM response
                        },
                    );
                }

                debug!("SMB2 CREATE: allocated file handle for {}", path);
                let response = Self::build_create_response(_header, &file_id)?;
                Ok(Some(response))
            }
            SMB2_CLOSE => {
                debug!("SMB2 CLOSE request");

                // Read CLOSE request body
                let mut body_buf = vec![0u8; 24]; // CLOSE body is 24 bytes
                _stream.read_exact(&mut body_buf).await?;

                // Extract file ID (16 bytes at offset 8)
                let file_id = body_buf[8..24].to_vec();

                // Remove file handle from state
                let path = {
                    let mut s = _state.lock().await;
                    s.files.remove(&file_id).map(|h| h.path)
                };

                if let Some(path) = path {
                    info!("SMB2 CLOSE: {}", path);
                    let _ = status_tx.send(format!("[INFO] SMB CLOSE: {}", path));
                } else {
                    warn!("SMB2 CLOSE: unknown file handle");
                    let _ = status_tx.send("[WARN] SMB CLOSE: unknown handle".to_string());
                }

                let response = Self::build_close_response(_header)?;
                Ok(Some(response))
            }
            SMB2_READ => {
                debug!("SMB2 READ request");

                // Read READ request body (49 bytes)
                let mut body_buf = vec![0u8; 49];
                _stream.read_exact(&mut body_buf).await?;

                // Extract file ID (16 bytes at offset 16)
                let file_id = body_buf[16..32].to_vec();

                // Extract read offset and length
                let offset = u64::from_le_bytes(body_buf[8..16].try_into().unwrap());
                let length = u32::from_le_bytes(body_buf[4..8].try_into().unwrap());

                // Look up file path from handle
                let path = {
                    let s = _state.lock().await;
                    s.files.get(&file_id).map(|h| h.path.clone())
                };

                let path = path.unwrap_or_else(|| "/unknown".to_string());
                info!("SMB2 READ: {} (offset={}, length={})", path, offset, length);
                let _ = status_tx.send(format!(
                    "[INFO] SMB READ: {} offset={} len={}",
                    path, offset, length
                ));

                // Consult LLM for file content
                let actions = Self::consult_llm(
                    _llm_client,
                    _app_state,
                    _server_id,
                    _protocol,
                    "read",
                    serde_json::json!({
                        "path": path,
                        "offset": offset,
                        "length": length
                    }),
                    status_tx,
                )
                .await?;

                // Extract file content from LLM response
                let content = actions
                    .iter()
                    .find(|a| a.get("type").and_then(|t| t.as_str()) == Some("smb_read_file"))
                    .and_then(|a| a.get("content"))
                    .and_then(|c| c.as_str())
                    .unwrap_or("File not found or empty")
                    .as_bytes()
                    .to_vec();

                debug!("SMB2 READ: returning {} bytes for {}", content.len(), path);
                let response = Self::build_read_response(_header, &content)?;
                Ok(Some(response))
            }
            SMB2_WRITE => {
                debug!("SMB2 WRITE request");

                // Read WRITE request body (49 bytes + data)
                let mut body_buf = vec![0u8; 49];
                _stream.read_exact(&mut body_buf).await?;

                // Extract file ID (16 bytes at offset 16)
                let file_id = body_buf[16..32].to_vec();

                // Extract write offset and length
                let offset = u64::from_le_bytes(body_buf[8..16].try_into().unwrap());
                let length = u32::from_le_bytes(body_buf[0..4].try_into().unwrap());

                // Read data to write (variable length)
                let mut data = vec![0u8; length as usize];
                _stream.read_exact(&mut data).await?;

                // Look up file path from handle
                let path = {
                    let s = _state.lock().await;
                    s.files.get(&file_id).map(|h| h.path.clone())
                };

                let path = path.unwrap_or_else(|| "/unknown".to_string());
                info!(
                    "SMB2 WRITE: {} (offset={}, length={})",
                    path, offset, length
                );
                let _ = status_tx.send(format!(
                    "[INFO] SMB WRITE: {} offset={} len={}",
                    path, offset, length
                ));

                // Convert data to string for LLM (assuming text files)
                let content = String::from_utf8_lossy(&data).to_string();

                // Consult LLM to store file content
                let _actions = Self::consult_llm(
                    _llm_client,
                    _app_state,
                    _server_id,
                    _protocol,
                    "write",
                    serde_json::json!({
                        "path": path,
                        "offset": offset,
                        "data": content
                    }),
                    status_tx,
                )
                .await?;

                debug!("SMB2 WRITE: wrote {} bytes to {}", length, path);
                let response = Self::build_write_response(_header, length)?;
                Ok(Some(response))
            }
            SMB2_QUERY_INFO => {
                debug!("SMB2 QUERY_INFO request");

                // Read QUERY_INFO request body (variable length)
                let mut body_buf = vec![0u8; 256];
                let bytes_read = _stream.read(&mut body_buf).await?;

                // Extract file ID (16 bytes at offset 16)
                if bytes_read >= 32 {
                    let file_id = body_buf[16..32].to_vec();

                    // Look up file path
                    let path = {
                        let s = _state.lock().await;
                        s.files.get(&file_id).map(|h| h.path.clone())
                    };

                    let path = path.unwrap_or_else(|| "/unknown".to_string());
                    info!("SMB2 QUERY_INFO: {}", path);
                    let _ = status_tx.send(format!("[INFO] SMB QUERY_INFO: {}", path));

                    // Consult LLM for file info
                    let actions = Self::consult_llm(
                        _llm_client,
                        _app_state,
                        _server_id,
                        _protocol,
                        "query_info",
                        serde_json::json!({
                            "path": path
                        }),
                        status_tx,
                    )
                    .await?;

                    // Extract file info from LLM response (or use defaults)
                    let size = actions
                        .iter()
                        .find(|a| {
                            a.get("type").and_then(|t| t.as_str()) == Some("smb_get_file_info")
                        })
                        .and_then(|a| a.get("size"))
                        .and_then(|s| s.as_u64())
                        .unwrap_or(4096);

                    let response = Self::build_query_info_response(_header, size)?;
                    Ok(Some(response))
                } else {
                    warn!("SMB2 QUERY_INFO: invalid request size");
                    Ok(None)
                }
            }
            SMB2_QUERY_DIRECTORY => {
                debug!("SMB2 QUERY_DIRECTORY request");

                // Read QUERY_DIRECTORY request body (variable length)
                let mut body_buf = vec![0u8; 512];
                let bytes_read = _stream.read(&mut body_buf).await?;

                // Extract file ID (directory handle, 16 bytes at offset 8)
                if bytes_read >= 24 {
                    let file_id = body_buf[8..24].to_vec();

                    // Look up directory path
                    let path = {
                        let s = _state.lock().await;
                        s.files.get(&file_id).map(|h| h.path.clone())
                    };

                    let path = path.unwrap_or_else(|| "/".to_string());
                    info!("SMB2 QUERY_DIRECTORY: {}", path);
                    let _ = status_tx.send(format!("[INFO] SMB QUERY_DIRECTORY: {}", path));

                    // Consult LLM for directory listing
                    let actions = Self::consult_llm(
                        _llm_client,
                        _app_state,
                        _server_id,
                        _protocol,
                        "query_directory",
                        serde_json::json!({
                            "path": path
                        }),
                        status_tx,
                    )
                    .await?;

                    // Extract file list from LLM response
                    let files = actions
                        .iter()
                        .find(|a| {
                            a.get("type").and_then(|t| t.as_str()) == Some("smb_list_directory")
                        })
                        .and_then(|a| a.get("files"))
                        .and_then(|f| f.as_array())
                        .cloned()
                        .unwrap_or_default();

                    debug!("SMB2 QUERY_DIRECTORY: returning {} files", files.len());
                    let response = Self::build_query_directory_response(_header, &files)?;
                    Ok(Some(response))
                } else {
                    warn!("SMB2 QUERY_DIRECTORY: invalid request size");
                    Ok(None)
                }
            }
            _ => {
                console_warn!(status_tx, "Unknown SMB2 command: 0x{:04x}", command);
                Ok(None)
            }
        }
    }

    /// Consult the LLM for SMB file system operations
    #[cfg(feature = "smb")]
    async fn consult_llm(
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        server_id: ServerId,
        protocol: &Arc<SmbProtocol>,
        operation: &str,
        params: serde_json::Value,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<Vec<serde_json::Value>> {
        debug!("Consulting LLM for SMB {} operation", operation);
        let _ = status_tx.send(format!("[DEBUG] SMB {}: {:?}", operation, params));

        // Create SMB operation event
        // Extract path from params if available, otherwise use empty string
        let path = params.get("path").and_then(|p| p.as_str()).unwrap_or("");

        let mut event_data = serde_json::json!({
            "operation": operation,
        });

        // Add path if it's not empty
        if !path.is_empty() {
            event_data["path"] = serde_json::json!(path);
        }

        // Add all params as additional fields for the LLM context
        if let Some(obj) = params.as_object() {
            for (key, value) in obj {
                if key != "operation" && key != "path" {
                    event_data[key] = value.clone();
                }
            }
        }

        let event = Event::new(&SMB_OPERATION_EVENT, event_data);

        trace!("Calling LLM for SMB {} operation", operation);
        let _ = status_tx.send(format!("[TRACE] Calling LLM for SMB {}", operation));

        // Call LLM with Event-based approach
        let execution_result = call_llm(
            llm_client,
            app_state,
            server_id,
            None, // SMB doesn't use connection-specific context yet
            &event,
            protocol.as_ref(),
        )
        .await?;

        // Display messages from LLM
        for message in &execution_result.messages {
            console_info!(status_tx, "{}", message);
        }

        debug!(
            "LLM returned {} actions for SMB {}",
            execution_result.raw_actions.len(),
            operation
        );

        // Return raw actions for manual processing
        Ok(execution_result.raw_actions)
    }

    /// Build SMB2 Negotiate Response
    /// Simplified implementation - offers SMB 2.1 dialect (0x0210)
    #[cfg(feature = "smb")]
    fn build_negotiate_response(request_header: &[u8]) -> Result<Vec<u8>> {
        let mut response = Vec::new();

        // SMB2 Header (64 bytes)
        response.extend_from_slice(b"\xFESMB"); // Protocol ID
        response.extend_from_slice(&[64, 0]); // Structure size (64 bytes)
        response.extend_from_slice(&[0, 0]); // Credit charge
        response.extend_from_slice(&[0, 0, 0, 0]); // Status (STATUS_SUCCESS)
        response.extend_from_slice(&[0x00, 0x00]); // Command (NEGOTIATE)
        response.extend_from_slice(&[1, 0]); // Credit (grant 1 credit)
        response.extend_from_slice(&[0, 0, 0, 0]); // Flags

        // Copy message ID from request (offset 24-31)
        response.extend_from_slice(&request_header[24..32]);

        response.extend_from_slice(&[0; 8]); // Reserved (process ID)
        response.extend_from_slice(&[0; 8]); // Tree ID
        response.extend_from_slice(&[0; 16]); // Session ID + Signature

        // SMB2 Negotiate Response body
        response.extend_from_slice(&[65, 0]); // Structure size (65 bytes)
        response.extend_from_slice(&[0, 0]); // Security mode
        response.extend_from_slice(&[0x10, 0x02]); // Dialect revision (SMB 2.1 = 0x0210)
        response.extend_from_slice(&[0, 0]); // Negotiate context count

        // Server GUID (16 bytes) - fixed for simplicity
        response.extend_from_slice(&[
            0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67, 0x89, 0xAB,
            0xCD, 0xEF,
        ]);

        response.extend_from_slice(&[0x07, 0x00, 0x00, 0x00]); // Capabilities (DFS)
        response.extend_from_slice(&[0x00, 0x00, 0x10, 0x00]); // Max transaction size
        response.extend_from_slice(&[0x00, 0x00, 0x10, 0x00]); // Max read size
        response.extend_from_slice(&[0x00, 0x00, 0x10, 0x00]); // Max write size

        // System time (current time in Windows FILETIME format)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        let filetime = (now / 100) + 116444736000000000; // Convert to FILETIME
        response.extend_from_slice(&filetime.to_le_bytes());

        response.extend_from_slice(&filetime.to_le_bytes()); // Server start time (same)
        response.extend_from_slice(&[0; 2]); // Security buffer offset (0 = no security)
        response.extend_from_slice(&[0; 2]); // Security buffer length

        response.extend_from_slice(&[0; 4]); // Negotiate context offset

        Ok(response)
    }

    /// Build SMB2 Session Setup Response (Guest)
    /// Accepts any session setup as guest
    #[cfg(feature = "smb")]
    fn _build_session_setup_response(
        request_header: &[u8],
        state: &Arc<Mutex<SmbConnectionState>>,
    ) -> Result<Vec<u8>> {
        let mut response = Vec::new();

        // Allocate session ID
        let session_id = {
            let mut s = state.blocking_lock();
            let sid = s.next_session_id;
            s.next_session_id += 1;

            // Create guest session
            s.sessions.insert(
                sid,
                SmbSession {
                    session_id: sid,
                    username: "guest".to_string(),
                    _authenticated: true,
                },
            );
            sid
        };

        // SMB2 Header
        response.extend_from_slice(b"\xFESMB");
        response.extend_from_slice(&[64, 0]);
        response.extend_from_slice(&[0, 0]);
        response.extend_from_slice(&[0, 0, 0, 0]); // STATUS_SUCCESS
        response.extend_from_slice(&[0x01, 0x00]); // Command (SESSION_SETUP)
        response.extend_from_slice(&[1, 0]);

        response.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Flags (response)

        // Copy message ID
        response.extend_from_slice(&request_header[24..32]);

        response.extend_from_slice(&[0; 8]);
        response.extend_from_slice(&[0; 4]);
        response.extend_from_slice(&session_id.to_le_bytes()); // Session ID
        response.extend_from_slice(&[0; 16]); // Signature

        // Session Setup Response body
        response.extend_from_slice(&[9, 0]); // Structure size
        response.extend_from_slice(&[0, 0]); // Session flags (guest)
        response.extend_from_slice(&[0, 0]); // Security buffer offset
        response.extend_from_slice(&[0, 0]); // Security buffer length

        Ok(response)
    }

    /// Build SMB2 Tree Connect Response
    /// Accepts all tree connects
    #[cfg(feature = "smb")]
    fn build_tree_connect_response(
        request_header: &[u8],
        state: &Arc<Mutex<SmbConnectionState>>,
        share_name: String,
    ) -> Result<Vec<u8>> {
        let mut response = Vec::new();

        // Allocate tree ID
        let tree_id = {
            let mut s = state.blocking_lock();
            let tid = s.next_tree_id;
            s.next_tree_id += 1;

            s.trees.insert(
                tid,
                SmbTreeConnect {
                    _tree_id: tid,
                    _share_name: share_name,
                },
            );
            tid
        };

        // SMB2 Header
        response.extend_from_slice(b"\xFESMB");
        response.extend_from_slice(&[64, 0]);
        response.extend_from_slice(&[0, 0]);
        response.extend_from_slice(&[0, 0, 0, 0]); // STATUS_SUCCESS
        response.extend_from_slice(&[0x03, 0x00]); // Command (TREE_CONNECT)
        response.extend_from_slice(&[1, 0]);
        response.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Flags

        // Copy message ID
        response.extend_from_slice(&request_header[24..32]);

        response.extend_from_slice(&[0; 8]);
        response.extend_from_slice(&tree_id.to_le_bytes()); // Tree ID
        response.extend_from_slice(&[0; 8]); // Session ID (should copy from request)
        response.extend_from_slice(&[0; 16]); // Signature

        // Tree Connect Response body
        response.extend_from_slice(&[16, 0]); // Structure size
        response.extend_from_slice(&[1]); // Share type (disk)
        response.extend_from_slice(&[0]); // Reserved
        response.extend_from_slice(&[0; 4]); // Share flags
        response.extend_from_slice(&[0; 4]); // Capabilities
        response.extend_from_slice(&[0x01, 0xF0, 0x1F, 0x00]); // Max access rights

        Ok(response)
    }

    /// Parse SMB2 file path from CREATE request
    /// Simplified parser - looks for UTF-16LE encoded path
    #[cfg(feature = "smb")]
    fn parse_smb2_path(body: &[u8]) -> Option<String> {
        // SMB2 CREATE request structure:
        // Offset 120+ contains the file name as UTF-16LE
        if body.len() < 120 {
            return None;
        }

        // Find the path - it's UTF-16LE after the fixed header
        // Look for null-terminated UTF-16LE string
        let mut path_bytes = Vec::new();
        let mut i = 120;
        while i + 1 < body.len() {
            let char_bytes = [body[i], body[i + 1]];
            if char_bytes == [0, 0] {
                break; // Null terminator
            }
            path_bytes.extend_from_slice(&char_bytes);
            i += 2;
        }

        // Convert UTF-16LE to String
        let utf16_chars: Vec<u16> = path_bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();

        String::from_utf16(&utf16_chars).ok()
    }

    /// Generate a 16-byte file handle (GUID)
    #[cfg(feature = "smb")]
    fn generate_file_handle() -> Vec<u8> {
        use std::time::SystemTime;

        // Simple file handle generation using timestamp + random-ish data
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        let mut handle = Vec::with_capacity(16);
        handle.extend_from_slice(&now.to_le_bytes());
        handle.extend_from_slice(&(now.wrapping_mul(0x123456789ABCDEF)).to_le_bytes());
        handle
    }

    /// Build SMB2 CREATE Response
    #[cfg(feature = "smb")]
    fn build_create_response(request_header: &[u8], file_id: &[u8]) -> Result<Vec<u8>> {
        let mut response = Vec::new();

        // SMB2 Header
        response.extend_from_slice(b"\xFESMB");
        response.extend_from_slice(&[64, 0]);
        response.extend_from_slice(&[0, 0]);
        response.extend_from_slice(&[0, 0, 0, 0]); // STATUS_SUCCESS
        response.extend_from_slice(&[0x05, 0x00]); // Command (CREATE)
        response.extend_from_slice(&[1, 0]);
        response.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Flags (response)

        // Copy message ID from request
        response.extend_from_slice(&request_header[24..32]);

        // Copy tree ID and session ID from request (should parse properly)
        response.extend_from_slice(&[0; 8]); // Reserved
        response.extend_from_slice(&[1, 0, 0, 0]); // Tree ID
        response.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0]); // Session ID
        response.extend_from_slice(&[0; 16]); // Signature

        // CREATE Response body (89 bytes)
        response.extend_from_slice(&[89, 0]); // Structure size
        response.extend_from_slice(&[0]); // Oplock level (none)
        response.extend_from_slice(&[0]); // Flags
        response.extend_from_slice(&[0, 0, 0, 0]); // Create action (file opened)

        // Timestamps (all zeros for simplicity)
        response.extend_from_slice(&[0; 8]); // Creation time
        response.extend_from_slice(&[0; 8]); // Last access time
        response.extend_from_slice(&[0; 8]); // Last write time
        response.extend_from_slice(&[0; 8]); // Change time

        response.extend_from_slice(&[0; 8]); // Allocation size
        response.extend_from_slice(&[0, 0x10, 0, 0, 0, 0, 0, 0]); // End of file (4096 bytes)
        response.extend_from_slice(&[0x80, 0, 0, 0]); // File attributes (normal)

        response.extend_from_slice(&[0; 4]); // Reserved

        // File ID (16 bytes - our handle)
        response.extend_from_slice(file_id);

        response.extend_from_slice(&[0; 4]); // Create contexts offset
        response.extend_from_slice(&[0; 4]); // Create contexts length

        Ok(response)
    }

    /// Build SMB2 CLOSE Response
    #[cfg(feature = "smb")]
    fn build_close_response(request_header: &[u8]) -> Result<Vec<u8>> {
        let mut response = Vec::new();

        // SMB2 Header
        response.extend_from_slice(b"\xFESMB");
        response.extend_from_slice(&[64, 0]);
        response.extend_from_slice(&[0, 0]);
        response.extend_from_slice(&[0, 0, 0, 0]); // STATUS_SUCCESS
        response.extend_from_slice(&[0x06, 0x00]); // Command (CLOSE)
        response.extend_from_slice(&[1, 0]);
        response.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Flags

        // Copy message ID
        response.extend_from_slice(&request_header[24..32]);

        response.extend_from_slice(&[0; 8]);
        response.extend_from_slice(&[1, 0, 0, 0]); // Tree ID
        response.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0]); // Session ID
        response.extend_from_slice(&[0; 16]);

        // CLOSE Response body (60 bytes)
        response.extend_from_slice(&[60, 0]); // Structure size
        response.extend_from_slice(&[0, 0]); // Flags
        response.extend_from_slice(&[0; 4]); // Reserved

        // Timestamps (all zeros)
        response.extend_from_slice(&[0; 8]); // Creation time
        response.extend_from_slice(&[0; 8]); // Last access
        response.extend_from_slice(&[0; 8]); // Last write
        response.extend_from_slice(&[0; 8]); // Change time

        response.extend_from_slice(&[0; 8]); // Allocation size
        response.extend_from_slice(&[0; 8]); // End of file
        response.extend_from_slice(&[0; 4]); // File attributes

        Ok(response)
    }

    /// Build SMB2 READ Response
    #[cfg(feature = "smb")]
    fn build_read_response(request_header: &[u8], data: &[u8]) -> Result<Vec<u8>> {
        let mut response = Vec::new();

        // SMB2 Header
        response.extend_from_slice(b"\xFESMB");
        response.extend_from_slice(&[64, 0]);
        response.extend_from_slice(&[0, 0]);
        response.extend_from_slice(&[0, 0, 0, 0]); // STATUS_SUCCESS
        response.extend_from_slice(&[0x08, 0x00]); // Command (READ)
        response.extend_from_slice(&[1, 0]);
        response.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Flags

        // Copy message ID
        response.extend_from_slice(&request_header[24..32]);

        response.extend_from_slice(&[0; 8]);
        response.extend_from_slice(&[1, 0, 0, 0]); // Tree ID
        response.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0]); // Session ID
        response.extend_from_slice(&[0; 16]);

        // READ Response body (17 bytes + data)
        response.extend_from_slice(&[17, 0]); // Structure size
        response.extend_from_slice(&[0x50, 0]); // Data offset (80 bytes from start)
        response.extend_from_slice(&[0; 4]); // Reserved
        let data_len = data.len() as u32;
        response.extend_from_slice(&data_len.to_le_bytes()); // Data length
        response.extend_from_slice(&[0; 4]); // Data remaining
        response.extend_from_slice(&[0; 4]); // Reserved

        // Data (variable length)
        response.extend_from_slice(data);

        Ok(response)
    }

    /// Build SMB2 WRITE Response
    #[cfg(feature = "smb")]
    fn build_write_response(request_header: &[u8], bytes_written: u32) -> Result<Vec<u8>> {
        let mut response = Vec::new();

        // SMB2 Header
        response.extend_from_slice(b"\xFESMB");
        response.extend_from_slice(&[64, 0]);
        response.extend_from_slice(&[0, 0]);
        response.extend_from_slice(&[0, 0, 0, 0]); // STATUS_SUCCESS
        response.extend_from_slice(&[0x09, 0x00]); // Command (WRITE)
        response.extend_from_slice(&[1, 0]);
        response.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Flags

        // Copy message ID
        response.extend_from_slice(&request_header[24..32]);

        response.extend_from_slice(&[0; 8]);
        response.extend_from_slice(&[1, 0, 0, 0]); // Tree ID
        response.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0]); // Session ID
        response.extend_from_slice(&[0; 16]);

        // WRITE Response body (17 bytes)
        response.extend_from_slice(&[17, 0]); // Structure size
        response.extend_from_slice(&[0, 0]); // Reserved
        response.extend_from_slice(&bytes_written.to_le_bytes()); // Count (bytes written)
        response.extend_from_slice(&[0; 4]); // Remaining
        response.extend_from_slice(&[0, 0]); // Write channel info offset
        response.extend_from_slice(&[0, 0]); // Write channel info length

        Ok(response)
    }

    /// Build SMB2 QUERY_INFO Response
    #[cfg(feature = "smb")]
    fn build_query_info_response(request_header: &[u8], file_size: u64) -> Result<Vec<u8>> {
        let mut response = Vec::new();

        // SMB2 Header
        response.extend_from_slice(b"\xFESMB");
        response.extend_from_slice(&[64, 0]);
        response.extend_from_slice(&[0, 0]);
        response.extend_from_slice(&[0, 0, 0, 0]); // STATUS_SUCCESS
        response.extend_from_slice(&[0x10, 0x00]); // Command (QUERY_INFO)
        response.extend_from_slice(&[1, 0]);
        response.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Flags

        // Copy message ID
        response.extend_from_slice(&request_header[24..32]);

        response.extend_from_slice(&[0; 8]);
        response.extend_from_slice(&[1, 0, 0, 0]); // Tree ID
        response.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0]); // Session ID
        response.extend_from_slice(&[0; 16]);

        // QUERY_INFO Response body (9 bytes + data)
        response.extend_from_slice(&[9, 0]); // Structure size
        response.extend_from_slice(&[0x48, 0]); // Output buffer offset (72 bytes from start)
        let info_size = 96u32; // FILE_ALL_INFORMATION size
        response.extend_from_slice(&info_size.to_le_bytes()); // Output buffer length

        // FILE_ALL_INFORMATION structure (simplified)
        // Creation time, access time, write time, change time (all zeros)
        response.extend_from_slice(&[0; 32]);
        // File attributes (normal file)
        response.extend_from_slice(&[0x80, 0, 0, 0]);
        // Reserved
        response.extend_from_slice(&[0; 4]);
        // Allocation size
        response.extend_from_slice(&file_size.to_le_bytes());
        // End of file (actual size)
        response.extend_from_slice(&file_size.to_le_bytes());
        // Number of links
        response.extend_from_slice(&[1, 0, 0, 0]);
        // Delete pending
        response.extend_from_slice(&[0]);
        // Is directory
        response.extend_from_slice(&[0]);
        // Reserved
        response.extend_from_slice(&[0; 2]);
        // File name length and name (empty for now)
        response.extend_from_slice(&[0; 44]); // Padding to reach 96 bytes

        Ok(response)
    }

    /// Build SMB2 QUERY_DIRECTORY Response
    #[cfg(feature = "smb")]
    fn build_query_directory_response(
        request_header: &[u8],
        files: &[serde_json::Value],
    ) -> Result<Vec<u8>> {
        let mut response = Vec::new();

        // SMB2 Header
        response.extend_from_slice(b"\xFESMB");
        response.extend_from_slice(&[64, 0]);
        response.extend_from_slice(&[0, 0]);
        response.extend_from_slice(&[0, 0, 0, 0]); // STATUS_SUCCESS
        response.extend_from_slice(&[0x0E, 0x00]); // Command (QUERY_DIRECTORY)
        response.extend_from_slice(&[1, 0]);
        response.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Flags

        // Copy message ID
        response.extend_from_slice(&request_header[24..32]);

        response.extend_from_slice(&[0; 8]);
        response.extend_from_slice(&[1, 0, 0, 0]); // Tree ID
        response.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0]); // Session ID
        response.extend_from_slice(&[0; 16]);

        // Build directory entries (simplified - just returns file names)
        let mut entries = Vec::new();

        for file in files {
            let name = file
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown.txt");
            let size = file.get("size").and_then(|s| s.as_u64()).unwrap_or(0);
            let is_dir = file
                .get("is_directory")
                .and_then(|d| d.as_bool())
                .unwrap_or(false);

            // FILE_DIRECTORY_INFORMATION entry
            let mut entry = Vec::new();
            entry.extend_from_slice(&[0; 4]); // Next entry offset (0 = last)
            entry.extend_from_slice(&[0; 4]); // File index
            entry.extend_from_slice(&[0; 32]); // Timestamps
            entry.extend_from_slice(&size.to_le_bytes()); // End of file
            entry.extend_from_slice(&size.to_le_bytes()); // Allocation size

            // File attributes
            let attrs = if is_dir { 0x10u32 } else { 0x80u32 };
            entry.extend_from_slice(&attrs.to_le_bytes());

            // File name (UTF-16LE)
            let name_utf16: Vec<u16> = name.encode_utf16().collect();
            let name_bytes = (name_utf16.len() * 2) as u32;
            entry.extend_from_slice(&name_bytes.to_le_bytes());

            // Convert UTF-16 to bytes
            for ch in name_utf16 {
                entry.extend_from_slice(&ch.to_le_bytes());
            }

            entries.extend_from_slice(&entry);
        }

        // QUERY_DIRECTORY Response body (9 bytes + entries)
        response.extend_from_slice(&[9, 0]); // Structure size
        response.extend_from_slice(&[0x48, 0]); // Output buffer offset
        let entries_len = entries.len() as u32;
        response.extend_from_slice(&entries_len.to_le_bytes()); // Output buffer length

        // Directory entries
        response.extend_from_slice(&entries);

        Ok(response)
    }

    /// Parse username from SMB2 SESSION_SETUP request (simplified)
    /// In real SMB2, username is in NTLMSSP blob. This is a simplified version.
    #[cfg(feature = "smb")]
    fn parse_smb2_username(body: &[u8]) -> Option<String> {
        // Look for printable ASCII username in the body
        // This is a simplified approach - real SMB2 would parse NTLMSSP
        if body.len() < 24 {
            return None;
        }

        // Try to find ASCII username (basic heuristic)
        let mut username_bytes = Vec::new();
        for &b in body.iter().take(body.len().min(200)).skip(24) {
            if (32..=126).contains(&b) {
                username_bytes.push(b);
            } else if !username_bytes.is_empty() {
                break;
            }
        }

        if username_bytes.len() >= 3 {
            String::from_utf8(username_bytes).ok()
        } else {
            None
        }
    }

    /// Build SMB2 ACCESS_DENIED response
    #[cfg(feature = "smb")]
    fn build_auth_denied_response(request_header: &[u8]) -> Result<Vec<u8>> {
        let mut response = Vec::new();

        // SMB2 Header with ACCESS_DENIED status
        response.extend_from_slice(b"\xFESMB");
        response.extend_from_slice(&[64, 0]); // Header length
        response.extend_from_slice(&[0, 0]); // Credit charge
        response.extend_from_slice(&[0x16, 0x00, 0x00, 0xC0]); // STATUS_ACCESS_DENIED (0xC0000016)
        response.extend_from_slice(&[0x01, 0x00]); // Command (SESSION_SETUP)
        response.extend_from_slice(&[0, 0]); // Credits

        response.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Flags (response)

        // Copy message ID from request
        response.extend_from_slice(&request_header[24..32]);

        response.extend_from_slice(&[0; 8]); // Reserved
        response.extend_from_slice(&[0; 4]); // Tree ID
        response.extend_from_slice(&[0; 8]); // Session ID (0 = denied)
        response.extend_from_slice(&[0; 16]); // Signature

        // Minimal Session Setup Response body (9 bytes for error)
        response.extend_from_slice(&[9, 0]); // Structure size
        response.extend_from_slice(&[0; 2]); // Session flags
        response.extend_from_slice(&[0; 2]); // Security buffer offset
        response.extend_from_slice(&[0; 2]); // Security buffer length
        response.extend_from_slice(&[0]); // Padding

        Ok(response)
    }

    /// Build SESSION_SETUP response with specific username
    #[cfg(feature = "smb")]
    fn build_session_setup_response_with_user(
        request_header: &[u8],
        state: &Arc<Mutex<SmbConnectionState>>,
        username: String,
    ) -> Result<Vec<u8>> {
        let mut response = Vec::new();

        // Allocate session ID
        let session_id = {
            let mut s = state.blocking_lock();
            let sid = s.next_session_id;
            s.next_session_id += 1;

            // Create session with specified username
            s.sessions.insert(
                sid,
                SmbSession {
                    session_id: sid,
                    username: username.clone(),
                    _authenticated: true,
                },
            );
            sid
        };

        // SMB2 Header
        response.extend_from_slice(b"\xFESMB");
        response.extend_from_slice(&[64, 0]);
        response.extend_from_slice(&[0, 0]);
        response.extend_from_slice(&[0, 0, 0, 0]); // STATUS_SUCCESS
        response.extend_from_slice(&[0x01, 0x00]); // Command (SESSION_SETUP)
        response.extend_from_slice(&[1, 0]);

        response.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Flags (response)

        // Copy message ID
        response.extend_from_slice(&request_header[24..32]);

        response.extend_from_slice(&[0; 8]);
        response.extend_from_slice(&[0; 4]);
        response.extend_from_slice(&session_id.to_le_bytes()); // Session ID
        response.extend_from_slice(&[0; 16]); // Signature

        // Session Setup Response body
        response.extend_from_slice(&[9, 0]); // Structure size
        response.extend_from_slice(&[0x01, 0x00]); // Session flags (logged in)
        response.extend_from_slice(&[0; 2]); // Security buffer offset
        response.extend_from_slice(&[0; 2]); // Security buffer length
        response.extend_from_slice(&[0]); // Padding

        Ok(response)
    }
}
