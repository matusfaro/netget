//! SSH server implementation using russh

use crate::llm::ollama_client::OllamaClient;
use crate::llm::prompt::PromptBuilder;
use crate::network::connection::ConnectionId;
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
    ) -> Self {
        Self {
            config,
            llm_client,
            app_state,
            status_tx,
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
        Self::spawn_with_config(listen_addr, config, llm_client, app_state, status_tx).await
    }

    /// Spawn SSH server with custom configuration
    pub async fn spawn_with_config(
        listen_addr: SocketAddr,
        config: SshServerConfig,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<SocketAddr> {
        // Generate host key
        let key_pair = generate_host_key()?;

        let mut server = SshServer::new(config, llm_client, app_state.clone(), status_tx.clone());
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
        _server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        // For now, delegate to the regular spawn_with_llm
        // TODO: Implement full action-based integration with server tracking
        Self::spawn_with_llm(listen_addr, llm_client, app_state, status_tx).await
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
    /// Active channels and their types
    channel_types: Arc<Mutex<HashMap<ChannelId, ChannelType>>>,
    /// Active channel objects (for SFTP)
    channels: Arc<Mutex<HashMap<ChannelId, Channel<Msg>>>>,
    /// Input buffers for shell channels (accumulate until newline)
    shell_buffers: Arc<Mutex<HashMap<ChannelId, Vec<u8>>>>,
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
    ) -> Self {
        Self {
            connection_id,
            config,
            llm_client,
            app_state,
            status_tx,
            channel_types: Arc::new(Mutex::new(HashMap::new())),
            channels: Arc::new(Mutex::new(HashMap::new())),
            shell_buffers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get a channel object from the internal storage
    async fn get_channel(&mut self, channel_id: ChannelId) -> Option<Channel<Msg>> {
        self.channels.lock().await.remove(&channel_id)
    }

    /// Ask LLM about authentication
    async fn llm_auth_decision(&self, username: &str, auth_type: &str) -> Result<bool> {
        let model = self.app_state.get_ollama_model().await;
        let event_description = format!(
            "SSH authentication request: user='{}', type='{}'",
            username, auth_type
        );

        let prompt = PromptBuilder::build_network_event_prompt(
            &self.app_state,
            self.connection_id,
            &event_description,
            get_llm_protocol_prompt(),
        )
        .await;

        debug!("SSH auth request for user '{}' via {}", username, auth_type);

        match self.llm_client.generate(&model, &prompt).await {
            Ok(response) => {
                // Parse LLM response - look for "allow", "accept", "yes" etc.
                let lower = response.to_lowercase();
                let allowed = lower.contains("allow")
                    || lower.contains("accept")
                    || lower.contains("yes")
                    || lower.contains("\"status\": \"success\"")
                    || lower.contains("\"allowed\": true");

                info!("SSH auth decision for '{}': {}", username, if allowed { "allowed" } else { "denied" });
                let _ = self.status_tx.send(format!("SSH auth {}: {}", username, if allowed { "✓" } else { "✗" }));
                Ok(allowed)
            }
            Err(e) => {
                error!("LLM error during SSH auth: {}", e);
                // Default to deny on error
                Ok(false)
            }
        }
    }

    /// Ask LLM for shell banner/greeting
    async fn llm_shell_banner(&self) -> Result<Option<String>> {
        let model = self.app_state.get_ollama_model().await;
        let event_description = "SSH shell session opened - send banner/greeting if needed";

        let prompt = PromptBuilder::build_network_event_prompt(
            &self.app_state,
            self.connection_id,
            event_description,
            crate::network::ssh::get_llm_protocol_prompt(),
        )
        .await;

        match self.llm_client.generate(&model, &prompt).await {
            Ok(response) => {
                if response.trim().is_empty() || response.trim().eq_ignore_ascii_case("NO_RESPONSE") {
                    Ok(None)
                } else {
                    // Try to parse as JSON, fallback to raw text
                    if let Ok(parsed) = self.llm_client.generate_llm_response(&model, &prompt).await {
                        Ok(parsed.output.map(|s| s.to_string()))
                    } else {
                        Ok(Some(response))
                    }
                }
            }
            Err(e) => {
                error!("LLM error getting shell banner: {}", e);
                Ok(None)
            }
        }
    }

    /// Ask LLM to handle shell command
    async fn llm_shell_command(&self, command: &[u8]) -> Result<Option<Vec<u8>>> {
        let model = self.app_state.get_ollama_model().await;

        let command_str = String::from_utf8_lossy(command);
        let event_description = format!("SSH shell command received: {:?}", command_str);

        let prompt = PromptBuilder::build_network_event_prompt(
            &self.app_state,
            self.connection_id,
            &event_description,
            get_llm_protocol_prompt(),
        )
        .await;

        debug!("SSH shell command: {:?}", command_str);
        trace!("SSH shell command (full): {}", command_str);

        match self.llm_client.generate(&model, &prompt).await {
            Ok(response) => {
                if response.trim().is_empty() || response.trim().eq_ignore_ascii_case("NO_RESPONSE") {
                    Ok(None)
                } else {
                    // Try to parse as structured response
                    if let Ok(parsed) = self.llm_client.generate_llm_response(&model, &prompt).await {
                        Ok(parsed.output.map(|s| s.as_bytes().to_vec()))
                    } else {
                        Ok(Some(response.into_bytes()))
                    }
                }
            }
            Err(e) => {
                error!("LLM error handling shell command: {}", e);
                Ok(None)
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

        SshHandler::new(
            connection_id,
            self.config.clone(),
            self.llm_client.clone(),
            self.app_state.clone(),
            self.status_tx.clone(),
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

        // TRACE: Full SSH subsystem request
        trace!("SSH SUBSYSTEM request: channel={}, name='{}', connection={}",
            channel_id, name, self.connection_id);

        if name == "sftp" {
            if !self.config.sftp_enabled {
                error!("SFTP subsystem requested but SFTP is disabled");

                // DEBUG: SFTP rejection
                debug!("SSH response: CHANNEL_FAILURE (SFTP disabled)");

                session.channel_failure(channel_id);
                return Ok(());
            }

            self.channel_types.lock().await.insert(channel_id, ChannelType::Sftp);

            // INFO: Major lifecycle event
            info!("SSH SFTP subsystem started on channel {} (connection {})",
                channel_id, self.connection_id);

            // Get the channel object
            if let Some(channel) = self.get_channel(channel_id).await {
                // DEBUG: SFTP subsystem starting
                debug!("SSH response: CHANNEL_SUCCESS (starting SFTP handler)");

                // TRACE: SFTP handler creation
                trace!("Creating LlmSftpHandler for channel {} on connection {}",
                    channel_id, self.connection_id);

                session.channel_success(channel_id);

                // Create LLM-controlled SFTP handler
                let sftp_handler = crate::network::LlmSftpHandler::new(
                    self.connection_id,
                    self.llm_client.clone(),
                    self.app_state.clone(),
                    self.status_tx.clone(),
                );

                // Run SFTP protocol (this handles all packet parsing)
                // NOTE: This blocks until SFTP session ends
                trace!("Starting russh_sftp::server::run() for channel {}", channel_id);
                russh_sftp::server::run(channel.into_stream(), sftp_handler).await;

                // INFO: SFTP session lifecycle event
                info!("SFTP session ended on channel {} (connection {})",
                    channel_id, self.connection_id);

                // DEBUG: SFTP session ended
                debug!("SSH: SFTP subsystem terminated on channel {}", channel_id);
            } else {
                error!("SFTP channel {} not found (this should not happen)", channel_id);

                // DEBUG: Channel lookup failure
                debug!("SSH response: CHANNEL_FAILURE (channel not found)");

                session.channel_failure(channel_id);
            }
        } else {
            error!("Unknown subsystem requested: '{}' on channel {}", name, channel_id);

            // DEBUG: Unknown subsystem rejection
            debug!("SSH response: CHANNEL_FAILURE (unknown subsystem '{}')", name);

            // TRACE: Full rejection details
            trace!("SSH rejecting unknown subsystem: name='{}', channel={}, connection={}",
                name, channel_id, self.connection_id);

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
        if let Ok(Some(output)) = self.llm_shell_command(data).await {
            let data = CryptoVec::from_slice(&output);
            session.data(channel_id, data);
            debug!("Sent exec output ({} bytes)", output.len());
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
                // Shell data - buffer until newline
                trace!("SSH shell data received on channel {}: {:?}", channel_id, String::from_utf8_lossy(data));

                // Echo the input back to the client (so user sees what they're typing)
                let echo = CryptoVec::from_slice(data);
                session.data(channel_id, echo);

                // Get or create buffer for this channel
                let mut buffers = self.shell_buffers.lock().await;
                let buffer = buffers.entry(channel_id).or_insert_with(Vec::new);

                // Append new data to buffer
                buffer.extend_from_slice(data);

                // Check if buffer contains newline (Enter key)
                let has_newline = buffer.iter().any(|&b| b == b'\n' || b == b'\r');

                if has_newline {
                    // Extract the complete line
                    let line = buffer.clone();
                    buffer.clear(); // Clear buffer for next line
                    drop(buffers); // Release lock before async LLM call

                    // Process the complete line with LLM
                    debug!("SSH shell processing complete line ({} bytes)", line.len());
                    trace!("SSH shell complete line: {:?}", String::from_utf8_lossy(&line));

                    if let Ok(Some(output)) = self.llm_shell_command(&line).await {
                        let response = CryptoVec::from_slice(&output);
                        session.data(channel_id, response);
                        debug!("Sent shell response ({} bytes)", output.len());
                    }
                } else {
                    // Still accumulating input, no newline yet
                    trace!("SSH shell buffering input ({} bytes total)", buffer.len());
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

   Example interaction:
   User connects → Show banner "Welcome to SSH Server\r\n"
   User types "ls" → Return "file1.txt\nfile2.txt\n"
   User types "pwd" → Return "/home/user\n"

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

OR just plain text:
"total 4\ndrwxr-xr-x 2 user user 4096 Jan 1 12:00 Desktop\n"

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
