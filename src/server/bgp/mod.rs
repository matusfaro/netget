//! BGP server implementation
//!
//! Border Gateway Protocol (BGP-4) server that allows LLM control over routing protocol operations.
//! Implements RFC 4271 with a 6-state FSM (Idle, Connect, Active, OpenSent, OpenConfirm, Established).

pub mod actions;

use anyhow::{anyhow, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

#[cfg(feature = "bgp")]
use crate::llm::action_helper::call_llm;
#[cfg(feature = "bgp")]
use crate::llm::ollama_client::OllamaClient;
#[cfg(feature = "bgp")]
use crate::protocol::Event;
#[cfg(feature = "bgp")]
use crate::server::BgpProtocol;
#[cfg(feature = "bgp")]
use crate::state::app_state::AppState;
#[cfg(feature = "bgp")]
use crate::state::server::BgpSessionState;
use crate::{console_error, console_info};
#[cfg(feature = "bgp")]
use actions::{BGP_NOTIFICATION_EVENT, BGP_OPEN_EVENT, BGP_UPDATE_EVENT};

// BGP Constants
const BGP_VERSION: u8 = 4;
const BGP_MARKER: [u8; 16] = [0xff; 16]; // All ones marker for BGP message header
const BGP_HEADER_LEN: usize = 19;
const DEFAULT_HOLD_TIME: u16 = 180; // seconds
const DEFAULT_KEEPALIVE_TIME: u16 = 60; // seconds (typically hold_time / 3)

// BGP Message Types (RFC 4271 Section 4.1)
const BGP_MSG_OPEN: u8 = 1;
const BGP_MSG_UPDATE: u8 = 2;
const BGP_MSG_NOTIFICATION: u8 = 3;
const BGP_MSG_KEEPALIVE: u8 = 4;

/// BGP server that handles routing protocol operations with LLM
pub struct BgpServer;

#[cfg(feature = "bgp")]
impl BgpServer {
    /// Spawn BGP server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        let listener =
            crate::server::socket_helpers::create_reusable_tcp_listener(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        console_info!(status_tx, "BGP server listening on {}", local_addr);

        // Extract AS number and router ID from startup params
        let (local_as, router_id) = if let Some(ref params) = startup_params {
            let as_num = params.get_optional_u32("as_number").unwrap_or(65000); // Default private ASN
            let router_id_str = params
                .get_optional_string("router_id")
                .unwrap_or_else(|| "192.168.1.1".to_string());
            console_info!(
                status_tx,
                "BGP configured with AS {} and router ID {}",
                as_num,
                router_id_str
            );
            (as_num, router_id_str)
        } else {
            // Defaults
            (65000, "192.168.1.1".to_string())
        };

        let protocol = Arc::new(BgpProtocol::new());

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        let connection_id = crate::server::connection::ConnectionId::new(
                            app_state.get_next_unified_id().await,
                        );
                        info!("BGP connection {} from {}", connection_id, remote_addr);
                        let _ = status_tx.send(format!(
                            "→ BGP connection {} from {}",
                            connection_id, remote_addr
                        ));

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_clone = protocol.clone();
                        let local_as_clone = local_as;
                        let router_id_clone = router_id.clone();

                        tokio::spawn(async move {
                            let mut session = BgpSession {
                                stream,
                                connection_id,
                                server_id,
                                remote_addr,
                                llm_client: llm_clone.clone(),
                                app_state: state_clone.clone(),
                                status_tx: status_clone.clone(),
                                protocol: protocol_clone.clone(),
                                session_state: BgpSessionState::Connect,
                                peer_as: None,
                                hold_time: DEFAULT_HOLD_TIME,
                                keepalive_time: DEFAULT_KEEPALIVE_TIME,
                                router_id: router_id_clone,
                                local_as: local_as_clone,
                            };

                            // Handle BGP session
                            if let Err(e) = session.handle().await {
                                error!("BGP session error: {}", e);
                                let _ =
                                    status_clone.send(format!("[ERROR] BGP session error: {}", e));
                            }

                            info!("BGP connection {} closed", connection_id);
                            let _ = status_clone
                                .send(format!("✗ BGP connection {} closed", connection_id));
                        });
                    }
                    Err(e) => {
                        console_error!(status_tx, "Failed to accept BGP connection: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }
}

#[cfg(feature = "bgp")]
struct BgpSession {
    stream: tokio::net::TcpStream,
    connection_id: crate::server::connection::ConnectionId,
    server_id: crate::state::ServerId,
    remote_addr: SocketAddr,
    llm_client: OllamaClient,
    app_state: Arc<AppState>,
    status_tx: mpsc::UnboundedSender<String>,
    protocol: Arc<BgpProtocol>,
    session_state: BgpSessionState,
    peer_as: Option<u32>,
    hold_time: u16,
    keepalive_time: u16,
    router_id: String,
    local_as: u32,
}

#[cfg(feature = "bgp")]
impl BgpSession {
    /// Main session handler
    async fn handle(&mut self) -> Result<()> {
        // Start with Connect state (TCP connection already established)
        self.session_state = BgpSessionState::Connect;
        debug!("BGP session {} in Connect state", self.connection_id);
        let _ = self.status_tx.send(format!(
            "[DEBUG] BGP session {} in Connect state",
            self.connection_id
        ));

        // Main message processing loop
        loop {
            // Read BGP message
            let mut header_buf = vec![0u8; BGP_HEADER_LEN];

            // Read header
            match self.stream.read_exact(&mut header_buf).await {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    debug!("BGP connection {} closed by peer", self.connection_id);
                    let _ = self.status_tx.send(format!(
                        "[DEBUG] BGP connection {} closed by peer",
                        self.connection_id
                    ));
                    break;
                }
                Err(e) => {
                    error!("BGP read error: {}", e);
                    let _ = self
                        .status_tx
                        .send(format!("[ERROR] BGP read error: {}", e));
                    break;
                }
            };

            // Validate marker
            if &header_buf[0..16] != &BGP_MARKER {
                error!("BGP invalid marker");
                let _ = self.status_tx.send(format!("[ERROR] BGP invalid marker"));
                self.send_notification(1, 1, &[]).await?; // Message Header Error - Connection Not Synchronized
                break;
            }

            // Parse message length
            let msg_len = u16::from_be_bytes([header_buf[16], header_buf[17]]) as usize;
            if msg_len < BGP_HEADER_LEN || msg_len > 4096 {
                error!("BGP invalid message length: {}", msg_len);
                let _ = self
                    .status_tx
                    .send(format!("[ERROR] BGP invalid message length: {}", msg_len));
                self.send_notification(1, 2, &[]).await?; // Message Header Error - Bad Message Length
                break;
            }

            // Parse message type
            let msg_type = header_buf[18];

            // Read message body
            let body_len = msg_len - BGP_HEADER_LEN;
            let mut body_buf = vec![0u8; body_len];
            if body_len > 0 {
                self.stream.read_exact(&mut body_buf).await?;
            }

            trace!("BGP received message type={} length={}", msg_type, msg_len);
            let _ = self.status_tx.send(format!(
                "[TRACE] BGP received message type={} length={}",
                msg_type, msg_len
            ));

            // Handle message based on type
            match msg_type {
                BGP_MSG_OPEN => {
                    if let Err(e) = self.handle_open_message(&body_buf).await {
                        error!("BGP OPEN handling error: {}", e);
                        let _ = self
                            .status_tx
                            .send(format!("[ERROR] BGP OPEN handling error: {}", e));
                        break;
                    }
                }
                BGP_MSG_KEEPALIVE => {
                    if let Err(e) = self.handle_keepalive_message().await {
                        error!("BGP KEEPALIVE handling error: {}", e);
                        let _ = self
                            .status_tx
                            .send(format!("[ERROR] BGP KEEPALIVE handling error: {}", e));
                        break;
                    }
                }
                BGP_MSG_UPDATE => {
                    if let Err(e) = self.handle_update_message(&body_buf).await {
                        error!("BGP UPDATE handling error: {}", e);
                        let _ = self
                            .status_tx
                            .send(format!("[ERROR] BGP UPDATE handling error: {}", e));
                        break;
                    }
                }
                BGP_MSG_NOTIFICATION => {
                    if let Err(e) = self.handle_notification_message(&body_buf).await {
                        error!("BGP NOTIFICATION handling error: {}", e);
                        let _ = self
                            .status_tx
                            .send(format!("[ERROR] BGP NOTIFICATION handling error: {}", e));
                    }
                    // NOTIFICATION closes the connection
                    break;
                }
                _ => {
                    warn!("BGP unsupported message type: {}", msg_type);
                    let _ = self
                        .status_tx
                        .send(format!("[WARN] BGP unsupported message type: {}", msg_type));
                    self.send_notification(1, 3, &[msg_type]).await?; // Message Header Error - Bad Message Type
                    break;
                }
            }
        }

        Ok(())
    }

    /// Handle BGP OPEN message
    async fn handle_open_message(&mut self, body: &[u8]) -> Result<()> {
        if body.len() < 10 {
            return Err(anyhow!("OPEN message too short"));
        }

        // Parse OPEN message (RFC 4271 Section 4.2)
        let version = body[0];
        let peer_as = u16::from_be_bytes([body[1], body[2]]) as u32;
        let hold_time = u16::from_be_bytes([body[3], body[4]]);
        let bgp_identifier = format!("{}.{}.{}.{}", body[5], body[6], body[7], body[8]);
        let _opt_param_len = body[9] as usize;

        info!(
            "BGP OPEN received: version={}, AS={}, hold_time={}, router_id={}",
            version, peer_as, hold_time, bgp_identifier
        );
        let _ = self.status_tx.send(format!(
            "[INFO] BGP OPEN: AS={}, hold_time={}s, router_id={}",
            peer_as, hold_time, bgp_identifier
        ));

        // Validate version
        if version != BGP_VERSION {
            self.send_notification(2, 1, &[BGP_VERSION]).await?; // OPEN Message Error - Unsupported Version Number
            return Err(anyhow!("Unsupported BGP version: {}", version));
        }

        // Store peer information
        self.peer_as = Some(peer_as);

        // Negotiate hold time (use minimum of local and peer)
        if hold_time > 0 && hold_time < 3 {
            self.send_notification(2, 6, &[]).await?; // OPEN Message Error - Unacceptable Hold Time
            return Err(anyhow!("Unacceptable hold time: {}", hold_time));
        }

        if hold_time > 0 {
            self.hold_time = self.hold_time.min(hold_time);
            self.keepalive_time = self.hold_time / 3;
        }

        // Transition to OpenSent (we'll send OPEN in response)
        self.session_state = BgpSessionState::OpenSent;
        debug!(
            "BGP session {} transitioned to OpenSent",
            self.connection_id
        );
        let _ = self
            .status_tx
            .send(format!("[DEBUG] BGP session transitioned to OpenSent"));

        // Ask LLM how to respond
        let event = Event {
            event_type: &BGP_OPEN_EVENT,
            data: serde_json::json!({
                "connection_id": self.connection_id.to_string(),
                "peer_as": peer_as,
                "hold_time": hold_time,
                "router_id": bgp_identifier,
                "remote_addr": self.remote_addr.to_string(),
            }),
        };

        match call_llm(
            &self.llm_client,
            &self.app_state,
            self.server_id,
            Some(self.connection_id),
            &event,
            &*self.protocol,
        )
        .await
        {
            Ok(_result) => {
                // Actions are executed automatically by the action system
                // For now, send default OPEN response
                self.send_open_message().await?;
            }
            Err(e) => {
                error!("LLM call failed for BGP OPEN: {}", e);
                let _ = self
                    .status_tx
                    .send(format!("[ERROR] LLM call failed: {}", e));
                // Send default OPEN response
                self.send_open_message().await?;
            }
        }

        Ok(())
    }

    /// Handle BGP KEEPALIVE message
    async fn handle_keepalive_message(&mut self) -> Result<()> {
        debug!("BGP KEEPALIVE received from {}", self.remote_addr);
        let _ = self
            .status_tx
            .send(format!("[DEBUG] BGP KEEPALIVE received"));

        // Update FSM state
        match self.session_state {
            BgpSessionState::OpenConfirm => {
                // Transition to Established
                self.session_state = BgpSessionState::Established;
                info!(
                    "BGP session {} established with AS{}",
                    self.connection_id,
                    self.peer_as.unwrap_or(0)
                );
                let _ = self.status_tx.send(format!(
                    "✓ BGP session {} established with AS{}",
                    self.connection_id,
                    self.peer_as.unwrap_or(0)
                ));
            }
            BgpSessionState::Established => {
                // Just a keepalive to maintain the session
                trace!("BGP keepalive in Established state");
            }
            _ => {
                warn!(
                    "BGP KEEPALIVE in unexpected state: {:?}",
                    self.session_state
                );
            }
        }

        // Respond with KEEPALIVE
        self.send_keepalive().await?;

        Ok(())
    }

    /// Handle BGP UPDATE message
    async fn handle_update_message(&mut self, body: &[u8]) -> Result<()> {
        if self.session_state != BgpSessionState::Established {
            return Err(anyhow!("UPDATE received in non-Established state"));
        }

        trace!("BGP UPDATE received: {} bytes", body.len());
        let _ = self
            .status_tx
            .send(format!("[TRACE] BGP UPDATE received: {} bytes", body.len()));

        // Parse UPDATE message (simplified)
        // Full parsing would require extensive path attribute handling

        // Ask LLM how to handle the UPDATE
        let event = Event {
            event_type: &BGP_UPDATE_EVENT,
            data: serde_json::json!({
                "connection_id": self.connection_id.to_string(),
                "peer_as": self.peer_as,
                "update_data": hex::encode(body),
            }),
        };

        match call_llm(
            &self.llm_client,
            &self.app_state,
            self.server_id,
            Some(self.connection_id),
            &event,
            &*self.protocol,
        )
        .await
        {
            Ok(_result) => {
                // Actions are executed automatically by the action system
            }
            Err(e) => {
                error!("LLM call failed for BGP UPDATE: {}", e);
                let _ = self
                    .status_tx
                    .send(format!("[ERROR] LLM call failed: {}", e));
            }
        }

        Ok(())
    }

    /// Handle BGP NOTIFICATION message
    async fn handle_notification_message(&mut self, body: &[u8]) -> Result<()> {
        if body.len() < 2 {
            return Err(anyhow!("NOTIFICATION message too short"));
        }

        let error_code = body[0];
        let error_subcode = body[1];
        let data = if body.len() > 2 { &body[2..] } else { &[] };

        error!(
            "BGP NOTIFICATION received: code={}, subcode={}",
            error_code, error_subcode
        );
        let _ = self.status_tx.send(format!(
            "[ERROR] BGP NOTIFICATION: code={}, subcode={}",
            error_code, error_subcode
        ));

        // Log to LLM
        let event = Event {
            event_type: &BGP_NOTIFICATION_EVENT,
            data: serde_json::json!({
                "connection_id": self.connection_id.to_string(),
                "error_code": error_code,
                "error_subcode": error_subcode,
                "data": hex::encode(data),
            }),
        };

        // Don't wait for LLM response since connection will close
        let _ = call_llm(
            &self.llm_client,
            &self.app_state,
            self.server_id,
            Some(self.connection_id),
            &event,
            &*self.protocol,
        )
        .await;

        Ok(())
    }

    /// Send BGP OPEN message
    async fn send_open_message(&mut self) -> Result<()> {
        let mut msg = Vec::new();

        // BGP Header
        msg.extend_from_slice(&BGP_MARKER);
        msg.extend_from_slice(&[0, 0]); // Length placeholder
        msg.push(BGP_MSG_OPEN);

        // OPEN message body
        msg.push(BGP_VERSION);
        msg.extend_from_slice(&(self.local_as as u16).to_be_bytes()); // AS number (truncated to 16-bit for now)
        msg.extend_from_slice(&self.hold_time.to_be_bytes());

        // Router ID (parse from string)
        let router_id_parts: Vec<u8> = self
            .router_id
            .split('.')
            .filter_map(|s| s.parse::<u8>().ok())
            .collect();
        if router_id_parts.len() == 4 {
            msg.extend_from_slice(&router_id_parts);
        } else {
            // Fallback: use 0.0.0.0
            msg.extend_from_slice(&[0, 0, 0, 0]);
        }

        msg.push(0); // Optional Parameters Length = 0

        // Update length field
        let msg_len = msg.len() as u16;
        msg[16..18].copy_from_slice(&msg_len.to_be_bytes());

        // Send
        self.stream.write_all(&msg).await?;
        self.stream.flush().await?;

        info!(
            "BGP OPEN sent: AS={}, hold_time={}",
            self.local_as, self.hold_time
        );
        let _ = self.status_tx.send(format!(
            "[INFO] BGP OPEN sent: AS={}, hold_time={}s",
            self.local_as, self.hold_time
        ));

        // Transition to OpenConfirm
        self.session_state = BgpSessionState::OpenConfirm;
        debug!(
            "BGP session {} transitioned to OpenConfirm",
            self.connection_id
        );
        let _ = self
            .status_tx
            .send(format!("[DEBUG] BGP session transitioned to OpenConfirm"));

        Ok(())
    }

    /// Send BGP KEEPALIVE message
    async fn send_keepalive(&mut self) -> Result<()> {
        let mut msg = Vec::new();

        // BGP Header
        msg.extend_from_slice(&BGP_MARKER);
        msg.extend_from_slice(&(BGP_HEADER_LEN as u16).to_be_bytes());
        msg.push(BGP_MSG_KEEPALIVE);

        // Send
        self.stream.write_all(&msg).await?;
        self.stream.flush().await?;

        trace!("BGP KEEPALIVE sent");
        let _ = self.status_tx.send(format!("[TRACE] BGP KEEPALIVE sent"));

        Ok(())
    }

    /// Send BGP NOTIFICATION message
    async fn send_notification(
        &mut self,
        error_code: u8,
        error_subcode: u8,
        data: &[u8],
    ) -> Result<()> {
        let mut msg = Vec::new();

        // BGP Header
        msg.extend_from_slice(&BGP_MARKER);
        msg.extend_from_slice(&[0, 0]); // Length placeholder
        msg.push(BGP_MSG_NOTIFICATION);

        // NOTIFICATION body
        msg.push(error_code);
        msg.push(error_subcode);
        msg.extend_from_slice(data);

        // Update length field
        let msg_len = msg.len() as u16;
        msg[16..18].copy_from_slice(&msg_len.to_be_bytes());

        // Send
        self.stream.write_all(&msg).await?;
        self.stream.flush().await?;

        error!(
            "BGP NOTIFICATION sent: code={}, subcode={}",
            error_code, error_subcode
        );
        let _ = self.status_tx.send(format!(
            "[ERROR] BGP NOTIFICATION sent: code={}, subcode={}",
            error_code, error_subcode
        ));

        Ok(())
    }
}
