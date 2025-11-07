//! BGP client implementation (query mode)
//!
//! This BGP client connects to BGP peers to query routing information.
//! It implements passive monitoring/querying, not active route announcement.

pub mod actions;
pub use actions::BgpClientProtocol;

use anyhow::{anyhow, Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace, warn};

use crate::client::bgp::actions::{
    BGP_CLIENT_CONNECTED_EVENT, BGP_CLIENT_NOTIFICATION_RECEIVED_EVENT,
    BGP_CLIENT_UPDATE_RECEIVED_EVENT,
};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::Client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::{Event, StartupParams};
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

// BGP Constants
const BGP_VERSION: u8 = 4;
const BGP_MARKER: [u8; 16] = [0xff; 16]; // All ones marker
const BGP_HEADER_LEN: usize = 19;
const DEFAULT_HOLD_TIME: u16 = 180; // seconds
const DEFAULT_LOCAL_AS: u32 = 65000; // Private ASN for testing
const DEFAULT_ROUTER_ID: &str = "192.168.1.100";

// BGP Message Types (RFC 4271 Section 4.1)
const BGP_MSG_OPEN: u8 = 1;
const BGP_MSG_UPDATE: u8 = 2;
const BGP_MSG_NOTIFICATION: u8 = 3;
const BGP_MSG_KEEPALIVE: u8 = 4;

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// BGP session state
#[derive(Debug, Clone, PartialEq)]
enum BgpState {
    Connect,
    OpenSent,
    OpenConfirm,
    Established,
}

/// Per-client data for LLM handling
struct ClientData {
    state: ConnectionState,
    bgp_state: BgpState,
    queued_data: Vec<u8>,
    memory: String,
    peer_as: Option<u32>,
    peer_router_id: Option<String>,
    hold_time: u16,
}

/// BGP client that connects to a BGP peer
pub struct BgpClient;

impl BgpClient {
    /// Connect to a BGP peer with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<StartupParams>,
    ) -> Result<SocketAddr> {
        // Extract startup parameters
        let local_as = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_u32("local_as"))
            .unwrap_or(DEFAULT_LOCAL_AS);

        let router_id = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("router_id"))
            .unwrap_or_else(|| DEFAULT_ROUTER_ID.to_string());

        let hold_time = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_u32("hold_time"))
            .map(|v| v as u16)
            .unwrap_or(DEFAULT_HOLD_TIME);

        info!(
            "BGP client {} connecting with AS={}, router_id={}, hold_time={}s",
            client_id, local_as, router_id, hold_time
        );

        // Connect to BGP peer (typically port 179)
        let stream = TcpStream::connect(&remote_addr)
            .await
            .context(format!("Failed to connect to BGP peer {}", remote_addr))?;

        let local_addr = stream.local_addr()?;
        let remote_sock_addr = stream.peer_addr()?;

        info!(
            "BGP client {} connected to {} (local: {})",
            client_id, remote_sock_addr, local_addr
        );

        // Update client state
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!("[CLIENT] BGP client {} connected", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Split stream
        let (read_half, write_half) = tokio::io::split(stream);
        let write_half_arc = Arc::new(Mutex::new(write_half));

        // Initialize client data
        let client_data = Arc::new(Mutex::new(ClientData {
            state: ConnectionState::Idle,
            bgp_state: BgpState::Connect,
            queued_data: Vec::new(),
            memory: String::new(),
            peer_as: None,
            peer_router_id: None,
            hold_time,
        }));

        // Send BGP OPEN message immediately after connection
        {
            let open_msg = Self::build_open_message(local_as, hold_time, &router_id)?;
            write_half_arc.lock().await.write_all(&open_msg).await?;
            write_half_arc.lock().await.flush().await?;

            info!(
                "BGP client {} sent OPEN: AS={}, hold_time={}s",
                client_id, local_as, hold_time
            );
            let _ = status_tx.send(format!(
                "[CLIENT] BGP OPEN sent: AS={}, hold_time={}s",
                local_as, hold_time
            ));

            // Transition to OpenSent
            client_data.lock().await.bgp_state = BgpState::OpenSent;
        }

        // Spawn read loop
        let client_data_clone = client_data.clone();
        let write_half_clone = write_half_arc.clone();
        let llm_client_clone = llm_client.clone();
        let app_state_clone = app_state.clone();
        let status_tx_clone = status_tx.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::read_loop(
                read_half,
                write_half_clone,
                client_id,
                llm_client_clone,
                app_state_clone,
                status_tx_clone,
                client_data_clone,
            )
            .await
            {
                error!("BGP client {} read loop error: {}", client_id, e);
            }
        });

        Ok(local_addr)
    }

    /// Build BGP OPEN message
    fn build_open_message(local_as: u32, hold_time: u16, router_id: &str) -> Result<Vec<u8>> {
        let mut msg = Vec::new();

        // BGP Header
        msg.extend_from_slice(&BGP_MARKER);
        msg.extend_from_slice(&[0, 0]); // Length placeholder
        msg.push(BGP_MSG_OPEN);

        // OPEN message body
        msg.push(BGP_VERSION);
        msg.extend_from_slice(&(local_as as u16).to_be_bytes()); // AS number (truncated to 16-bit)
        msg.extend_from_slice(&hold_time.to_be_bytes());

        // Router ID (parse from string)
        let router_id_parts: Vec<u8> = router_id
            .split('.')
            .filter_map(|s| s.parse::<u8>().ok())
            .collect();
        if router_id_parts.len() == 4 {
            msg.extend_from_slice(&router_id_parts);
        } else {
            return Err(anyhow!("Invalid router ID format: {}", router_id));
        }

        msg.push(0); // Optional Parameters Length = 0

        // Update length field
        let msg_len = msg.len() as u16;
        msg[16..18].copy_from_slice(&msg_len.to_be_bytes());

        Ok(msg)
    }

    /// Read loop for BGP messages
    async fn read_loop(
        mut read_half: tokio::io::ReadHalf<TcpStream>,
        write_half: Arc<Mutex<tokio::io::WriteHalf<TcpStream>>>,
        client_id: ClientId,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_data: Arc<Mutex<ClientData>>,
    ) -> Result<()> {
        loop {
            // Read BGP message header
            let mut header_buf = vec![0u8; BGP_HEADER_LEN];

            match read_half.read_exact(&mut header_buf).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    info!("BGP client {} disconnected", client_id);
                    app_state
                        .update_client_status(client_id, ClientStatus::Disconnected)
                        .await;
                    let _ = status_tx.send(format!("[CLIENT] BGP client {} disconnected", client_id));
                    let _ = status_tx.send("__UPDATE_UI__".to_string());
                    break;
                }
                Err(e) => {
                    error!("BGP client {} read error: {}", client_id, e);
                    return Err(e.into());
                }
            }

            // Validate marker
            if &header_buf[0..16] != &BGP_MARKER {
                error!("BGP client {} received invalid marker", client_id);
                return Err(anyhow!("Invalid BGP marker"));
            }

            // Parse message length
            let msg_len = u16::from_be_bytes([header_buf[16], header_buf[17]]) as usize;
            if msg_len < BGP_HEADER_LEN || msg_len > 4096 {
                error!("BGP client {} invalid message length: {}", client_id, msg_len);
                return Err(anyhow!("Invalid BGP message length: {}", msg_len));
            }

            // Parse message type
            let msg_type = header_buf[18];

            // Read message body
            let body_len = msg_len - BGP_HEADER_LEN;
            let mut body_buf = vec![0u8; body_len];
            if body_len > 0 {
                read_half.read_exact(&mut body_buf).await?;
            }

            trace!(
                "BGP client {} received message type={} length={}",
                client_id,
                msg_type,
                msg_len
            );

            // Handle message based on type
            match msg_type {
                BGP_MSG_OPEN => {
                    Self::handle_open_message(
                        &body_buf,
                        &write_half,
                        client_id,
                        &llm_client,
                        &app_state,
                        &status_tx,
                        &client_data,
                    )
                    .await?;
                }
                BGP_MSG_KEEPALIVE => {
                    Self::handle_keepalive_message(
                        &write_half,
                        client_id,
                        &llm_client,
                        &app_state,
                        &status_tx,
                        &client_data,
                    )
                    .await?;
                }
                BGP_MSG_UPDATE => {
                    Self::handle_update_message(
                        &body_buf,
                        client_id,
                        &llm_client,
                        &app_state,
                        &status_tx,
                        &client_data,
                    )
                    .await?;
                }
                BGP_MSG_NOTIFICATION => {
                    Self::handle_notification_message(
                        &body_buf,
                        client_id,
                        &llm_client,
                        &app_state,
                        &status_tx,
                        &client_data,
                    )
                    .await?;
                    // NOTIFICATION closes the connection
                    break;
                }
                _ => {
                    warn!("BGP client {} unsupported message type: {}", client_id, msg_type);
                }
            }
        }

        Ok(())
    }

    /// Handle BGP OPEN message
    async fn handle_open_message(
        body: &[u8],
        write_half: &Arc<Mutex<tokio::io::WriteHalf<TcpStream>>>,
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        client_data: &Arc<Mutex<ClientData>>,
    ) -> Result<()> {
        if body.len() < 10 {
            return Err(anyhow!("OPEN message too short"));
        }

        // Parse OPEN message
        let version = body[0];
        let peer_as = u16::from_be_bytes([body[1], body[2]]) as u32;
        let hold_time = u16::from_be_bytes([body[3], body[4]]);
        let peer_router_id = format!("{}.{}.{}.{}", body[5], body[6], body[7], body[8]);

        info!(
            "BGP client {} received OPEN: AS={}, hold_time={}s, router_id={}",
            client_id, peer_as, hold_time, peer_router_id
        );
        let _ = status_tx.send(format!(
            "[CLIENT] BGP OPEN received: AS={}, hold_time={}s, router_id={}",
            peer_as, hold_time, peer_router_id
        ));

        // Validate version
        if version != BGP_VERSION {
            return Err(anyhow!("Unsupported BGP version: {}", version));
        }

        // Store peer information
        {
            let mut data = client_data.lock().await;
            data.peer_as = Some(peer_as);
            data.peer_router_id = Some(peer_router_id.clone());
            data.hold_time = data.hold_time.min(hold_time);
            data.bgp_state = BgpState::OpenConfirm;
        }

        // Send KEEPALIVE to complete handshake
        let keepalive_msg = Self::build_keepalive_message();
        write_half.lock().await.write_all(&keepalive_msg).await?;
        write_half.lock().await.flush().await?;

        info!("BGP client {} sent KEEPALIVE", client_id);
        let _ = status_tx.send(format!("[CLIENT] BGP KEEPALIVE sent"));

        // Transition to Established (will be confirmed when peer sends KEEPALIVE)
        client_data.lock().await.bgp_state = BgpState::Established;

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(BgpClientProtocol::new());
            let event = Event::new(
                &BGP_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "remote_addr": app_state.get_client(client_id).await.map(|c| c.remote_addr).unwrap_or_default(),
                    "peer_as": peer_as,
                    "peer_router_id": peer_router_id,
                    "hold_time": hold_time,
                }),
            );

            match call_llm_for_client(
                llm_client,
                app_state,
                client_id.to_string(),
                &instruction,
                &client_data.lock().await.memory,
                Some(&event),
                protocol.as_ref(),
                status_tx,
            )
            .await
            {
                Ok(ClientLlmResult {
                    actions,
                    memory_updates,
                }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        client_data.lock().await.memory = mem;
                    }

                    // Execute actions
                    Self::execute_actions(actions, write_half, client_id, protocol.as_ref()).await;
                }
                Err(e) => {
                    error!("LLM error for BGP client {}: {}", client_id, e);
                }
            }
        }

        Ok(())
    }

    /// Handle BGP KEEPALIVE message
    async fn handle_keepalive_message(
        _write_half: &Arc<Mutex<tokio::io::WriteHalf<TcpStream>>>,
        client_id: ClientId,
        _llm_client: &OllamaClient,
        _app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        client_data: &Arc<Mutex<ClientData>>,
    ) -> Result<()> {
        debug!("BGP client {} received KEEPALIVE", client_id);
        let _ = status_tx.send(format!("[CLIENT] BGP KEEPALIVE received"));

        // Update state if in OpenConfirm
        let mut data = client_data.lock().await;
        if data.bgp_state == BgpState::OpenConfirm {
            data.bgp_state = BgpState::Established;
            info!("BGP client {} session established", client_id);
            let _ = status_tx.send(format!("[CLIENT] BGP session established"));
        }

        Ok(())
    }

    /// Handle BGP UPDATE message
    async fn handle_update_message(
        body: &[u8],
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        client_data: &Arc<Mutex<ClientData>>,
    ) -> Result<()> {
        trace!("BGP client {} received UPDATE: {} bytes", client_id, body.len());
        let _ = status_tx.send(format!(
            "[CLIENT] BGP UPDATE received: {} bytes",
            body.len()
        ));

        // Call LLM with UPDATE event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(BgpClientProtocol::new());
            let event = Event::new(
                &BGP_CLIENT_UPDATE_RECEIVED_EVENT,
                serde_json::json!({
                    "update_data_hex": hex::encode(body),
                    "update_length": body.len(),
                }),
            );

            match call_llm_for_client(
                llm_client,
                app_state,
                client_id.to_string(),
                &instruction,
                &client_data.lock().await.memory,
                Some(&event),
                protocol.as_ref(),
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
                        client_data.lock().await.memory = mem;
                    }

                    // Note: No write_half needed for UPDATE handling (read-only)
                }
                Err(e) => {
                    error!("LLM error for BGP client {}: {}", client_id, e);
                }
            }
        }

        Ok(())
    }

    /// Handle BGP NOTIFICATION message
    async fn handle_notification_message(
        body: &[u8],
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        client_data: &Arc<Mutex<ClientData>>,
    ) -> Result<()> {
        if body.len() < 2 {
            return Err(anyhow!("NOTIFICATION message too short"));
        }

        let error_code = body[0];
        let error_subcode = body[1];
        let error_data = if body.len() > 2 { &body[2..] } else { &[] };

        error!(
            "BGP client {} received NOTIFICATION: code={}, subcode={}",
            client_id, error_code, error_subcode
        );
        let _ = status_tx.send(format!(
            "[CLIENT] BGP NOTIFICATION: code={}, subcode={}",
            error_code, error_subcode
        ));

        // Call LLM with NOTIFICATION event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(BgpClientProtocol::new());
            let event = Event::new(
                &BGP_CLIENT_NOTIFICATION_RECEIVED_EVENT,
                serde_json::json!({
                    "error_code": error_code,
                    "error_subcode": error_subcode,
                    "error_data_hex": hex::encode(error_data),
                }),
            );

            let _ = call_llm_for_client(
                llm_client,
                app_state,
                client_id.to_string(),
                &instruction,
                &client_data.lock().await.memory,
                Some(&event),
                protocol.as_ref(),
                status_tx,
            )
            .await;
        }

        Ok(())
    }

    /// Build BGP KEEPALIVE message
    fn build_keepalive_message() -> Vec<u8> {
        let mut msg = Vec::new();

        // BGP Header
        msg.extend_from_slice(&BGP_MARKER);
        msg.extend_from_slice(&(BGP_HEADER_LEN as u16).to_be_bytes());
        msg.push(BGP_MSG_KEEPALIVE);

        msg
    }

    /// Execute actions from LLM
    async fn execute_actions(
        actions: Vec<serde_json::Value>,
        write_half: &Arc<Mutex<tokio::io::WriteHalf<TcpStream>>>,
        client_id: ClientId,
        protocol: &dyn Client,
    ) {
        for action in actions {
            match protocol.execute_action(action) {
                Ok(crate::llm::actions::client_trait::ClientActionResult::SendData(bytes)) => {
                    if let Ok(_) = write_half.lock().await.write_all(&bytes).await {
                        trace!("BGP client {} sent {} bytes", client_id, bytes.len());
                    }
                }
                Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                    info!("BGP client {} disconnecting", client_id);
                    break;
                }
                Ok(crate::llm::actions::client_trait::ClientActionResult::WaitForMore) => {
                    // Do nothing, wait for more data
                }
                Ok(crate::llm::actions::client_trait::ClientActionResult::NoAction) => {
                    // No action needed
                }
                Ok(crate::llm::actions::client_trait::ClientActionResult::Multiple(_actions)) => {
                    // Multiple actions - not used by BGP client
                    debug!("BGP client {} received multiple actions", client_id);
                }
                Ok(crate::llm::actions::client_trait::ClientActionResult::Custom {
                    name,
                    data: _,
                }) => {
                    debug!("BGP client {} custom action: {}", client_id, name);
                }
                Err(e) => {
                    error!("BGP client {} action error: {}", client_id, e);
                }
            }
        }
    }
}
