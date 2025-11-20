//! DC (Direct Connect) client implementation
pub mod actions;

pub use actions::DcClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;
use tracing::{debug, error, info, trace};

use crate::client::dc::actions::{
    DC_CLIENT_AUTHENTICATED_EVENT, DC_CLIENT_CONNECTED_EVENT, DC_CLIENT_HUBINFO_EVENT,
    DC_CLIENT_KICKED_EVENT, DC_CLIENT_MESSAGE_RECEIVED_EVENT, DC_CLIENT_REDIRECT_EVENT,
    DC_CLIENT_SEARCH_RESULT_EVENT, DC_CLIENT_USERLIST_EVENT,
};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::Client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

/// Enum to handle both TCP and TLS write halves
enum DcWriteHalf {
    Plain(tokio::io::WriteHalf<TcpStream>),
    Tls(tokio::io::WriteHalf<TlsStream<TcpStream>>),
}

impl DcWriteHalf {
    async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        match self {
            DcWriteHalf::Plain(w) => w.write_all(buf).await.map_err(Into::into),
            DcWriteHalf::Tls(w) => w.write_all(buf).await.map_err(Into::into),
        }
    }

    async fn flush(&mut self) -> Result<()> {
        match self {
            DcWriteHalf::Plain(w) => w.flush().await.map_err(Into::into),
            DcWriteHalf::Tls(w) => w.flush().await.map_err(Into::into),
        }
    }
}

/// Connection state for DC authentication
#[derive(Debug, Clone, PartialEq)]
enum DcConnectionState {
    AwaitingLock,   // Waiting for hub's $Lock challenge
    AwaitingHello,  // Sent $Key, waiting for $Hello acceptance
    Authenticated,  // Fully authenticated, can chat/search
}

/// Per-client state for DC
struct DcClientState {
    auth_state: DcConnectionState,
    nickname: String,
    description: String,
    email: String,
    share_size: u64,
    memory: String,
}

/// DC client that connects to a DC hub
pub struct DcClient;

impl DcClient {
    /// Connect to a DC hub with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        // Get startup parameters
        let nickname = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("nickname"))
            .unwrap_or_else(|| "NetGetUser".to_string());
        let description = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("description"))
            .unwrap_or_else(|| "NetGet DC Client".to_string());
        let email = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("email"))
            .unwrap_or_else(|| String::new());
        let share_size = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_u64("share_size"))
            .unwrap_or(0);
        let use_tls = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_bool("use_tls"))
            .unwrap_or(false);
        let auto_reconnect = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_bool("auto_reconnect"))
            .unwrap_or(false);
        let max_reconnect_attempts = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_u64("max_reconnect_attempts"))
            .unwrap_or(5) as u32;
        let initial_reconnect_delay_secs = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_u64("initial_reconnect_delay_secs"))
            .unwrap_or(2);

        // Attempt connection with reconnection loop
        Self::connect_with_reconnect(
            remote_addr,
            llm_client,
            app_state,
            status_tx,
            client_id,
            nickname,
            description,
            email,
            share_size,
            use_tls,
            auto_reconnect,
            max_reconnect_attempts,
            initial_reconnect_delay_secs,
        )
        .await
    }

    /// Internal function to handle connection with reconnection logic
    async fn connect_with_reconnect(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        nickname: String,
        description: String,
        email: String,
        share_size: u64,
        use_tls: bool,
        auto_reconnect: bool,
        max_reconnect_attempts: u32,
        initial_reconnect_delay_secs: u64,
    ) -> Result<SocketAddr> {
        let mut reconnect_attempt = 0u32;

        loop {
            // Attempt to connect
            let connect_result = Self::connect_once(
                remote_addr.clone(),
                llm_client.clone(),
                app_state.clone(),
                status_tx.clone(),
                client_id,
                nickname.clone(),
                description.clone(),
                email.clone(),
                share_size,
                use_tls,
                auto_reconnect,
                max_reconnect_attempts,
                initial_reconnect_delay_secs,
                reconnect_attempt,
            )
            .await;

            match connect_result {
                Ok(local_addr) => return Ok(local_addr),
                Err(e) if !auto_reconnect => {
                    // Not configured for auto-reconnect, return error
                    return Err(e);
                }
                Err(e) if max_reconnect_attempts > 0 && reconnect_attempt >= max_reconnect_attempts => {
                    // Exceeded max attempts
                    error!(
                        "DC client {} exhausted reconnection attempts ({}/{}): {}",
                        client_id, reconnect_attempt, max_reconnect_attempts, e
                    );
                    return Err(anyhow::anyhow!(
                        "Failed to connect after {} attempts: {}",
                        max_reconnect_attempts,
                        e
                    ));
                }
                Err(e) => {
                    // Will reconnect
                    reconnect_attempt += 1;
                    let delay_secs = initial_reconnect_delay_secs * (2u64.pow(reconnect_attempt - 1));
                    let delay_secs = delay_secs.min(60); // Cap at 60 seconds

                    info!(
                        "DC client {} connection failed (attempt {}/{}), retrying in {}s: {}",
                        client_id,
                        reconnect_attempt,
                        if max_reconnect_attempts == 0 { "∞".to_string() } else { max_reconnect_attempts.to_string() },
                        delay_secs,
                        e
                    );

                    let _ = status_tx.send(format!(
                        "[CLIENT] DC client {} reconnecting in {}s (attempt {}/{})",
                        client_id,
                        delay_secs,
                        reconnect_attempt,
                        if max_reconnect_attempts == 0 { "∞".to_string() } else { max_reconnect_attempts.to_string() }
                    ));

                    // Sleep with exponential backoff
                    tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;
                }
            }
        }
    }

    /// Perform a single connection attempt
    async fn connect_once(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        nickname: String,
        description: String,
        email: String,
        share_size: u64,
        use_tls: bool,
        _auto_reconnect: bool,
        _max_reconnect_attempts: u32,
        _initial_reconnect_delay_secs: u64,
        _reconnect_attempt: u32,
    ) -> Result<SocketAddr> {
        // Connect to DC hub
        let tcp_stream = TcpStream::connect(&remote_addr)
            .await
            .context(format!("Failed to connect to DC hub at {}", remote_addr))?;

        let local_addr = tcp_stream.local_addr()?;
        let remote_sock_addr = tcp_stream.peer_addr()?;

        // Wrap with TLS if requested
        if use_tls {
            info!(
                "DC client {} establishing TLS connection to {}",
                client_id, remote_sock_addr
            );

            // Extract hostname for SNI
            let server_name_str = remote_addr.split(':').next().unwrap_or("dc.hub");

            // Create TLS config
            let root_store = RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            };

            let config = ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth();

            let connector = TlsConnector::from(Arc::new(config));

            // Parse server name for SNI
            let server_name = match ServerName::try_from(server_name_str.to_string()) {
                Ok(name) => name,
                Err(_) => {
                    debug!("Failed to parse server name, using IP");
                    ServerName::try_from(remote_sock_addr.ip().to_string())
                        .map_err(|e| anyhow::anyhow!("Invalid server name: {}", e))?
                }
            };

            // Perform TLS handshake
            let tls_stream = connector
                .connect(server_name, tcp_stream)
                .await
                .context("TLS handshake failed")?;

            info!(
                "DC client {} connected via TLS to {} (local: {})",
                client_id, remote_sock_addr, local_addr
            );

            // Update client state
            app_state
                .update_client_status(client_id, ClientStatus::Connected)
                .await;
            let _ = status_tx.send(format!("[CLIENT] DC client {} connected (TLS)", client_id));
            let _ = status_tx.send("__UPDATE_UI__".to_string());

            // Split TLS stream
            let (read_half, write_half) = tokio::io::split(tls_stream);
            let write_half_arc = Arc::new(Mutex::new(DcWriteHalf::Tls(write_half)));

            // Continue with common logic
            return run_dc_client_loop(
                read_half,
                write_half_arc,
                client_id,
                app_state,
                llm_client,
                status_tx,
                nickname,
                description,
                email,
                share_size,
                local_addr,
            )
            .await;
        }

        info!(
            "DC client {} connected to {} (local: {})",
            client_id, remote_sock_addr, local_addr
        );

        // Update client state
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!("[CLIENT] DC client {} connected", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Split stream
        let (read_half, write_half) = tokio::io::split(tcp_stream);
        let write_half_arc = Arc::new(Mutex::new(DcWriteHalf::Plain(write_half)));

        // Continue with common logic
        run_dc_client_loop(
            read_half,
            write_half_arc,
            client_id,
            app_state,
            llm_client,
            status_tx,
            nickname,
            description,
            email,
            share_size,
            local_addr,
        )
        .await
    }
}

/// Run the DC client read loop (common for both TCP and TLS)
async fn run_dc_client_loop<R>(
    read_half: R,
    write_half_arc: Arc<Mutex<DcWriteHalf>>,
    client_id: ClientId,
    app_state: Arc<AppState>,
    llm_client: OllamaClient,
    status_tx: mpsc::UnboundedSender<String>,
    nickname: String,
    description: String,
    email: String,
    share_size: u64,
    local_addr: SocketAddr,
) -> Result<SocketAddr>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    // Wrap in BufReader for line-based reading
    let mut reader = BufReader::new(read_half);
    // Initialize client state
    let client_state = Arc::new(Mutex::new(DcClientState {
        auth_state: DcConnectionState::AwaitingLock,
        nickname: nickname.clone(),
        description,
        email,
        share_size,
        memory: String::new(),
    }));

    // Spawn read loop for DC messages
    tokio::spawn(async move {
        info!("DC client {} read loop started", client_id);

        // Read pipe-delimited messages
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line).await {
                Ok(0) => {
                    info!("DC client {} disconnected from hub", client_id);
                    app_state
                        .update_client_status(client_id, ClientStatus::Disconnected)
                        .await;
                    let _ = status_tx
                        .send(format!("[CLIENT] DC client {} disconnected", client_id));
                    let _ = status_tx.send("__UPDATE_UI__".to_string());
                    break;
                }
                Ok(_) => {
                    // DC messages can span multiple lines but are pipe-delimited
                    // For simplicity, we'll process each pipe-delimited segment
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    // Split by pipe delimiter
                    for segment in trimmed.split('|') {
                        if segment.is_empty() {
                            continue;
                        }

                        trace!("DC client {} received: {}", client_id, segment);

                        // Process DC message
                        if let Err(e) = process_dc_message(
                            segment,
                            &client_state,
                            &write_half_arc,
                            &llm_client,
                            &app_state,
                            &status_tx,
                            client_id,
                        )
                        .await
                        {
                            error!("Error processing DC message: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("DC client {} read error: {}", client_id, e);
                    app_state
                        .update_client_status(client_id, ClientStatus::Error(e.to_string()))
                        .await;
                    let _ = status_tx.send("__UPDATE_UI__".to_string());
                    break;
                }
            }
        }
    });

    Ok(local_addr)
}

/// Process a single DC message (works with both TCP and TLS)
async fn process_dc_message(
    message: &str,
    client_state: &Arc<Mutex<DcClientState>>,
    write_half: &Arc<Mutex<DcWriteHalf>>,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    client_id: ClientId,
) -> Result<()> {
    let state = client_state.lock().await;
    let current_auth_state = state.auth_state.clone();
    let nickname = state.nickname.clone();
    let description = state.description.clone();
    let email = state.email.clone();
    let share_size = state.share_size;
    let memory = state.memory.clone();
    drop(state);

    // Parse message type
    if message.starts_with("$Lock ") {
        // Lock challenge from hub
        handle_lock_message(
            message,
            client_state,
            write_half,
            llm_client,
            app_state,
            status_tx,
            client_id,
            &nickname,
            &description,
            &email,
            share_size,
        )
        .await?;
    } else if message.starts_with("$Hello ") {
        // Hello acceptance from hub
        handle_hello_message(
            message,
            client_state,
            llm_client,
            app_state,
            status_tx,
            client_id,
            &memory,
        )
        .await?;
    } else if message.starts_with("<") {
        // Chat message
        handle_chat_message(
            message,
            client_state,
            write_half,
            llm_client,
            app_state,
            status_tx,
            client_id,
            &memory,
            current_auth_state,
        )
        .await?;
    } else if message.starts_with("$To:") {
        // Private message
        handle_private_message(
            message,
            client_state,
            write_half,
            llm_client,
            app_state,
            status_tx,
            client_id,
            &memory,
            current_auth_state,
        )
        .await?;
    } else if message.starts_with("$SR ") {
        // Search result
        handle_search_result(
            message,
            client_state,
            write_half,
            llm_client,
            app_state,
            status_tx,
            client_id,
            &memory,
            current_auth_state,
        )
        .await?;
    } else if message.starts_with("$NickList ") {
        // User list
        handle_nicklist(
            message,
            client_state,
            write_half,
            llm_client,
            app_state,
            status_tx,
            client_id,
            &memory,
            current_auth_state,
        )
        .await?;
    } else if message.starts_with("$HubName ") {
        // Hub name
        handle_hub_name(
            message,
            client_state,
            write_half,
            llm_client,
            app_state,
            status_tx,
            client_id,
            &memory,
            current_auth_state,
        )
        .await?;
    } else if message.starts_with("$HubTopic ") {
        // Hub topic
        handle_hub_topic(
            message,
            client_state,
            write_half,
            llm_client,
            app_state,
            status_tx,
            client_id,
            &memory,
            current_auth_state,
        )
        .await?;
    } else if message.starts_with("$Kick ") {
        // Kicked from hub
        handle_kick(
            message,
            client_state,
            llm_client,
            app_state,
            status_tx,
            client_id,
            &memory,
        )
        .await?;
    } else if message.starts_with("$ForceMove ") {
        // Redirect to another hub
        handle_redirect(
            message,
            client_state,
            llm_client,
            app_state,
            status_tx,
            client_id,
            &memory,
        )
        .await?;
    } else {
        // Unknown message type - log but don't error
        debug!("DC client {} received unknown message: {}", client_id, message);
    }

    Ok(())
}

/// Handle $Lock challenge
#[allow(clippy::too_many_arguments)]
async fn handle_lock_message(
    message: &str,
    client_state: &Arc<Mutex<DcClientState>>,
    write_half: &Arc<Mutex<DcWriteHalf>>,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    client_id: ClientId,
    nickname: &str,
    description: &str,
    email: &str,
    share_size: u64,
) -> Result<()> {
    // Parse Lock: "$Lock EXTENDEDPROTOCOLABCABCABCABCABCABC Pk=HubName"
    let parts: Vec<&str> = message.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(anyhow::anyhow!("Invalid Lock format"));
    }

    let lock_str = parts[1];
    let pk = parts
        .get(2)
        .and_then(|s| s.strip_prefix("Pk="))
        .unwrap_or("UnknownHub");

    info!("DC client {} received Lock from hub '{}'", client_id, pk);

    // Call LLM with connected event
    let event = Event::new(
        &DC_CLIENT_CONNECTED_EVENT,
        serde_json::json!({
            "lock": lock_str,
            "pk": pk,
        }),
    );

    let instruction = app_state
        .get_instruction_for_client(client_id)
        .await
        .unwrap_or_default();

    match call_llm_for_client(
        llm_client,
        app_state,
        client_id.to_string(),
        &instruction,
        &client_state.lock().await.memory,
        Some(&event),
        &DcClientProtocol::new(),
        status_tx,
    )
    .await
    {
        Ok(ClientLlmResult {
            actions: _,
            memory_updates,
        }) => {
            // Update memory
            if let Some(mem) = memory_updates {
                client_state.lock().await.memory = mem;
            }
        }
        Err(e) => {
            error!("LLM error on dc_connected event: {}", e);
        }
    }

    // Calculate key from lock
    let key = calculate_dc_key(lock_str);

    // Send Key response
    let key_cmd = format!("$Key {}|", key);
    send_dc_command(write_half, &key_cmd).await?;
    info!("DC client {} sent Key", client_id);

    // Send ValidateNick
    let validate_cmd = format!("$ValidateNick {}|", nickname);
    send_dc_command(write_half, &validate_cmd).await?;
    info!("DC client {} sent ValidateNick: {}", client_id, nickname);

    // Send Version (optional but common)
    let version_cmd = "$Version 1,0091|".to_string();
    send_dc_command(write_half, &version_cmd).await?;

    // Send MyINFO
    let myinfo_cmd = format!(
        "$MyINFO $ALL {} {}$ $LAN(T3)A${}${}$|",
        nickname, description, email, share_size
    );
    send_dc_command(write_half, &myinfo_cmd).await?;
    info!("DC client {} sent MyINFO", client_id);

    // Update state
    client_state.lock().await.auth_state = DcConnectionState::AwaitingHello;

    Ok(())
}

/// Handle $Hello acceptance
async fn handle_hello_message(
    message: &str,
    client_state: &Arc<Mutex<DcClientState>>,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    client_id: ClientId,
    memory: &str,
) -> Result<()> {
    // Parse Hello: "$Hello nickname"
    let nickname = message.strip_prefix("$Hello ").unwrap_or("").trim();

    info!(
        "DC client {} authenticated as '{}'",
        client_id, nickname
    );

    // Update state
    client_state.lock().await.auth_state = DcConnectionState::Authenticated;

    // Call LLM with authenticated event
    let event = Event::new(
        &DC_CLIENT_AUTHENTICATED_EVENT,
        serde_json::json!({
            "nickname": nickname,
        }),
    );

    let instruction = app_state
        .get_instruction_for_client(client_id)
        .await
        .unwrap_or_default();

    match call_llm_for_client(
        llm_client,
        app_state,
        client_id.to_string(),
        &instruction,
        memory,
        Some(&event),
        &DcClientProtocol::new(),
        status_tx,
    )
    .await
    {
        Ok(ClientLlmResult {
            actions: _,
            memory_updates,
        }) => {
            // Update memory
            if let Some(mem) = memory_updates {
                client_state.lock().await.memory = mem;
            }

            // Actions are not executed here since authentication is a state transition
            // Any post-auth actions will be sent on subsequent events
        }
        Err(e) => {
            error!("LLM error on dc_authenticated event: {}", e);
        }
    }

    Ok(())
}

/// Handle chat message
#[allow(clippy::too_many_arguments)]
async fn handle_chat_message(
    message: &str,
    client_state: &Arc<Mutex<DcClientState>>,
    write_half: &Arc<Mutex<DcWriteHalf>>,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    client_id: ClientId,
    memory: &str,
    auth_state: DcConnectionState,
) -> Result<()> {
    if auth_state != DcConnectionState::Authenticated {
        return Ok(()); // Ignore messages before authentication
    }

    // Parse chat: "<nickname> message"
    let close_bracket = message.find('>').unwrap_or(0);
    let source = message[1..close_bracket].to_string();
    let msg = message[close_bracket + 2..].to_string(); // Skip "> "

    debug!(
        "DC client {} received chat from {}: {}",
        client_id, source, msg
    );

    // Call LLM
    let event = Event::new(
        &DC_CLIENT_MESSAGE_RECEIVED_EVENT,
        serde_json::json!({
            "source": source,
            "message": msg,
            "is_private": false,
        }),
    );

    let instruction = app_state
        .get_instruction_for_client(client_id)
        .await
        .unwrap_or_default();

    match call_llm_for_client(
        llm_client,
        app_state,
        client_id.to_string(),
        &instruction,
        memory,
        Some(&event),
        &DcClientProtocol::new(),
        status_tx,
    )
    .await
    {
        Ok(result) => {
            execute_dc_actions(
                result,
                client_state,
                write_half,
                client_id,
            )
            .await?;
        }
        Err(e) => {
            error!("LLM error on dc_message_received: {}", e);
        }
    }

    Ok(())
}

/// Handle private message
#[allow(clippy::too_many_arguments)]
async fn handle_private_message(
    message: &str,
    client_state: &Arc<Mutex<DcClientState>>,
    write_half: &Arc<Mutex<DcWriteHalf>>,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    client_id: ClientId,
    memory: &str,
    auth_state: DcConnectionState,
) -> Result<()> {
    if auth_state != DcConnectionState::Authenticated {
        return Ok(());
    }

    // Parse: "$To: target From: source $<source> message"
    // This is simplified - real parsing is more complex
    let parts: Vec<&str> = message.split('$').collect();
    if parts.len() < 3 {
        return Ok(());
    }

    // Extract source from "<source>" pattern
    let source_part = parts.iter().find(|s| s.starts_with('<')).unwrap_or(&"");
    let source = source_part
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or("")
        .to_string();

    let msg = parts.last().unwrap_or(&"").trim().to_string();

    debug!(
        "DC client {} received private message from {}: {}",
        client_id, source, msg
    );

    // Call LLM
    let event = Event::new(
        &DC_CLIENT_MESSAGE_RECEIVED_EVENT,
        serde_json::json!({
            "source": source,
            "message": msg,
            "is_private": true,
        }),
    );

    let instruction = app_state
        .get_instruction_for_client(client_id)
        .await
        .unwrap_or_default();

    match call_llm_for_client(
        llm_client,
        app_state,
        client_id.to_string(),
        &instruction,
        memory,
        Some(&event),
        &DcClientProtocol::new(),
        status_tx,
    )
    .await
    {
        Ok(result) => {
            execute_dc_actions(result, client_state, write_half, client_id).await?;
        }
        Err(e) => {
            error!("LLM error on private message: {}", e);
        }
    }

    Ok(())
}

/// Handle search result
#[allow(clippy::too_many_arguments)]
async fn handle_search_result(
    message: &str,
    client_state: &Arc<Mutex<DcClientState>>,
    write_half: &Arc<Mutex<DcWriteHalf>>,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    client_id: ClientId,
    memory: &str,
    auth_state: DcConnectionState,
) -> Result<()> {
    if auth_state != DcConnectionState::Authenticated {
        return Ok(());
    }

    // Parse: "$SR sender filename\x05size free/total\x05hubname"
    // Simplified parsing
    let without_prefix = message.strip_prefix("$SR ").unwrap_or("");
    let parts: Vec<&str> = without_prefix.split('\x05').collect();

    if parts.is_empty() {
        return Ok(());
    }

    let first_parts: Vec<&str> = parts[0].split_whitespace().collect();
    let source = first_parts.first().unwrap_or(&"").to_string();
    let filename = first_parts.get(1).unwrap_or(&"").to_string();
    let size = parts.get(1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);

    let slots_str = parts.get(2).unwrap_or(&"");
    let slots_parts: Vec<&str> = slots_str.split('/').collect();
    let free_slots = slots_parts.first().and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(0);
    let total_slots = slots_parts.get(1).and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(0);

    debug!(
        "DC client {} received search result: {} from {} ({} bytes, {}/{} slots)",
        client_id, filename, source, size, free_slots, total_slots
    );

    // Call LLM
    let event = Event::new(
        &DC_CLIENT_SEARCH_RESULT_EVENT,
        serde_json::json!({
            "source": source,
            "filename": filename,
            "size": size,
            "free_slots": free_slots,
            "total_slots": total_slots,
        }),
    );

    let instruction = app_state
        .get_instruction_for_client(client_id)
        .await
        .unwrap_or_default();

    match call_llm_for_client(
        llm_client,
        app_state,
        client_id.to_string(),
        &instruction,
        memory,
        Some(&event),
        &DcClientProtocol::new(),
        status_tx,
    )
    .await
    {
        Ok(result) => {
            execute_dc_actions(result, client_state, write_half, client_id).await?;
        }
        Err(e) => {
            error!("LLM error on search result: {}", e);
        }
    }

    Ok(())
}

/// Handle nicklist
#[allow(clippy::too_many_arguments)]
async fn handle_nicklist(
    message: &str,
    client_state: &Arc<Mutex<DcClientState>>,
    write_half: &Arc<Mutex<DcWriteHalf>>,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    client_id: ClientId,
    memory: &str,
    auth_state: DcConnectionState,
) -> Result<()> {
    if auth_state != DcConnectionState::Authenticated {
        return Ok(());
    }

    // Parse: "$NickList nick1$$nick2$$nick3$$"
    let without_prefix = message.strip_prefix("$NickList ").unwrap_or("");
    let users: Vec<String> = without_prefix
        .split("$$")
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    debug!(
        "DC client {} received user list: {} users",
        client_id,
        users.len()
    );

    // Call LLM
    let event = Event::new(
        &DC_CLIENT_USERLIST_EVENT,
        serde_json::json!({
            "users": users,
        }),
    );

    let instruction = app_state
        .get_instruction_for_client(client_id)
        .await
        .unwrap_or_default();

    match call_llm_for_client(
        llm_client,
        app_state,
        client_id.to_string(),
        &instruction,
        memory,
        Some(&event),
        &DcClientProtocol::new(),
        status_tx,
    )
    .await
    {
        Ok(result) => {
            execute_dc_actions(result, client_state, write_half, client_id).await?;
        }
        Err(e) => {
            error!("LLM error on userlist: {}", e);
        }
    }

    Ok(())
}

/// Handle hub name
#[allow(clippy::too_many_arguments)]
async fn handle_hub_name(
    message: &str,
    client_state: &Arc<Mutex<DcClientState>>,
    write_half: &Arc<Mutex<DcWriteHalf>>,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    client_id: ClientId,
    memory: &str,
    auth_state: DcConnectionState,
) -> Result<()> {
    if auth_state != DcConnectionState::Authenticated {
        return Ok(());
    }

    let hub_name = message.strip_prefix("$HubName ").unwrap_or("").to_string();

    debug!("DC client {} hub name: {}", client_id, hub_name);

    // Call LLM
    let event = Event::new(
        &DC_CLIENT_HUBINFO_EVENT,
        serde_json::json!({
            "hub_name": hub_name,
        }),
    );

    let instruction = app_state
        .get_instruction_for_client(client_id)
        .await
        .unwrap_or_default();

    match call_llm_for_client(
        llm_client,
        app_state,
        client_id.to_string(),
        &instruction,
        memory,
        Some(&event),
        &DcClientProtocol::new(),
        status_tx,
    )
    .await
    {
        Ok(result) => {
            execute_dc_actions(result, client_state, write_half, client_id).await?;
        }
        Err(e) => {
            error!("LLM error on hub name: {}", e);
        }
    }

    Ok(())
}

/// Handle hub topic
#[allow(clippy::too_many_arguments)]
async fn handle_hub_topic(
    message: &str,
    client_state: &Arc<Mutex<DcClientState>>,
    write_half: &Arc<Mutex<DcWriteHalf>>,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    client_id: ClientId,
    memory: &str,
    auth_state: DcConnectionState,
) -> Result<()> {
    if auth_state != DcConnectionState::Authenticated {
        return Ok(());
    }

    let hub_topic = message.strip_prefix("$HubTopic ").unwrap_or("").to_string();

    debug!("DC client {} hub topic: {}", client_id, hub_topic);

    // Call LLM
    let event = Event::new(
        &DC_CLIENT_HUBINFO_EVENT,
        serde_json::json!({
            "hub_topic": hub_topic,
        }),
    );

    let instruction = app_state
        .get_instruction_for_client(client_id)
        .await
        .unwrap_or_default();

    match call_llm_for_client(
        llm_client,
        app_state,
        client_id.to_string(),
        &instruction,
        memory,
        Some(&event),
        &DcClientProtocol::new(),
        status_tx,
    )
    .await
    {
        Ok(result) => {
            execute_dc_actions(result, client_state, write_half, client_id).await?;
        }
        Err(e) => {
            error!("LLM error on hub topic: {}", e);
        }
    }

    Ok(())
}

/// Handle kick
async fn handle_kick(
    message: &str,
    _client_state: &Arc<Mutex<DcClientState>>,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    client_id: ClientId,
    memory: &str,
) -> Result<()> {
    let nickname = message.strip_prefix("$Kick ").unwrap_or("").to_string();

    info!("DC client {} was kicked: {}", client_id, nickname);

    // Call LLM
    let event = Event::new(
        &DC_CLIENT_KICKED_EVENT,
        serde_json::json!({
            "nickname": nickname,
        }),
    );

    let instruction = app_state
        .get_instruction_for_client(client_id)
        .await
        .unwrap_or_default();

    let _ = call_llm_for_client(
        llm_client,
        app_state,
        client_id.to_string(),
        &instruction,
        memory,
        Some(&event),
        &DcClientProtocol::new(),
        status_tx,
    )
    .await;

    Ok(())
}

/// Handle redirect
async fn handle_redirect(
    message: &str,
    _client_state: &Arc<Mutex<DcClientState>>,
    llm_client: &OllamaClient,
    app_state: &Arc<AppState>,
    status_tx: &mpsc::UnboundedSender<String>,
    client_id: ClientId,
    memory: &str,
) -> Result<()> {
    let address = message.strip_prefix("$ForceMove ").unwrap_or("").to_string();

    info!("DC client {} redirected to {}", client_id, address);

    // Call LLM
    let event = Event::new(
        &DC_CLIENT_REDIRECT_EVENT,
        serde_json::json!({
            "address": address,
        }),
    );

    let instruction = app_state
        .get_instruction_for_client(client_id)
        .await
        .unwrap_or_default();

    let _ = call_llm_for_client(
        llm_client,
        app_state,
        client_id.to_string(),
        &instruction,
        memory,
        Some(&event),
        &DcClientProtocol::new(),
        status_tx,
    )
    .await;

    Ok(())
}

/// Execute DC actions from LLM result
async fn execute_dc_actions(
    result: ClientLlmResult,
    client_state: &Arc<Mutex<DcClientState>>,
    write_half: &Arc<Mutex<DcWriteHalf>>,
    client_id: ClientId,
) -> Result<()> {
    use crate::llm::actions::client_trait::ClientActionResult;

    // Update memory
    if let Some(mem) = result.memory_updates {
        client_state.lock().await.memory = mem;
    }

    let protocol = DcClientProtocol::new();
    let nickname = client_state.lock().await.nickname.clone();

    // Execute actions
    for action in result.actions {
        match protocol.execute_action(action)? {
            ClientActionResult::Custom { name, data } => match name.as_str() {
                "dc_chat" => {
                    if let Some(message) = data.get("message").and_then(|v| v.as_str()) {
                        let cmd = format!("<{}> {}|", nickname, message);
                        send_dc_command(write_half, &cmd).await?;
                        info!("DC client {} sent chat: {}", client_id, message);
                    }
                }
                "dc_private_message" => {
                    if let (Some(target), Some(message)) = (
                        data.get("target").and_then(|v| v.as_str()),
                        data.get("message").and_then(|v| v.as_str()),
                    ) {
                        let cmd = format!(
                            "$To: {} From: {} $<{}> {}|",
                            target, nickname, nickname, message
                        );
                        send_dc_command(write_half, &cmd).await?;
                        info!(
                            "DC client {} sent private message to {}: {}",
                            client_id, target, message
                        );
                    }
                }
                "dc_search" => {
                    if let Some(query) = data.get("query").and_then(|v| v.as_str()) {
                        // Simple search format: "$Search Hub:nickname F?F?0?1?query"
                        let cmd = format!("$Search Hub:{} F?F?0?1?{}|", nickname, query);
                        send_dc_command(write_half, &cmd).await?;
                        info!("DC client {} sent search: {}", client_id, query);
                    }
                }
                "dc_myinfo" => {
                    if let (Some(description), Some(email), Some(share_size)) = (
                        data.get("description").and_then(|v| v.as_str()),
                        data.get("email").and_then(|v| v.as_str()),
                        data.get("share_size").and_then(|v| v.as_u64()),
                    ) {
                        let cmd = format!(
                            "$MyINFO $ALL {} {}$ $LAN(T3)A${}${}$|",
                            nickname, description, email, share_size
                        );
                        send_dc_command(write_half, &cmd).await?;
                        info!("DC client {} sent MyINFO", client_id);
                    }
                }
                "dc_get_nicklist" => {
                    let cmd = "$GetNickList|".to_string();
                    send_dc_command(write_half, &cmd).await?;
                    info!("DC client {} requested user list", client_id);
                }
                "dc_raw_command" => {
                    if let Some(command) = data.get("command").and_then(|v| v.as_str()) {
                        let mut cmd = command.to_string();
                        if !cmd.ends_with('|') {
                            cmd.push('|');
                        }
                        send_dc_command(write_half, &cmd).await?;
                        info!("DC client {} sent raw command: {}", client_id, command);
                    }
                }
                _ => {}
            },
            ClientActionResult::Disconnect => {
                info!("DC client {} disconnecting", client_id);
                // Send quit
                let cmd = "$Quit|".to_string();
                let _ = send_dc_command(write_half, &cmd).await;
            }
            ClientActionResult::WaitForMore => {
                // Just wait
            }
            _ => {}
        }
    }

    Ok(())
}

/// Send a DC command (ensures pipe termination)
async fn send_dc_command(
    write_half: &Arc<Mutex<DcWriteHalf>>,
    command: &str,
) -> Result<()> {
    let mut write_guard = write_half.lock().await;
    write_guard.write_all(command.as_bytes()).await?;
    write_guard.flush().await?;
    Ok(())
}

/// Calculate DC key from lock using NMDC algorithm
fn calculate_dc_key(lock: &str) -> String {
    let lock_bytes = lock.as_bytes();
    let len = lock_bytes.len();

    if len == 0 {
        return String::new();
    }

    let mut key = vec![0u8; len];

    // NMDC key algorithm
    for i in 1..len {
        key[i] = lock_bytes[i] ^ lock_bytes[i - 1];
    }

    key[0] = lock_bytes[0] ^ lock_bytes[len - 1] ^ lock_bytes[len - 2] ^ 5;

    // Nibble swap each byte
    for byte in &mut key {
        // Nibble swap
        *byte = ((*byte << 4) & 0xF0) | ((*byte >> 4) & 0x0F);
    }

    // Convert to string, escaping non-printable
    String::from_utf8_lossy(&key).to_string()
}
