//! WebRTC Signaling Server - WebSocket-based SDP relay for WebRTC connections
pub mod actions;

use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tracing::{debug, error, info, trace, warn};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::server::WebRtcSignalingProtocol;
use crate::state::app_state::AppState;
use crate::state::ServerId;
use actions::{
    WEBRTC_SIGNALING_PEER_CONNECTED_EVENT, WEBRTC_SIGNALING_PEER_DISCONNECTED_EVENT,
};

/// Unique identifier for a signaling peer
pub type PeerId = String;

/// Signaling message types
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum SignalingMessage {
    /// Register with a peer ID
    #[serde(rename = "register")]
    Register { peer_id: String },

    /// SDP offer
    #[serde(rename = "offer")]
    Offer {
        from: String,
        to: String,
        sdp: serde_json::Value,
    },

    /// SDP answer
    #[serde(rename = "answer")]
    Answer {
        from: String,
        to: String,
        sdp: serde_json::Value,
    },

    /// ICE candidate
    #[serde(rename = "ice_candidate")]
    IceCandidate {
        from: String,
        to: String,
        candidate: serde_json::Value,
    },

    /// Error message
    #[serde(rename = "error")]
    Error { message: String },

    /// Registration success
    #[serde(rename = "registered")]
    Registered { peer_id: String },

    /// Generic relay message
    #[serde(rename = "relay")]
    Relay {
        from: String,
        to: String,
        data: serde_json::Value,
    },
}

/// Peer connection data
struct PeerConnection {
    #[allow(dead_code)]
    peer_id: PeerId,
    ws_tx: futures::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<TcpStream>,
        Message,
    >,
    #[allow(dead_code)]
    remote_addr: SocketAddr,
    #[allow(dead_code)]
    connection_id: ConnectionId,
}

/// WebRTC signaling server shared state
pub struct WebRtcSignalingServerData {
    /// Connected peers indexed by peer ID
    peers: Arc<Mutex<HashMap<PeerId, Arc<Mutex<PeerConnection>>>>>,
}

impl WebRtcSignalingServerData {
    pub fn new() -> Self {
        Self {
            peers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a new peer
    pub async fn register_peer(
        &self,
        peer_id: PeerId,
        ws_tx: futures::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<TcpStream>,
            Message,
        >,
        remote_addr: SocketAddr,
        connection_id: ConnectionId,
    ) -> Result<()> {
        let peer_conn = Arc::new(Mutex::new(PeerConnection {
            peer_id: peer_id.clone(),
            ws_tx,
            remote_addr,
            connection_id,
        }));

        let mut peers = self.peers.lock().await;
        if peers.contains_key(&peer_id) {
            anyhow::bail!("Peer ID {} already registered", peer_id);
        }

        peers.insert(peer_id.clone(), peer_conn);
        info!("Registered signaling peer: {}", peer_id);

        Ok(())
    }

    /// Unregister a peer
    pub async fn unregister_peer(&self, peer_id: &str) {
        let mut peers = self.peers.lock().await;
        peers.remove(peer_id);
        info!("Unregistered signaling peer: {}", peer_id);
    }

    /// Forward message to a specific peer
    pub async fn forward_message(&self, to: &str, message: SignalingMessage) -> Result<()> {
        let peers = self.peers.lock().await;
        let peer_conn = peers
            .get(to)
            .context(format!("Peer {} not found", to))?
            .clone();
        drop(peers);

        let msg_json = serde_json::to_string(&message)?;
        let mut peer_conn_lock = peer_conn.lock().await;
        peer_conn_lock
            .ws_tx
            .send(Message::Text(msg_json))
            .await?;

        trace!("Forwarded message to peer {}: {:?}", to, message);
        Ok(())
    }

    /// List all connected peer IDs
    pub async fn list_peers(&self) -> Vec<String> {
        self.peers.lock().await.keys().cloned().collect()
    }

    /// Get peer count
    pub async fn peer_count(&self) -> usize {
        self.peers.lock().await.len()
    }
}

/// WebRTC signaling server
pub struct WebRtcSignalingServer;

impl WebRtcSignalingServer {
    /// Spawn the WebRTC signaling server
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: ServerId,
    ) -> Result<SocketAddr> {
        let listener = TcpListener::bind(listen_addr).await?;
        let local_addr = listener.local_addr()?;
        info!("WebRTC Signaling server listening on {}", local_addr);
        let _ = status_tx.send(format!(
            "[INFO] WebRTC Signaling server listening on {}",
            local_addr
        ));

        // Create server data
        let server_data = Arc::new(WebRtcSignalingServerData::new());

        // Store server data in AppState for action execution
        app_state
            .with_server_mut(server_id, |server| {
                server.set_protocol_field(
                    "server_data_ptr".to_string(),
                    serde_json::json!(Arc::into_raw(Arc::clone(&server_data)) as usize),
                );
            })
            .await;

        let protocol = Arc::new(WebRtcSignalingProtocol::new());

        // Spawn accept loop
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        info!("Signaling server accepted connection from {}", remote_addr);

                        let server_data_clone = Arc::clone(&server_data);
                        let app_state_clone = Arc::clone(&app_state);
                        let status_tx_clone = status_tx.clone();
                        let llm_client_clone = llm_client.clone();
                        let protocol_clone = Arc::clone(&protocol);

                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_connection(
                                stream,
                                remote_addr,
                                server_data_clone,
                                app_state_clone,
                                status_tx_clone,
                                llm_client_clone,
                                server_id,
                                protocol_clone,
                            )
                            .await
                            {
                                error!(
                                    "Error handling signaling connection from {}: {}",
                                    remote_addr, e
                                );
                            }
                        });
                    }
                    Err(e) => {
                        error!("Error accepting signaling connection: {}", e);
                    }
                }
            }
        });

        Ok(local_addr)
    }

    async fn handle_connection(
        stream: TcpStream,
        remote_addr: SocketAddr,
        server_data: Arc<WebRtcSignalingServerData>,
        app_state: Arc<AppState>,
        _status_tx: mpsc::UnboundedSender<String>,
        llm_client: OllamaClient,
        server_id: ServerId,
        protocol: Arc<WebRtcSignalingProtocol>,
    ) -> Result<()> {
        // Upgrade to WebSocket
        let ws_stream = accept_async(stream).await?;
        info!("WebSocket connection established with {}", remote_addr);

        let (ws_tx, mut ws_rx) = ws_stream.split();

        let mut peer_id: Option<PeerId> = None;
        let mut connection_id: Option<ConnectionId> = None;

        // Handle incoming messages
        while let Some(msg_result) = ws_rx.next().await {
            match msg_result {
                Ok(Message::Text(text)) => {
                    trace!("Received signaling message: {}", text);

                    // Parse message
                    let message: SignalingMessage = match serde_json::from_str(&text) {
                        Ok(m) => m,
                        Err(e) => {
                            warn!("Invalid signaling message: {}", e);
                            continue;
                        }
                    };

                    match message {
                        SignalingMessage::Register {
                            peer_id: new_peer_id,
                        } => {
                            // Register peer
                            let conn_id = ConnectionId::new(app_state.get_next_unified_id().await);
                            connection_id = Some(conn_id);

                            match server_data
                                .register_peer(new_peer_id.clone(), ws_tx, remote_addr, conn_id)
                                .await
                            {
                                Ok(_) => {
                                    peer_id = Some(new_peer_id.clone());

                                    // Add connection to server
                                    use crate::state::server::{
                                        ConnectionState as ServerConnectionState,
                                        ConnectionStatus, ProtocolConnectionInfo,
                                    };
                                    let now = std::time::Instant::now();
                                    let conn_state = ServerConnectionState {
                                        id: conn_id,
                                        remote_addr,
                                        local_addr: "0.0.0.0:0".parse().unwrap(),
                                        bytes_sent: 0,
                                        bytes_received: 0,
                                        packets_sent: 0,
                                        packets_received: 0,
                                        last_activity: now,
                                        status: ConnectionStatus::Active,
                                        status_changed_at: now,
                                        protocol_info: ProtocolConnectionInfo::new(
                                            serde_json::json!({
                                                "peer_id": new_peer_id,
                                            }),
                                        ),
                                    };
                                    app_state
                                        .add_connection_to_server(server_id, conn_state)
                                        .await;

                                    // Fire connected event
                                    let event = Event::new(
                                        &WEBRTC_SIGNALING_PEER_CONNECTED_EVENT,
                                        serde_json::json!({
                                            "peer_id": new_peer_id,
                                            "remote_addr": remote_addr.to_string(),
                                        }),
                                    );

                                    let _ = call_llm(
                                        &llm_client,
                                        &app_state,
                                        server_id,
                                        None, // connection_id
                                        &event,
                                        protocol.as_ref(),
                                    )
                                    .await;

                                    // Send registration confirmation
                                    // (ws_tx was moved into register_peer, can't use here)
                                    info!("Peer {} registered successfully", new_peer_id);
                                }
                                Err(e) => {
                                    warn!("Failed to register peer {}: {}", new_peer_id, e);
                                    // Can't send error, ws_tx was consumed
                                    break;
                                }
                            }

                            // ws_tx was consumed, can't continue with this connection
                            break;
                        }
                        SignalingMessage::Offer { from, to, sdp } => {
                            // Forward offer
                            if let Err(e) = server_data
                                .forward_message(
                                    &to,
                                    SignalingMessage::Offer {
                                        from: from.clone(),
                                        to: to.clone(),
                                        sdp,
                                    },
                                )
                                .await
                            {
                                warn!("Failed to forward offer from {} to {}: {}", from, to, e);
                            }
                        }
                        SignalingMessage::Answer { from, to, sdp } => {
                            // Forward answer
                            if let Err(e) = server_data
                                .forward_message(
                                    &to,
                                    SignalingMessage::Answer {
                                        from: from.clone(),
                                        to: to.clone(),
                                        sdp,
                                    },
                                )
                                .await
                            {
                                warn!("Failed to forward answer from {} to {}: {}", from, to, e);
                            }
                        }
                        SignalingMessage::IceCandidate { from, to, candidate } => {
                            // Forward ICE candidate
                            if let Err(e) = server_data
                                .forward_message(
                                    &to,
                                    SignalingMessage::IceCandidate {
                                        from: from.clone(),
                                        to: to.clone(),
                                        candidate,
                                    },
                                )
                                .await
                            {
                                warn!(
                                    "Failed to forward ICE candidate from {} to {}: {}",
                                    from, to, e
                                );
                            }
                        }
                        _ => {
                            debug!("Ignoring signaling message: {:?}", message);
                        }
                    }
                }
                Ok(Message::Close(_)) => {
                    info!("Signaling connection closed by peer");
                    break;
                }
                Ok(_) => {
                    // Ignore binary, ping, pong messages
                }
                Err(e) => {
                    warn!("WebSocket error: {}", e);
                    break;
                }
            }
        }

        // Cleanup on disconnect
        if let Some(pid) = peer_id {
            server_data.unregister_peer(&pid).await;

            // Fire disconnected event
            let event = Event::new(
                &WEBRTC_SIGNALING_PEER_DISCONNECTED_EVENT,
                serde_json::json!({
                    "peer_id": pid,
                }),
            );

            let _ = call_llm(
                &llm_client,
                &app_state,
                server_id,
                None, // connection_id
                &event,
                protocol.as_ref(),
            )
            .await;

            // Remove connection from server
            if let Some(conn_id) = connection_id {
                app_state
                    .remove_connection_from_server(server_id, conn_id)
                    .await;
            }
        }

        Ok(())
    }
}
