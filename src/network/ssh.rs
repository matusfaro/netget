//! SSH server implementation using russh

use crate::llm::ollama_client::OllamaClient;
use crate::llm::{call_llm_with_actions, ActionResult};
use crate::network::connection::ConnectionId;
use crate::network::ssh_actions::{
    ssh_auth_decision_action, ssh_close_connection_action, ssh_send_banner_action,
    ssh_shell_response_action, SshProtocol,
};
use crate::state::app_state::AppState;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use russh::server::{Auth, Msg, Server as RusshServer, Session};
use russh::{Channel, ChannelId, CryptoVec};
use russh_keys::key::KeyPair;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};

/// SSH server configuration
#[derive(Clone, Debug)]
pub struct SshServerConfig {
    /// Enable shell channel support
    pub shell_enabled: bool,
    /// Enable SFTP subsystem support
    pub sftp_enabled: bool,
}

impl Default for SshServerConfig {
    fn default() -> Self {
        Self {
            shell_enabled: true,
            sftp_enabled: true,
        }
    }
}

/// SSH server implementation
pub struct SshServer {
    config: SshServerConfig,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    server_id: Option<crate::state::ServerId>,
}

impl SshServer {
    /// Create a new SSH server
    pub fn new(
        config: SshServerConfig,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: Option<crate::state::ServerId>,
    ) -> Self {
        Self {
            config,
            llm_client,
            app_state,
            status_tx,
            server_id,
        }
    }

    /// Spawn SSH server with LLM integration
    pub async fn spawn_with_llm(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<SocketAddr> {
        let config = SshServerConfig::default();
        Self::spawn_with_config(listen_addr, config, llm_client, app_state, status_tx, None).await
    }

    /// Spawn SSH server with custom configuration
    pub async fn spawn_with_config(
        listen_addr: SocketAddr,
        config: SshServerConfig,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: Option<crate::state::ServerId>,
    ) -> Result<SocketAddr> {
        // Generate host key
        let key_pair = generate_host_key()?;

        let mut server = SshServer::new(config, llm_client, app_state.clone(), status_tx.clone(), server_id);
        let russh_config = russh::server::Config {
            inactivity_timeout: Some(std::time::Duration::from_secs(3600)),
            auth_rejection_time: std::time::Duration::from_secs(3),
            auth_rejection_time_initial: Some(std::time::Duration::from_secs(0)),
            keys: vec![key_pair],
            ..Default::default()
        };

        let russh_config = Arc::new(russh_config);

        info!("SSH server starting on {} (shell: {}, sftp: {})",
            listen_addr, server.config.shell_enabled, server.config.sftp_enabled);

        // Start the russh server
        tokio::spawn(async move {
            if let Err(e) = server.run_on_address(russh_config, listen_addr).await {
                error!("SSH server error: {}", e);
            }
        });

        Ok(listen_addr)
    }

    /// Spawn SSH server with action-based LLM integration
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        _send_first: bool,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let config = SshServerConfig::default();
        Self::spawn_with_config(listen_addr, config, llm_client, app_state, status_tx, Some(server_id)).await
    }
}

/// Generate a host key for the SSH server
fn generate_host_key() -> Result<KeyPair> {
    // Generate an Ed25519 key pair
    let key = KeyPair::generate_ed25519()
        .ok_or_else(|| anyhow!("Failed to generate Ed25519 key"))?;
    Ok(key)
}

/// SSH session handler
pub struct SshHandler {
    connection_id: ConnectionId,
    config: SshServerConfig,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    #[allow(dead_code)] // Used for connection tracking in new_client, not in handler methods
    server_id: Option<crate::state::ServerId>,
    #[allow(dead_code)] // Stored for future use (e.g., logging peer address in errors)
    remote_addr: Option<SocketAddr>,
    /// SSH protocol handler for action execution
    protocol: Arc<SshProtocol>,
    /// Active channels and their types
    channel_types: Arc<Mutex<HashMap<ChannelId, ChannelType>>>,
    /// Active channel objects (for SFTP)
    channels: Arc<Mutex<HashMap<ChannelId, Channel<Msg>>>>,
    /// Input buffers for shell channels (accumulate until newline)
    shell_buffers: Arc<Mutex<HashMap<ChannelId, Vec<u8>>>>,
    /// Track if we've sent initial data for each channel (for banner vs empty enter)
    channel_initialized: Arc<Mutex<HashMap<ChannelId, bool>>>,
}

/// Type of SSH channel
#[derive(Debug, Clone)]
enum ChannelType {
    Session,
    Sftp,
}

impl SshHandler {
    fn new(
        connection_id: ConnectionId,
        config: SshServerConfig,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: Option<crate::state::ServerId>,
        remote_addr: Option<SocketAddr>,
    ) -> Self {
        Self {
            connection_id,
            config,
            llm_client,
            app_state,
            status_tx,
            server_id,
            remote_addr,
            protocol: Arc::new(SshProtocol::new()),
            channel_types: Arc::new(Mutex::new(HashMap::new())),
            channels: Arc::new(Mutex::new(HashMap::new())),
            shell_buffers: Arc::new(Mutex::new(HashMap::new())),
            channel_initialized: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Convert line endings to SSH/terminal format (\r\n)
    /// This is required for proper display in SSH terminals.
    ///
    /// Handles both Unix (\n) and Windows (\r\n) line endings by:
    /// 1. First normalizing to Unix (\n) - removes any existing \r
    /// 2. Then converting all \n to \r\n
    ///
    /// This ensures consistent output regardless of what the LLM generates.
    fn normalize_line_endings(text: &str) -> String {
        // First normalize to Unix line endings, then convert to SSH format
        // This prevents double \r\r\n if LLM already sends \r\n
        text.replace("\r\n", "\n").replace('\n', "\r\n")
    }

    /// Get a channel object from the internal storage
    async fn get_channel(&mut self, channel_id: ChannelId) -> Option<Channel<Msg>> {
        self.channels.lock().await.remove(&channel_id)
    }

    /// Ask LLM about authentication using action-based framework
    async fn llm_auth_decision(&self, username: &str, auth_type: &str) -> Result<bool> {
        let server_id = self.server_id.unwrap_or_else(|| crate::state::ServerId::new(1));
        let event_description = format!(
            "SSH authentication request: user='{}', type='{}'",
            username, auth_type
        );

        debug!("SSH auth request for user '{}' via {}", username, auth_type);

        // Use action helper with custom SSH auth action and protocol
        let custom_action = ssh_auth_decision_action(username, auth_type);

        match call_llm_with_actions(
            &self.llm_client,
            &self.app_state,
            server_id,
            &event_description,
            Some(self.protocol.as_ref()),
            vec![custom_action],
        )
        .await
        {
            Ok(result) => {
                // Look for Custom result with auth decision
                for protocol_result in result.protocol_results {
                    if let ActionResult::Custom { name, data } = protocol_result {
                        if name == "ssh_auth_decision" {
                            if let Some(allowed) = data.get("allowed").and_then(|v| v.as_bool()) {
                                info!(
                                    "SSH auth decision for '{}': {}",
                                    username,
                                    if allowed { "allowed" } else { "denied" }
                                );
                                let _ = self.status_tx.send(format!(
                                    "SSH auth {}: {}",
                                    username,
                                    if allowed { "✓" } else { "✗" }
                                ));
                                return Ok(allowed);
                            }
                        }
                    }
                }

                // If no auth decision found, deny by default
                error!("SSH auth: LLM did not return auth decision, denying by default");
                Ok(false)
            }
            Err(e) => {
                error!("LLM error during SSH auth: {}", e);
                // Default to deny on error
                Ok(false)
            }
        }
    }

    /// Ask LLM for shell banner/greeting using action-based framework
    async fn llm_shell_banner(&self) -> Result<Option<String>> {
        let server_id = self.server_id.unwrap_or_else(|| crate::state::ServerId::new(1));
        let event_description = "SSH shell session opened - send banner/greeting if needed";

        debug!("SSH requesting shell banner from LLM");

        // Use action helper with custom SSH banner action and protocol
        let custom_action = ssh_send_banner_action();

        match call_llm_with_actions(
            &self.llm_client,
            &self.app_state,
            server_id,
            event_description,
            Some(self.protocol.as_ref()),
            vec![custom_action],
        )
        .await
        {
            Ok(result) => {
                // Look for Output result with banner data
                for protocol_result in result.protocol_results {
                    if let ActionResult::Output(data) = protocol_result {
                        let banner = String::from_utf8_lossy(&data).to_string();
                        // Convert \n to \r\n for proper SSH terminal display
                        let normalized = Self::normalize_line_endings(&banner);
                        debug!("SSH banner received: {} bytes", normalized.len());
                        return Ok(Some(normalized));
                    }
                }

                // No banner in results
                debug!("SSH: No banner returned by LLM");
                Ok(None)
            }
            Err(e) => {
                error!("LLM error getting shell banner: {}", e);
                Ok(None)
            }
        }
    }

    /// Ask LLM to handle shell command using action-based framework
    /// Returns (output, close_connection)
    async fn llm_shell_command(&self, command: &[u8]) -> Result<(Option<String>, bool)> {
        let server_id = self.server_id.unwrap_or_else(|| crate::state::ServerId::new(1));
        let command_str = String::from_utf8_lossy(command);
        let event_description = format!("SSH shell command received: {:?}", command_str);

        debug!("SSH shell command: {:?}", command_str);
        trace!("SSH shell command (full): {}", command_str);

        // Use action helper with custom SSH shell response action and protocol
        let custom_actions = vec![
            ssh_shell_response_action(&command_str),
            ssh_close_connection_action(),
        ];

        match call_llm_with_actions(
            &self.llm_client,
            &self.app_state,
            server_id,
            &event_description,
            Some(self.protocol.as_ref()),
            custom_actions,
        )
        .await
        {
            Ok(result) => {
                let mut output: Option<String> = None;
                let mut close_connection = false;

                // Process all protocol results
                for protocol_result in result.protocol_results {
                    match protocol_result {
                        ActionResult::Output(data) => {
                            let text = String::from_utf8_lossy(&data).to_string();
                            // Convert \n to \r\n for proper SSH terminal display
                            output = Some(Self::normalize_line_endings(&text));
                        }
                        ActionResult::CloseConnection => {
                            close_connection = true;
                        }
                        _ => {}
                    }
                }

                debug!(
                    "SSH shell response: output={}, close={}",
                    output.is_some(),
                    close_connection
                );

                Ok((output, close_connection))
            }
            Err(e) => {
                error!("LLM error handling shell command: {}", e);
                Ok((None, false))
            }
        }
    }

}

impl RusshServer for SshServer {
    type Handler = SshHandler;

    fn new_client(&mut self, peer_addr: Option<std::net::SocketAddr>) -> Self::Handler {
        let connection_id = ConnectionId::new();
        let addr = peer_addr.unwrap_or_else(|| "0.0.0.0:0".parse().unwrap());

        info!("SSH connection {} from {}", connection_id, addr);
        let _ = self.status_tx.send(format!("SSH connection from {}", addr));

        // Track connection in server state if server_id is available
        if let Some(server_id) = self.server_id {
            use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
            let now = std::time::Instant::now();

            // Assume local address is the bind address (we don't have access to the actual socket here)
            let local_addr = "0.0.0.0:22".parse().unwrap(); // Placeholder

            let conn_state = ServerConnectionState {
                id: connection_id,
                remote_addr: addr,
                local_addr,
                bytes_sent: 0,
                bytes_received: 0,
                packets_sent: 0,
                packets_received: 0,
                last_activity: now,
                status: ConnectionStatus::Active,
                status_changed_at: now,
                protocol_info: ProtocolConnectionInfo::Ssh {
                    authenticated: false,
                    username: None,
                    channels: Vec::new(),
                },
            };

            let app_state = self.app_state.clone();
            let status_tx = self.status_tx.clone();

            // Spawn task to add connection (new_client is not async)
            tokio::spawn(async move {
                app_state.add_connection_to_server(server_id, conn_state).await;
                let _ = status_tx.send("__UPDATE_UI__".to_string());
            });
        }

        SshHandler::new(
            connection_id,
            self.config.clone(),
            self.llm_client.clone(),
            self.app_state.clone(),
            self.status_tx.clone(),
            self.server_id,
            peer_addr,
        )
    }
}

#[async_trait]
impl russh::server::Handler for SshHandler {
    type Error = anyhow::Error;

    async fn auth_publickey(
        &mut self,
        user: &str,
        _public_key: &russh_keys::key::PublicKey,
    ) -> Result<Auth, Self::Error> {
        // Ask LLM if this user should be allowed
        let allowed = self.llm_auth_decision(user, "publickey").await?;

        if allowed {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::Reject {
                proceed_with_methods: None,
            })
        }
    }

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        // Ask LLM if this user/password should be allowed
        let event_desc = format!("password (user='{}', password='{}')", user, password);
        let allowed = self.llm_auth_decision(user, &event_desc).await?;

        if allowed {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::Reject {
                proceed_with_methods: None,
            })
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        if !self.config.shell_enabled {
            debug!("SSH shell channel requested but shell is disabled");
            return Ok(false);
        }

        let channel_id = channel.id();
        self.channel_types.lock().await.insert(channel_id, ChannelType::Session);
        self.channels.lock().await.insert(channel_id, channel);

        debug!("SSH session channel {} opened", channel_id);
        Ok(true)
    }

    async fn subsystem_request(
        &mut self,
        channel_id: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        // DEBUG: SSH subsystem request summary
        debug!("SSH request: SUBSYSTEM channel={}, name={}", channel_id, name);
        let _ = self.status_tx.send(format!("[DEBUG] SSH request: SUBSYSTEM channel={}, name={}", channel_id, name));

        // TRACE: Full SSH subsystem request
        trace!("SSH SUBSYSTEM request: channel={}, name='{}', connection={}",
            channel_id, name, self.connection_id);
        let _ = self.status_tx.send(format!("[TRACE] SSH SUBSYSTEM request: channel={}, name='{}', connection={}",
            channel_id, name, self.connection_id));

        if name == "sftp" {
            if !self.config.sftp_enabled {
                error!("SFTP subsystem requested but SFTP is disabled");
                let _ = self.status_tx.send(format!("[ERROR] SFTP subsystem requested but SFTP is disabled"));

                debug!("SSH response: CHANNEL_FAILURE (SFTP disabled)");
                let _ = self.status_tx.send(format!("[DEBUG] SSH response: CHANNEL_FAILURE (SFTP disabled)"));

                session.channel_failure(channel_id);
                return Ok(());
            }

            self.channel_types.lock().await.insert(channel_id, ChannelType::Sftp);

            // INFO: Major lifecycle event
            info!("SSH SFTP subsystem started on channel {} (connection {})",
                channel_id, self.connection_id);
            let _ = self.status_tx.send(format!("→ SFTP subsystem started on channel {} (conn {})",
                channel_id, self.connection_id));

            // Get the channel object
            if let Some(channel) = self.get_channel(channel_id).await {
                debug!("SSH response: CHANNEL_SUCCESS (starting SFTP handler)");
                let _ = self.status_tx.send(format!("[DEBUG] SSH response: CHANNEL_SUCCESS (starting SFTP handler)"));

                trace!("Creating LlmSftpHandler for channel {} on connection {}",
                    channel_id, self.connection_id);
                let _ = self.status_tx.send(format!("[TRACE] Creating LlmSftpHandler for channel {} on connection {}",
                    channel_id, self.connection_id));

                session.channel_success(channel_id);

                // Create LLM-controlled SFTP handler
                let sftp_handler = crate::network::LlmSftpHandler::new(
                    self.connection_id,
                    self.llm_client.clone(),
                    self.app_state.clone(),
                    self.status_tx.clone(),
                );

                // Run SFTP protocol (this handles all packet parsing)
                trace!("Starting russh_sftp::server::run() for channel {}", channel_id);
                let _ = self.status_tx.send(format!("[TRACE] Starting russh_sftp::server::run() for channel {}", channel_id));

                russh_sftp::server::run(channel.into_stream(), sftp_handler).await;

                // INFO: SFTP session ended
                info!("SFTP session ended on channel {} (connection {})",
                    channel_id, self.connection_id);
                let _ = self.status_tx.send(format!("✗ SFTP session ended on channel {} (conn {})",
                    channel_id, self.connection_id));

                debug!("SSH: SFTP subsystem terminated on channel {}", channel_id);
                let _ = self.status_tx.send(format!("[DEBUG] SSH: SFTP subsystem terminated on channel {}", channel_id));
            } else {
                error!("SFTP channel {} not found (this should not happen)", channel_id);
                let _ = self.status_tx.send(format!("[ERROR] SFTP channel {} not found", channel_id));

                debug!("SSH response: CHANNEL_FAILURE (channel not found)");
                let _ = self.status_tx.send(format!("[DEBUG] SSH response: CHANNEL_FAILURE (channel not found)"));

                session.channel_failure(channel_id);
            }
        } else {
            error!("Unknown subsystem requested: '{}' on channel {}", name, channel_id);
            let _ = self.status_tx.send(format!("[ERROR] Unknown subsystem requested: '{}'", name));

            debug!("SSH response: CHANNEL_FAILURE (unknown subsystem '{}')", name);
            let _ = self.status_tx.send(format!("[DEBUG] SSH response: CHANNEL_FAILURE (unknown subsystem '{}')", name));

            trace!("SSH rejecting unknown subsystem: name='{}', channel={}, connection={}",
                name, channel_id, self.connection_id);
            let _ = self.status_tx.send(format!("[TRACE] SSH rejecting unknown subsystem: name='{}', channel={}, conn={}",
                name, channel_id, self.connection_id));

            session.channel_failure(channel_id);
        }

        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel_id: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        debug!("SSH shell request on channel {}", channel_id);

        if !self.config.shell_enabled {
            error!("Shell requested but shell is disabled");
            session.channel_failure(channel_id);
            return Ok(());
        }

        session.channel_success(channel_id);

        // Send banner/greeting via LLM
        if let Ok(Some(banner)) = self.llm_shell_banner().await {
            let data = CryptoVec::from_slice(banner.as_bytes());
            session.data(channel_id, data);
            debug!("Sent shell banner ({} bytes)", banner.len());
        }

        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel_id: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let command = String::from_utf8_lossy(data);
        debug!("SSH exec request on channel {}: {:?}", channel_id, command);

        if !self.config.shell_enabled {
            error!("Exec requested but shell is disabled");
            session.channel_failure(channel_id);
            return Ok(());
        }

        session.channel_success(channel_id);

        // Execute command via LLM
        if let Ok((output, _close)) = self.llm_shell_command(data).await {
            if let Some(output_text) = output {
                let data = CryptoVec::from_slice(output_text.as_bytes());
                session.data(channel_id, data);
                debug!("Sent exec output ({} bytes)", output_text.len());
            }
        }

        // Close channel after exec
        session.exit_status_request(channel_id, 0);
        session.eof(channel_id);
        session.close(channel_id);

        Ok(())
    }

    async fn data(
        &mut self,
        channel_id: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let channel_types = self.channel_types.lock().await;
        let channel_type = channel_types.get(&channel_id).cloned();
        drop(channel_types); // Release lock before async operations

        match channel_type {
            Some(ChannelType::Session) => {
                // Shell data - handle backspace, echo properly, and buffer until newline or Ctrl-C
                trace!("SSH shell data received on channel {}: hex={:02x?}", channel_id, data);
                let _ = self.status_tx.send(format!("[TRACE] SSH shell data received on channel {}: hex={:02x?}",
                    channel_id, data));

                // Get or create buffer for this channel
                let mut buffers = self.shell_buffers.lock().await;
                let buffer = buffers.entry(channel_id).or_insert_with(Vec::new);

                // Process each byte
                for &byte in data {
                    match byte {
                        // Backspace (0x7F) or Delete (0x08)
                        0x7F | 0x08 => {
                            if !buffer.is_empty() {
                                buffer.pop();
                                // Echo: backspace + space + backspace (to erase character on screen)
                                let erase = CryptoVec::from_slice(&[0x08, b' ', 0x08]);
                                session.data(channel_id, erase);
                                trace!("SSH shell: backspace, buffer now {} bytes", buffer.len());
                            }
                        }
                        // Tab (0x09) - echo but don't buffer (for tab completion)
                        0x09 => {
                            let echo = CryptoVec::from_slice(&[byte]);
                            session.data(channel_id, echo);
                            // Don't buffer tabs - they should be handled immediately by the client
                            // or used for tab completion which doesn't need buffering
                        }
                        // Newline characters (Enter key)
                        b'\n' | b'\r' => {
                            // Echo newline as \r\n (proper line ending for terminals)
                            let echo = CryptoVec::from_slice(b"\r\n");
                            session.data(channel_id, echo);
                            // Add actual received character to buffer
                            buffer.push(byte);
                        }
                        // Control characters (0x01-0x1F except tab/newline/carriage return)
                        0x01..=0x1F => {
                            // Echo control characters visually as "^X\r\n" (e.g., ^C, ^D, ^Z)
                            // Control character to printable: add 0x40 (e.g., 0x03 + 0x40 = 0x43 = 'C')
                            let ctrl_char = byte + 0x40;
                            let echo_str = format!("^{}\r\n", ctrl_char as char);
                            let echo = CryptoVec::from_slice(echo_str.as_bytes());
                            session.data(channel_id, echo);
                            // Add to buffer for LLM to see
                            buffer.push(byte);
                        }
                        // Printable characters (0x20-0x7E)
                        0x20..=0x7E => {
                            // Echo the character
                            let echo = CryptoVec::from_slice(&[byte]);
                            session.data(channel_id, echo);
                            // Add to buffer
                            buffer.push(byte);
                        }
                        // Other bytes (non-printable, non-control)
                        _ => {
                            // Just add to buffer without echo
                            buffer.push(byte);
                        }
                    }
                }

                // Check if we should process the buffer (Enter or control characters received)
                // NOTE: Echo has already happened above in the byte loop - LLM invocation comes AFTER echo
                // Process on: Enter (\r, \n) or any control character except Tab (0x09)
                let should_process = data.iter().any(|&b| {
                    b == b'\n' || b == b'\r' || (b >= 0x01 && b <= 0x1F && b != 0x09)
                });

                if should_process {
                    // Check if this is the first interaction (for banner) or empty input
                    let mut initialized = self.channel_initialized.lock().await;
                    let is_first_input = !initialized.get(&channel_id).copied().unwrap_or(false);
                    initialized.insert(channel_id, true);
                    drop(initialized);

                    let line = buffer.clone();
                    buffer.clear();
                    drop(buffers);

                    // Only call LLM if:
                    // 1. First input (even if empty) - for banner
                    // 2. Non-empty input (command to process)
                    // 3. Any control character present (Ctrl-C, Ctrl-D, etc.) - always process
                    // 4. Empty Enter - LLM should respond with prompt
                    let has_ctrl_c = line.iter().any(|&b| b == 0x03);
                    let has_any_ctrl = line.iter().any(|&b| b >= 0x01 && b <= 0x1F && b != 0x09);
                    let is_empty_cmd = line.iter().all(|&b| b == b'\n' || b == b'\r');

                    // Always process if we have any control character or non-empty command
                    if is_first_input || !is_empty_cmd || has_any_ctrl {
                        debug!("SSH shell processing input ({} bytes, first={}, empty={})",
                            line.len(), is_first_input, is_empty_cmd);
                        let _ = self.status_tx.send(format!("[DEBUG] SSH shell processing input ({} bytes, first={}, empty={})",
                            line.len(), is_first_input, is_empty_cmd));

                        trace!("SSH shell input (hex): {:02x?}", line);
                        let _ = self.status_tx.send(format!("[TRACE] SSH shell input (hex): {:02x?}", line));

                        trace!("SSH shell input (text): {:?}", String::from_utf8_lossy(&line));
                        let _ = self.status_tx.send(format!("[TRACE] SSH shell input (text): {:?}", String::from_utf8_lossy(&line)));

                        // Build context string for LLM
                        let mut context_parts = Vec::new();
                        if is_first_input {
                            context_parts.push("FIRST_INPUT - send banner/greeting if appropriate");
                        }
                        if has_ctrl_c {
                            context_parts.push("CTRL_C - user interrupted");
                        }
                        // Check for other common control characters
                        if line.iter().any(|&b| b == 0x04) {
                            context_parts.push("CTRL_D - EOF signal");
                        }
                        if line.iter().any(|&b| b == 0x1A) {
                            context_parts.push("CTRL_Z - suspend signal");
                        }
                        if is_empty_cmd && !is_first_input {
                            context_parts.push("EMPTY_ENTER - just show prompt");
                        }
                        let context = if context_parts.is_empty() {
                            String::new()
                        } else {
                            format!(" [{}]", context_parts.join(", "))
                        };

                        if let Ok((output, close_connection)) = self.llm_shell_command(&line).await {
                            // Send output if present
                            if let Some(output_text) = output {
                                let response = CryptoVec::from_slice(output_text.as_bytes());
                                session.data(channel_id, response);

                                debug!("Sent shell response ({} bytes){}", output_text.len(), context);
                                let _ = self.status_tx.send(format!("[DEBUG] Sent shell response ({} bytes){}", output_text.len(), context));
                                let _ = self.status_tx.send(format!("→ Sent shell response to channel {}", channel_id));
                            }

                            // Handle close_connection flag (e.g., from Ctrl-C)
                            if close_connection {
                                info!("LLM requested shell connection close on channel {}", channel_id);
                                let _ = self.status_tx.send(format!("✗ Closing shell (LLM request) on channel {}", channel_id));

                                session.exit_status_request(channel_id, 0);
                                session.eof(channel_id);
                                session.close(channel_id);
                            } else {
                                // Send a prompt after the response so user knows where to type next
                                // This prevents commands from being echoed on the same line as output
                                let prompt = CryptoVec::from_slice(b"$ ");
                                session.data(channel_id, prompt);
                            }
                        }
                    } else {
                        // Empty Enter press after initialization - ignore it
                        trace!("SSH shell: ignoring empty Enter (already initialized)");
                        let _ = self.status_tx.send(format!("[TRACE] SSH shell: ignoring empty Enter"));
                    }
                } else {
                    // Still accumulating input
                    trace!("SSH shell buffering: {} bytes total", buffer.len());
                    let _ = self.status_tx.send(format!("[TRACE] SSH shell buffering: {} bytes total", buffer.len()));
                }
            }
            Some(ChannelType::Sftp) => {
                // SFTP data is handled by russh_sftp::server::run() in subsystem_request()
                // This case shouldn't normally be reached
                debug!("SFTP data received on channel {} - should be handled by SFTP subsystem", channel_id);
            }
            None => {
                debug!("Data received on unknown channel {}", channel_id);
            }
        }

        Ok(())
    }

    async fn channel_eof(
        &mut self,
        channel_id: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        debug!("SSH channel {} EOF", channel_id);
        session.close(channel_id);
        Ok(())
    }

    async fn channel_close(
        &mut self,
        channel_id: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        debug!("SSH channel {} closed", channel_id);
        self.channel_types.lock().await.remove(&channel_id);
        self.channels.lock().await.remove(&channel_id);
        self.shell_buffers.lock().await.remove(&channel_id);
        self.channel_initialized.lock().await.remove(&channel_id);
        Ok(())
    }
}

/// Get LLM context and output format instructions for SSH stack
pub fn get_llm_protocol_prompt() -> (&'static str, &'static str) {
    let context = r#"You are handling the SSH (Secure Shell) protocol, which provides secure remote access and file transfer.

IMPORTANT: SSH protocol includes TWO main capabilities:
1. Shell sessions - Interactive command-line access (like terminal/console)
2. SFTP subsystem - Secure File Transfer Protocol for file operations

=== SSH PROTOCOL OPERATIONS ===

1. AUTHENTICATION (decides who can connect):
   - Password authentication: user provides password
   - Public key authentication: user provides SSH key
   - Decide whether to allow/deny each user
   - Respond with JSON: {"allowed": true} or {"allowed": false, "message": "reason"}

2. SHELL SESSIONS (interactive terminal access):
   - Provide welcome banner when user first connects
   - Show command prompt (e.g., "$ " or "username@host:~$ ")
   - Execute user commands and return output
   - Simulate a Linux/Unix-like shell environment
   - Handle commands like: ls, pwd, cat, echo, cd, etc.

   CRITICAL: ALWAYS use \r\n (CRLF) for line endings, NOT just \n (LF)
   SSH terminals require carriage return (\r) to move cursor to beginning of line

   Special Input Handling:
   - Ctrl-C (control character \u{3} or 0x03) - Interrupt signal
     * The system will echo "^C" visually to the user
     * You will see "CTRL_C" in the context flags
     * You decide how to respond (typically show a new prompt, or exit if appropriate)
   - Other control characters are handled and echoed appropriately

   Example interaction:
   User connects → Show banner "Welcome to SSH Server\r\nLast login: Mon Jan 1 12:00:00 2024\r\n$ "
   User types "ls" → Return "file1.txt\r\nfile2.txt\r\n"
   User types "pwd" → Return "/home/user\r\n"
   User presses Enter (empty) → Return "$ " (just show prompt again)
   User presses Ctrl-C → System echoes "^C\r\n", you can respond with new prompt "$ " or handle as needed
   User presses Ctrl-D → You can close connection or show message, your choice

3. SFTP SUBSYSTEM (file transfer operations):
   - SFTP runs as a subsystem within SSH
   - Users can upload, download, list, delete files
   - Define a virtual filesystem structure
   - Handle operations: read file, write file, list directory, create/delete

   Example SFTP operations:
   List directory "/" → Return: {"entries": ["home", "etc", "var"]}
   Read file "readme.txt" → Return: {"content": "Hello from SFTP!\n"}
   File not found → Return: {"error": "No such file"}

NOTE: SSH and SFTP are the SAME protocol - SFTP is a subsystem that runs over SSH.
When handling SSH, you may receive both shell commands AND file transfer requests."#;

    let output_format = r#"IMPORTANT: Response format depends on the operation type:

=== AUTHENTICATION RESPONSES ===
{
  "allowed": true,
  "message": "User admin authenticated successfully"
}

OR

{
  "allowed": false,
  "message": "Invalid credentials"
}

=== SHELL SESSION RESPONSES ===
Plain text output or JSON:

{
  "output": "Welcome to SSH Server\r\nLast login: Mon Jan 1 12:00:00 2024\r\n$ ",
  "close_connection": false
}

OR just plain text (ALWAYS use \r\n for line endings):
"total 4\r\ndrwxr-xr-x 2 user user 4096 Jan 1 12:00 Desktop\r\n"

=== SFTP RESPONSES ===
For file reads:
{
  "content": "This is the file contents.\nLine 2 of the file."
}

For directory listings:
{
  "entries": ["file1.txt", "file2.pdf", "subfolder/"]
}

For errors:
{
  "error": "Permission denied"
}

For success operations (delete, create, write):
{
  "success": true
}

REMEMBER: SSH protocol = Shell access + SFTP file transfer in one protocol"#;

    (context, output_format)
}
