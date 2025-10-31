//! SOCKS5 proxy server implementation with LLM control
//!
//! This module implements a SOCKS5 proxy server with:
//! - SOCKS5 protocol handshake and authentication
//! - LLM-controlled connection decisions (allow/deny)
//! - Pattern-based filtering for selective LLM involvement
//! - Optional MITM mode for traffic inspection
//! - Support for IPv4, IPv6, and domain name targets

pub mod filter;
pub mod actions;

use crate::server::connection::ConnectionId;
use filter::{Socks5FilterConfig, FilterMode};
use anyhow::{Result, Context, bail};
use std::net::{SocketAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn, trace};

use crate::llm::action_helper::call_llm;
use crate::llm::actions::protocol_trait::ActionResult;
use crate::llm::ollama_client::OllamaClient;
use actions::{SOCKS5_AUTH_REQUEST_EVENT, SOCKS5_CONNECT_REQUEST_EVENT};
use crate::server::Socks5Protocol;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::ServerId;

/// SOCKS5 protocol constants
const SOCKS5_VERSION: u8 = 0x05;
const AUTH_METHOD_NO_AUTH: u8 = 0x00;
const AUTH_METHOD_USERNAME_PASSWORD: u8 = 0x02;
const AUTH_METHOD_NO_ACCEPTABLE: u8 = 0xFF;

const CMD_CONNECT: u8 = 0x01;
const ATYP_IPV4: u8 = 0x01;
const ATYP_DOMAIN: u8 = 0x03;
const ATYP_IPV6: u8 = 0x04;

const REPLY_SUCCESS: u8 = 0x00;
const _REPLY_GENERAL_FAILURE: u8 = 0x01;
const REPLY_CONNECTION_NOT_ALLOWED: u8 = 0x02;
const _REPLY_NETWORK_UNREACHABLE: u8 = 0x03;
const _REPLY_HOST_UNREACHABLE: u8 = 0x04;
const _REPLY_CONNECTION_REFUSED: u8 = 0x05;
const _REPLY_COMMAND_NOT_SUPPORTED: u8 = 0x07;
const _REPLY_ADDRESS_TYPE_NOT_SUPPORTED: u8 = 0x08;

/// Target address for SOCKS5 connection
#[derive(Debug, Clone)]
pub enum TargetAddr {
    Ipv4(Ipv4Addr, u16),
    Ipv6(Ipv6Addr, u16),
    Domain(String, u16),
}

impl std::fmt::Display for TargetAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TargetAddr::Ipv4(ip, port) => write!(f, "{}:{}", ip, port),
            TargetAddr::Ipv6(ip, port) => write!(f, "[{}]:{}", ip, port),
            TargetAddr::Domain(domain, port) => write!(f, "{}:{}", domain, port),
        }
    }
}

/// SOCKS5 proxy server that forwards connections via LLM decisions
pub struct Socks5Server;

impl Socks5Server {
    /// Spawn SOCKS5 proxy server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: ServerId,
        startup_params: Option<serde_json::Value>,
    ) -> Result<SocketAddr> {
        info!("SOCKS5 proxy server (action-based) starting on {}", listen_addr);
        let _ = status_tx.send(format!("[INFO] SOCKS5 starting on {}", listen_addr));

        // Get or initialize SOCKS5 filter configuration
        let mut config = app_state.get_socks5_filter_config(server_id).await
            .unwrap_or_else(|| {
                info!("No SOCKS5 filter config found, using defaults");
                Socks5FilterConfig::default()
            });

        // Apply startup parameters if provided
        if let Some(params) = startup_params {
            info!("Applying startup parameters: {:?}", params);
            let _ = status_tx.send("[INFO] Applying SOCKS5 startup parameters".to_string());

            // Parse auth methods
            if let Some(methods) = params.get("auth_methods").and_then(|v| v.as_array()) {
                config.auth_methods.clear();
                for method in methods {
                    if let Some(method_str) = method.as_str() {
                        match method_str {
                            "none" => config.auth_methods.push(AUTH_METHOD_NO_AUTH),
                            "username_password" => config.auth_methods.push(AUTH_METHOD_USERNAME_PASSWORD),
                            _ => warn!("Unknown auth method: {}", method_str),
                        }
                    }
                }
                let _ = status_tx.send(format!("[INFO] Auth methods: {:?}", config.auth_methods));
            }

            // Parse default action
            if let Some(action_str) = params.get("default_action").and_then(|v| v.as_str()) {
                config.default_action = action_str.to_string();
                let _ = status_tx.send(format!("[INFO] Default action: {}", config.default_action));
            }

            // Parse filter configuration
            if let Some(filter) = params.get("filter") {
                if let Some(patterns) = filter.get("target_host_patterns").and_then(|v| v.as_array()) {
                    config.target_host_patterns = patterns.iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| s.to_string())
                        .collect();
                }
                if let Some(ranges) = filter.get("target_port_ranges").and_then(|v| v.as_array()) {
                    config.target_port_ranges = ranges.iter()
                        .filter_map(|v| v.as_array())
                        .filter_map(|arr| {
                            if arr.len() == 2 {
                                let start = arr[0].as_u64()? as u16;
                                let end = arr[1].as_u64()? as u16;
                                Some((start, end))
                            } else {
                                None
                            }
                        })
                        .collect();
                }
            }

            // Parse filter mode
            if let Some(mode_str) = params.get("filter_mode").and_then(|v| v.as_str()) {
                config.filter_mode = match mode_str {
                    "allow_all" => FilterMode::AllowAll,
                    "deny_all" => FilterMode::DenyAll,
                    "ask_llm" => FilterMode::AskLlm,
                    "selective" => FilterMode::Selective,
                    _ => {
                        warn!("Unknown filter mode: {}, using default", mode_str);
                        config.filter_mode
                    }
                };
                let _ = status_tx.send(format!("[INFO] Filter mode: {:?}", config.filter_mode));
            }
        }

        // Store config in app state
        app_state.set_socks5_filter_config(server_id, config.clone()).await;

        let listener = crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("SOCKS5 proxy server listening on {}", local_addr);
        let _ = status_tx.send(format!("→ SOCKS5 proxy ready on {}", local_addr));

        let protocol = Arc::new(Socks5Protocol::new());

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = ConnectionId::new();
                        let local_addr_conn = stream.local_addr().unwrap_or(local_addr);
                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();
                        let config_clone = config.clone();

                        tokio::spawn(async move {
                            info!("SOCKS5 connection {} from {}", connection_id, remote_addr);
                            let _ = status_clone.send(format!("[INFO] SOCKS5 connection {} from {}", connection_id, remote_addr));

                            if let Err(e) = Self::handle_connection(
                                stream,
                                connection_id,
                                remote_addr,
                                local_addr_conn,
                                llm_clone,
                                state_clone.clone(),
                                status_clone.clone(),
                                protocol_clone,
                                server_id,
                                config_clone,
                            ).await {
                                error!("SOCKS5 connection {} error: {}", connection_id, e);
                                let _ = status_clone.send(format!("✗ SOCKS5 connection {} error: {}", connection_id, e));
                            }

                            // Connection closed - mark as closed
                            state_clone.close_connection_on_server(server_id, connection_id).await;
                            let _ = status_clone.send("__UPDATE_UI__".to_string());
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept SOCKS5 connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Handle individual SOCKS5 connection
    async fn handle_connection(
        mut client_stream: TcpStream,
        connection_id: ConnectionId,
        remote_addr: SocketAddr,
        local_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<Socks5Protocol>,
        server_id: ServerId,
        config: Socks5FilterConfig,
    ) -> Result<()> {
        // Add connection to ServerInstance
        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus, ProtocolState};
        let now = std::time::Instant::now();
        let conn_state = ServerConnectionState {
            id: connection_id,
            remote_addr,
            local_addr,
            bytes_sent: 0,
            bytes_received: 0,
            packets_sent: 0,
            packets_received: 0,
            last_activity: now,
            status: ConnectionStatus::Active,
            status_changed_at: now,
            protocol_info: ProtocolConnectionInfo::Socks5 {
                target_addr: None,
                username: None,
                mitm_enabled: false,
                state: ProtocolState::Idle,
                queued_data: Vec::new(),
            },
        };
        app_state.add_connection_to_server(server_id, conn_state).await;
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Phase 1: Handshake - negotiate auth method
        debug!("SOCKS5 {} phase 1: handshake", connection_id);
        let _ = status_tx.send(format!("[DEBUG] SOCKS5 {} phase 1: handshake", connection_id));

        let selected_method = Self::negotiate_auth(&mut client_stream, &config, connection_id, &status_tx).await?;

        debug!("SOCKS5 {} selected auth method: 0x{:02x}", connection_id, selected_method);
        let _ = status_tx.send(format!("[DEBUG] SOCKS5 {} selected auth method: 0x{:02x}", connection_id, selected_method));

        // Phase 2: Authentication (if required)
        let username = if selected_method == AUTH_METHOD_USERNAME_PASSWORD {
            debug!("SOCKS5 {} phase 2: authentication", connection_id);
            let _ = status_tx.send(format!("[DEBUG] SOCKS5 {} phase 2: authentication", connection_id));

            let auth_result = Self::authenticate_username_password(
                &mut client_stream,
                connection_id,
                &llm_client,
                &app_state,
                &status_tx,
                &protocol,
                server_id,
            ).await?;

            Some(auth_result)
        } else {
            None
        };

        // Phase 3: Process CONNECT request
        debug!("SOCKS5 {} phase 3: CONNECT request", connection_id);
        let _ = status_tx.send(format!("[DEBUG] SOCKS5 {} phase 3: CONNECT request", connection_id));

        let target_addr = Self::parse_connect_request(&mut client_stream, connection_id, &status_tx).await?;

        info!("SOCKS5 {} CONNECT to {}", connection_id, target_addr);
        let _ = status_tx.send(format!("[INFO] SOCKS5 {} CONNECT to {}", connection_id, target_addr));

        // Update connection with target address
        app_state.update_socks5_target(server_id, connection_id, Some(target_addr.to_string()), username.clone()).await;

        // Check if target matches filter
        let matches_filter = Self::check_filter_match(&target_addr, &config);

        debug!("SOCKS5 {} filter match: {}", connection_id, matches_filter);
        let _ = status_tx.send(format!("[DEBUG] SOCKS5 {} filter match: {}", connection_id, matches_filter));

        // Decide whether to ask LLM or use default action
        let (should_allow, mitm_enabled) = match (&config.filter_mode, matches_filter) {
            (FilterMode::AllowAll, _) => (true, config.mitm_by_default),
            (FilterMode::DenyAll, _) => (false, false),
            (FilterMode::Selective, true) | (FilterMode::AskLlm, _) => {
                // Ask LLM
                Self::ask_llm_for_decision(
                    &target_addr,
                    username.as_deref(),
                    connection_id,
                    &llm_client,
                    &app_state,
                    &status_tx,
                    &protocol,
                    server_id,
                ).await?
            }
            (FilterMode::Selective, false) => {
                // No filter match, use default action
                let allow = config.default_action == "allow";
                (allow, allow && config.mitm_by_default)
            }
        };

        if !should_allow {
            warn!("SOCKS5 {} connection denied by policy", connection_id);
            let _ = status_tx.send(format!("✗ SOCKS5 {} connection denied", connection_id));

            // Send SOCKS5 reply: connection not allowed
            Self::send_connect_reply(&mut client_stream, REPLY_CONNECTION_NOT_ALLOWED, &target_addr).await?;
            return Ok(());
        }

        // Connect to target
        let mut target_stream = Self::connect_to_target(&target_addr, connection_id, &status_tx).await?;

        info!("SOCKS5 {} connected to target {}", connection_id, target_addr);
        let _ = status_tx.send(format!("→ SOCKS5 {} connected to {}", connection_id, target_addr));

        // Send SOCKS5 reply: success
        Self::send_connect_reply(&mut client_stream, REPLY_SUCCESS, &target_addr).await?;

        // Phase 4: Relay data bidirectionally
        debug!("SOCKS5 {} phase 4: relay data (MITM: {})", connection_id, mitm_enabled);
        let _ = status_tx.send(format!("[DEBUG] SOCKS5 {} phase 4: relay (MITM: {})", connection_id, mitm_enabled));

        if mitm_enabled {
            // MITM mode: inspect and modify data
            Self::relay_with_mitm(
                client_stream,
                target_stream,
                connection_id,
                &target_addr,
                username.as_deref(),
                &llm_client,
                &app_state,
                &status_tx,
                &protocol,
                server_id,
            ).await?;
        } else {
            // Passthrough mode: direct relay
            match tokio::io::copy_bidirectional(&mut client_stream, &mut target_stream).await {
                Ok((client_to_target_bytes, target_to_client_bytes)) => {
                    info!("SOCKS5 {} relay complete: {} bytes to target, {} bytes from target",
                          connection_id, client_to_target_bytes, target_to_client_bytes);
                    let _ = status_tx.send(format!("[INFO] SOCKS5 {} relay complete: {}↑ {}↓",
                                                   connection_id, client_to_target_bytes, target_to_client_bytes));
                }
                Err(e) => {
                    warn!("SOCKS5 {} relay error: {}", connection_id, e);
                    let _ = status_tx.send(format!("[WARN] SOCKS5 {} relay error: {}", connection_id, e));
                }
            }
        }

        Ok(())
    }

    /// Relay with MITM inspection - asks LLM for each data chunk
    async fn relay_with_mitm(
        mut client_stream: TcpStream,
        mut target_stream: TcpStream,
        connection_id: ConnectionId,
        target_addr: &TargetAddr,
        username: Option<&str>,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: &Arc<Socks5Protocol>,
        server_id: ServerId,
    ) -> Result<()> {
        use actions::{SOCKS5_DATA_TO_TARGET_EVENT, SOCKS5_DATA_FROM_TARGET_EVENT};

        info!("SOCKS5 {} starting MITM relay", connection_id);
        let _ = status_tx.send(format!("[INFO] SOCKS5 {} MITM relay active", connection_id));

        let mut client_buf = vec![0u8; 8192];
        let mut target_buf = vec![0u8; 8192];
        let mut client_to_target_total = 0u64;
        let mut target_to_client_total = 0u64;

        loop {
            tokio::select! {
                // Read from client (data going to target)
                result = client_stream.read(&mut client_buf) => {
                    match result {
                        Ok(0) => {
                            debug!("SOCKS5 {} client closed connection", connection_id);
                            let _ = status_tx.send(format!("[DEBUG] SOCKS5 {} client closed", connection_id));
                            break;
                        }
                        Ok(n) => {
                            let data = &client_buf[..n];
                            trace!("SOCKS5 {} client→target {} bytes: {:?}", connection_id, n, data);
                            let _ = status_tx.send(format!("[TRACE] SOCKS5 {} client→target {} bytes", connection_id, n));

                            // Ask LLM what to do with this data
                            let data_str = String::from_utf8_lossy(data).to_string();
                            let event = Event::new(&SOCKS5_DATA_TO_TARGET_EVENT, serde_json::json!({
                                "data": data_str,
                                "target": target_addr.to_string(),
                                "username": username,
                            }));

                            let execution_result = call_llm(
                                llm_client,
                                app_state,
                                server_id,
                                Some(connection_id),
                                &event,
                                protocol.as_ref(),
                            ).await?;

                            // Process LLM actions
                            let mut should_close = false;
                            let mut data_to_send: Option<Vec<u8>> = Some(data.to_vec());

                            for result in &execution_result.protocol_results {
                                match result {
                                    ActionResult::NoAction => {
                                        // Forward as-is (already set)
                                    }
                                    ActionResult::Output(modified_data) => {
                                        // Use modified data
                                        data_to_send = Some(modified_data.clone());
                                        debug!("SOCKS5 {} LLM modified data ({} → {} bytes)",
                                               connection_id, n, modified_data.len());
                                        let _ = status_tx.send(format!("[DEBUG] SOCKS5 {} data modified: {} → {} bytes",
                                                                       connection_id, n, modified_data.len()));
                                    }
                                    ActionResult::CloseConnection => {
                                        should_close = true;
                                        data_to_send = None;
                                        warn!("SOCKS5 {} LLM requested close", connection_id);
                                        let _ = status_tx.send(format!("[WARN] SOCKS5 {} LLM close request", connection_id));
                                    }
                                    _ => {}
                                }
                            }

                            if should_close {
                                break;
                            }

                            // Send data to target
                            if let Some(data) = data_to_send {
                                target_stream.write_all(&data).await?;
                                target_stream.flush().await?;
                                client_to_target_total += data.len() as u64;
                            }
                        }
                        Err(e) => {
                            error!("SOCKS5 {} client read error: {}", connection_id, e);
                            let _ = status_tx.send(format!("[ERROR] SOCKS5 {} client read error: {}", connection_id, e));
                            break;
                        }
                    }
                }

                // Read from target (data going to client)
                result = target_stream.read(&mut target_buf) => {
                    match result {
                        Ok(0) => {
                            debug!("SOCKS5 {} target closed connection", connection_id);
                            let _ = status_tx.send(format!("[DEBUG] SOCKS5 {} target closed", connection_id));
                            break;
                        }
                        Ok(n) => {
                            let data = &target_buf[..n];
                            trace!("SOCKS5 {} target→client {} bytes: {:?}", connection_id, n, data);
                            let _ = status_tx.send(format!("[TRACE] SOCKS5 {} target→client {} bytes", connection_id, n));

                            // Ask LLM what to do with this data
                            let data_str = String::from_utf8_lossy(data).to_string();
                            let event = Event::new(&SOCKS5_DATA_FROM_TARGET_EVENT, serde_json::json!({
                                "data": data_str,
                                "target": target_addr.to_string(),
                                "username": username,
                            }));

                            let execution_result = call_llm(
                                llm_client,
                                app_state,
                                server_id,
                                Some(connection_id),
                                &event,
                                protocol.as_ref(),
                            ).await?;

                            // Process LLM actions
                            let mut should_close = false;
                            let mut data_to_send: Option<Vec<u8>> = Some(data.to_vec());

                            for result in &execution_result.protocol_results {
                                match result {
                                    ActionResult::NoAction => {
                                        // Forward as-is (already set)
                                    }
                                    ActionResult::Output(modified_data) => {
                                        // Use modified data
                                        data_to_send = Some(modified_data.clone());
                                        debug!("SOCKS5 {} LLM modified data ({} → {} bytes)",
                                               connection_id, n, modified_data.len());
                                        let _ = status_tx.send(format!("[DEBUG] SOCKS5 {} data modified: {} → {} bytes",
                                                                       connection_id, n, modified_data.len()));
                                    }
                                    ActionResult::CloseConnection => {
                                        should_close = true;
                                        data_to_send = None;
                                        warn!("SOCKS5 {} LLM requested close", connection_id);
                                        let _ = status_tx.send(format!("[WARN] SOCKS5 {} LLM close request", connection_id));
                                    }
                                    _ => {}
                                }
                            }

                            if should_close {
                                break;
                            }

                            // Send data to client
                            if let Some(data) = data_to_send {
                                client_stream.write_all(&data).await?;
                                client_stream.flush().await?;
                                target_to_client_total += data.len() as u64;
                            }
                        }
                        Err(e) => {
                            error!("SOCKS5 {} target read error: {}", connection_id, e);
                            let _ = status_tx.send(format!("[ERROR] SOCKS5 {} target read error: {}", connection_id, e));
                            break;
                        }
                    }
                }
            }
        }

        info!("SOCKS5 {} MITM relay complete: {}↑ {}↓",
              connection_id, client_to_target_total, target_to_client_total);
        let _ = status_tx.send(format!("[INFO] SOCKS5 {} MITM relay complete: {}↑ {}↓",
                                       connection_id, client_to_target_total, target_to_client_total));

        Ok(())
    }

    /// Negotiate authentication method with client
    async fn negotiate_auth(
        stream: &mut TcpStream,
        config: &Socks5FilterConfig,
        connection_id: ConnectionId,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<u8> {
        // Read handshake: [VER, NMETHODS, METHODS...]
        let mut buf = [0u8; 2];
        stream.read_exact(&mut buf).await?;

        let version = buf[0];
        let nmethods = buf[1];

        trace!("SOCKS5 {} handshake: version={}, nmethods={}", connection_id, version, nmethods);
        let _ = status_tx.send(format!("[TRACE] SOCKS5 {} handshake: version={}, nmethods={}", connection_id, version, nmethods));

        if version != SOCKS5_VERSION {
            bail!("Unsupported SOCKS version: {}", version);
        }

        if nmethods == 0 {
            bail!("No authentication methods provided");
        }

        // Read methods
        let mut methods = vec![0u8; nmethods as usize];
        stream.read_exact(&mut methods).await?;

        trace!("SOCKS5 {} client methods: {:?}", connection_id, methods);
        let _ = status_tx.send(format!("[TRACE] SOCKS5 {} client methods: {:?}", connection_id, methods));

        // Select method based on config
        let selected_method = config.auth_methods.iter()
            .find(|&&method| methods.contains(&method))
            .copied()
            .unwrap_or(AUTH_METHOD_NO_ACCEPTABLE);

        // Send method selection: [VER, METHOD]
        let response = [SOCKS5_VERSION, selected_method];
        stream.write_all(&response).await?;
        stream.flush().await?;

        if selected_method == AUTH_METHOD_NO_ACCEPTABLE {
            bail!("No acceptable authentication methods");
        }

        Ok(selected_method)
    }

    /// Authenticate using username/password
    async fn authenticate_username_password(
        stream: &mut TcpStream,
        connection_id: ConnectionId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: &Arc<Socks5Protocol>,
        server_id: ServerId,
    ) -> Result<String> {
        // Read auth request: [VER(1), ULEN, UNAME, PLEN, PASSWD]
        let mut buf = [0u8; 1];
        stream.read_exact(&mut buf).await?;

        let auth_version = buf[0];
        if auth_version != 0x01 {
            bail!("Unsupported username/password auth version: {}", auth_version);
        }

        // Read username
        stream.read_exact(&mut buf).await?;
        let ulen = buf[0] as usize;
        let mut username_bytes = vec![0u8; ulen];
        stream.read_exact(&mut username_bytes).await?;
        let username = String::from_utf8_lossy(&username_bytes).to_string();

        // Read password
        stream.read_exact(&mut buf).await?;
        let plen = buf[0] as usize;
        let mut password_bytes = vec![0u8; plen];
        stream.read_exact(&mut password_bytes).await?;
        let password = String::from_utf8_lossy(&password_bytes).to_string();

        debug!("SOCKS5 {} auth request: username={}", connection_id, username);
        let _ = status_tx.send(format!("[DEBUG] SOCKS5 {} auth request: username={}", connection_id, username));

        // Ask LLM to validate credentials
        let event = Event::new(&SOCKS5_AUTH_REQUEST_EVENT, serde_json::json!({
            "username": username,
            "password": password,
        }));

        let execution_result = call_llm(
            llm_client,
            app_state,
            server_id,
            Some(connection_id),
            &event,
            protocol.as_ref(),
        ).await?;

        // Check if LLM allowed auth
        let auth_allowed = execution_result.protocol_results.iter().any(|result| {
            matches!(result, ActionResult::NoAction)
        });

        // Send auth response: [VER(1), STATUS]
        let status = if auth_allowed { 0x00 } else { 0x01 };
        let response = [0x01, status];
        stream.write_all(&response).await?;
        stream.flush().await?;

        if !auth_allowed {
            bail!("Authentication failed for user: {}", username);
        }

        info!("SOCKS5 {} authenticated as {}", connection_id, username);
        let _ = status_tx.send(format!("→ SOCKS5 {} authenticated as {}", connection_id, username));

        Ok(username)
    }

    /// Parse CONNECT request from client
    async fn parse_connect_request(
        stream: &mut TcpStream,
        connection_id: ConnectionId,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<TargetAddr> {
        // Read request: [VER, CMD, RSV(0), ATYP, DST.ADDR, DST.PORT]
        let mut buf = [0u8; 4];
        stream.read_exact(&mut buf).await?;

        let version = buf[0];
        let cmd = buf[1];
        let _rsv = buf[2];
        let atyp = buf[3];

        trace!("SOCKS5 {} request: version={}, cmd={}, atyp={}", connection_id, version, cmd, atyp);
        let _ = status_tx.send(format!("[TRACE] SOCKS5 {} request: version={}, cmd=0x{:02x}, atyp=0x{:02x}", connection_id, version, cmd, atyp));

        if version != SOCKS5_VERSION {
            bail!("Unsupported SOCKS version: {}", version);
        }

        if cmd != CMD_CONNECT {
            bail!("Unsupported command: 0x{:02x} (only CONNECT supported)", cmd);
        }

        // Parse destination address
        let target_addr = match atyp {
            ATYP_IPV4 => {
                let mut addr = [0u8; 4];
                stream.read_exact(&mut addr).await?;
                let ip = Ipv4Addr::from(addr);
                let mut port_buf = [0u8; 2];
                stream.read_exact(&mut port_buf).await?;
                let port = u16::from_be_bytes(port_buf);
                TargetAddr::Ipv4(ip, port)
            }
            ATYP_DOMAIN => {
                let mut len_buf = [0u8; 1];
                stream.read_exact(&mut len_buf).await?;
                let len = len_buf[0] as usize;
                let mut domain_bytes = vec![0u8; len];
                stream.read_exact(&mut domain_bytes).await?;
                let domain = String::from_utf8_lossy(&domain_bytes).to_string();
                let mut port_buf = [0u8; 2];
                stream.read_exact(&mut port_buf).await?;
                let port = u16::from_be_bytes(port_buf);
                TargetAddr::Domain(domain, port)
            }
            ATYP_IPV6 => {
                let mut addr = [0u8; 16];
                stream.read_exact(&mut addr).await?;
                let ip = Ipv6Addr::from(addr);
                let mut port_buf = [0u8; 2];
                stream.read_exact(&mut port_buf).await?;
                let port = u16::from_be_bytes(port_buf);
                TargetAddr::Ipv6(ip, port)
            }
            _ => bail!("Unsupported address type: 0x{:02x}", atyp),
        };

        Ok(target_addr)
    }

    /// Send CONNECT reply to client
    async fn send_connect_reply(
        stream: &mut TcpStream,
        reply_code: u8,
        target_addr: &TargetAddr,
    ) -> Result<()> {
        // Build reply: [VER, REP, RSV(0), ATYP, BND.ADDR, BND.PORT]
        let mut response = vec![SOCKS5_VERSION, reply_code, 0x00];

        // Add bound address (use target address for simplicity)
        match target_addr {
            TargetAddr::Ipv4(ip, port) => {
                response.push(ATYP_IPV4);
                response.extend_from_slice(&ip.octets());
                response.extend_from_slice(&port.to_be_bytes());
            }
            TargetAddr::Ipv6(ip, port) => {
                response.push(ATYP_IPV6);
                response.extend_from_slice(&ip.octets());
                response.extend_from_slice(&port.to_be_bytes());
            }
            TargetAddr::Domain(domain, port) => {
                response.push(ATYP_DOMAIN);
                response.push(domain.len() as u8);
                response.extend_from_slice(domain.as_bytes());
                response.extend_from_slice(&port.to_be_bytes());
            }
        }

        stream.write_all(&response).await?;
        stream.flush().await?;

        Ok(())
    }

    /// Connect to target address
    async fn connect_to_target(
        target_addr: &TargetAddr,
        connection_id: ConnectionId,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<TcpStream> {
        let target_str = target_addr.to_string();

        debug!("SOCKS5 {} connecting to target: {}", connection_id, target_str);
        let _ = status_tx.send(format!("[DEBUG] SOCKS5 {} connecting to {}", connection_id, target_str));

        let stream = TcpStream::connect(&target_str).await
            .context(format!("Failed to connect to {}", target_str))?;

        Ok(stream)
    }

    /// Check if target matches filter patterns
    fn check_filter_match(target_addr: &TargetAddr, config: &Socks5FilterConfig) -> bool {
        let target_host = match target_addr {
            TargetAddr::Ipv4(ip, _) => ip.to_string(),
            TargetAddr::Ipv6(ip, _) => ip.to_string(),
            TargetAddr::Domain(domain, _) => domain.clone(),
        };

        let target_port = match target_addr {
            TargetAddr::Ipv4(_, port) => *port,
            TargetAddr::Ipv6(_, port) => *port,
            TargetAddr::Domain(_, port) => *port,
        };

        // Check host patterns
        let host_matches = if config.target_host_patterns.is_empty() {
            true
        } else {
            config.target_host_patterns.iter().any(|pattern| {
                if let Ok(re) = regex::Regex::new(pattern) {
                    re.is_match(&target_host)
                } else {
                    false
                }
            })
        };

        // Check port ranges
        let port_matches = if config.target_port_ranges.is_empty() {
            true
        } else {
            config.target_port_ranges.iter().any(|(start, end)| {
                target_port >= *start && target_port <= *end
            })
        };

        host_matches && port_matches
    }

    /// Ask LLM whether to allow connection (returns: allowed, mitm_enabled)
    async fn ask_llm_for_decision(
        target_addr: &TargetAddr,
        username: Option<&str>,
        connection_id: ConnectionId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: &Arc<Socks5Protocol>,
        server_id: ServerId,
    ) -> Result<(bool, bool)> {
        debug!("SOCKS5 {} asking LLM for decision", connection_id);
        let _ = status_tx.send(format!("[DEBUG] SOCKS5 {} asking LLM for decision", connection_id));

        let event = Event::new(&SOCKS5_CONNECT_REQUEST_EVENT, serde_json::json!({
            "target": target_addr.to_string(),
            "username": username,
        }));

        let execution_result = call_llm(
            llm_client,
            app_state,
            server_id,
            Some(connection_id),
            &event,
            protocol.as_ref(),
        ).await?;

        // Check if LLM allowed connection
        let allowed = execution_result.protocol_results.iter().any(|result| {
            matches!(result, ActionResult::NoAction)
        });

        // Extract MITM flag from actions if allowed
        let mitm_enabled = if allowed {
            execution_result.raw_actions.iter()
                .find(|action| {
                    action.get("type")
                        .and_then(|v| v.as_str())
                        .map(|s| s == "allow_socks5_connect")
                        .unwrap_or(false)
                })
                .and_then(|action| action.get("mitm"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        } else {
            false
        };

        debug!("SOCKS5 {} decision: allowed={}, mitm={}", connection_id, allowed, mitm_enabled);
        let _ = status_tx.send(format!("[DEBUG] SOCKS5 {} decision: allowed={}, mitm={}", connection_id, allowed, mitm_enabled));

        Ok((allowed, mitm_enabled))
    }
}
