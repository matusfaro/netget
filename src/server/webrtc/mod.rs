//! WebRTC server implementation (data channels only, no media)
pub mod actions;

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ActionResult;
use crate::protocol::Event;
use crate::server::connection::ConnectionId;
use crate::server::WebRtcProtocol;
use crate::state::app_state::AppState;
use crate::state::ServerId;
use actions::{
    WEBRTC_MESSAGE_RECEIVED_EVENT, WEBRTC_OFFER_RECEIVED_EVENT, WEBRTC_PEER_CONNECTED_EVENT,
    WEBRTC_PEER_DISCONNECTED_EVENT,
};

/// Unique identifier for a WebRTC peer
pub type PeerId = String;

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-peer data for LLM handling
struct PeerData {
    state: ConnectionState,
    queued_messages: Vec<String>,
    memory: String,
    peer_connection: Arc<RTCPeerConnection>,
    data_channel: Option<Arc<RTCDataChannel>>,
    connection_id: ConnectionId,
}

/// WebRTC server data shared across all peers
pub struct WebRtcServerData {
    /// Peer connections indexed by peer ID
    peers: Arc<Mutex<HashMap<PeerId, PeerData>>>,
    /// WebRTC API for creating peer connections
    api: Arc<webrtc::api::API>,
    /// ICE server configuration
    ice_servers: Vec<RTCIceServer>,
}

impl WebRtcServerData {
    pub fn new() -> Result<Self> {
        // Create a MediaEngine
        let mut m = MediaEngine::default();

        // Create an InterceptorRegistry and register default interceptors
        let registry = Registry::new();
        let registry = register_default_interceptors(registry, &mut m)?;

        // Create the API object with the MediaEngine
        let api = APIBuilder::new()
            .with_media_engine(m)
            .with_interceptor_registry(registry)
            .build();

        // Default ICE servers (Google STUN)
        let ice_servers = vec![RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_owned()],
            ..Default::default()
        }];

        Ok(Self {
            peers: Arc::new(Mutex::new(HashMap::new())),
            api: Arc::new(api),
            ice_servers,
        })
    }

    /// Accept an SDP offer from a peer and create an answer
    pub async fn accept_offer(
        &self,
        peer_id: PeerId,
        offer_sdp: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: ServerId,
    ) -> Result<String> {
        info!("WebRTC server accepting offer from peer {}", peer_id);

        // Parse offer
        let offer: RTCSessionDescription =
            serde_json::from_str(&offer_sdp).context("Failed to parse SDP offer JSON")?;

        // Configure ICE servers
        let config = RTCConfiguration {
            ice_servers: self.ice_servers.clone(),
            ..Default::default()
        };

        // Create peer connection
        let peer_connection = Arc::new(self.api.new_peer_connection(config).await?);
        info!("WebRTC server created peer connection for {}", peer_id);

        // Set remote description (the offer)
        peer_connection.set_remote_description(offer).await?;

        // Create a ConnectionId for tracking
        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);

        // Clone for callbacks
        let peer_connection_cb = Arc::clone(&peer_connection);
        let app_state_cb = Arc::clone(&app_state);
        let status_tx_cb = status_tx.clone();
        let llm_client_cb = llm_client.clone();
        let peers_cb = Arc::clone(&self.peers);
        let peer_id_cb = peer_id.clone();

        // Handle incoming data channel
        peer_connection.on_data_channel(Box::new(move |data_channel: Arc<RTCDataChannel>| {
            let app_state = Arc::clone(&app_state_cb);
            let status_tx = status_tx_cb.clone();
            let llm_client = llm_client_cb.clone();
            let peers = Arc::clone(&peers_cb);
            let peer_id = peer_id_cb.clone();
            let pc = Arc::clone(&peer_connection_cb);

            Box::pin(async move {
                let label = data_channel.label().to_string();
                info!(
                    "WebRTC server received data channel '{}' from peer {}",
                    label, peer_id
                );

                // Store data channel in peer data
                {
                    let mut peers_lock = peers.lock().await;
                    if let Some(peer_data) = peers_lock.get_mut(&peer_id) {
                        peer_data.data_channel = Some(Arc::clone(&data_channel));
                    }
                }

                // Set up data channel callbacks
                let dc_clone = Arc::clone(&data_channel);
                let app_state_on_open = Arc::clone(&app_state);
                let status_tx_on_open = status_tx.clone();
                let peers_on_open = Arc::clone(&peers);
                let peer_id_on_open = peer_id.clone();
                let llm_on_open = llm_client.clone();

                data_channel.on_open(Box::new(move || {
                    let app_state = Arc::clone(&app_state_on_open);
                    let status_tx = status_tx_on_open.clone();
                    let peers = Arc::clone(&peers_on_open);
                    let peer_id = peer_id_on_open.clone();
                    let llm_client = llm_on_open.clone();

                    Box::pin(async move {
                        info!("WebRTC server data channel opened for peer {}", peer_id);
                        let _ = status_tx.send(format!(
                            "[SERVER] WebRTC peer {} connected",
                            peer_id
                        ));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        // Call LLM with connected event
                        let protocol = Arc::new(crate::server::WebRtcProtocol::new());
                        let event = Event::new(
                            &WEBRTC_PEER_CONNECTED_EVENT,
                            serde_json::json!({
                                "peer_id": peer_id,
                                "channel_label": label,
                            }),
                        );

                        match call_llm(
                            &llm_client,
                            &app_state,
                            server_id,
                            None, // connection_id
                            &event,
                            protocol.as_ref(),
                        )
                        .await
                        {
                            Ok(_result) => {
                                // ExecutionResult doesn't have memory_updates
                                // Memory is handled via actions
                            }
                            Err(e) => {
                                error!("LLM error for WebRTC peer {} on open: {}", peer_id, e);
                            }
                        }
                    })
                }));

                // Handle incoming messages
                let app_state_on_msg = Arc::clone(&app_state);
                let status_tx_on_msg = status_tx.clone();
                let peers_on_msg = Arc::clone(&peers);
                let peer_id_on_msg = peer_id.clone();
                let llm_on_msg = llm_client.clone();
                let dc_on_msg = Arc::clone(&dc_clone);

                data_channel.on_message(Box::new(move |msg: DataChannelMessage| {
                    let app_state = Arc::clone(&app_state_on_msg);
                    let status_tx = status_tx_on_msg.clone();
                    let peers = Arc::clone(&peers_on_msg);
                    let peer_id = peer_id_on_msg.clone();
                    let llm_client = llm_on_msg.clone();
                    let dc = Arc::clone(&dc_on_msg);

                    Box::pin(async move {
                        let message_text = String::from_utf8_lossy(&msg.data).to_string();
                        trace!(
                            "WebRTC server received message from peer {}: {}",
                            peer_id,
                            message_text
                        );

                        // Handle data with LLM
                        let mut peers_lock = peers.lock().await;
                        let peer_data = match peers_lock.get_mut(&peer_id) {
                            Some(data) => data,
                            None => {
                                error!("Peer {} not found in peers map", peer_id);
                                return;
                            }
                        };

                        match peer_data.state {
                            ConnectionState::Idle => {
                                // Process immediately
                                peer_data.state = ConnectionState::Processing;
                                drop(peers_lock);

                                // Call LLM
                                let protocol = Arc::new(crate::server::WebRtcProtocol::new());
                                let event = Event::new(
                                    &WEBRTC_MESSAGE_RECEIVED_EVENT,
                                    serde_json::json!({
                                        "peer_id": peer_id,
                                        "message": message_text,
                                        "is_binary": !message_text.is_ascii(),
                                    }),
                                );

                                match call_llm(
                                    &llm_client,
                                    &app_state,
                                    server_id,
                                    None, // connection_id
                                    &event,
                                    protocol.as_ref(),
                                )
                                .await
                                {
                                    Ok(result) => {
                                        // Execute protocol results
                                        for action_result in result.protocol_results {
                                            match action_result {
                                                ActionResult::Output(bytes) => {
                                                    match dc.send_text(String::from_utf8_lossy(&bytes).to_string()).await {
                                                        Ok(_) => {
                                                            trace!("WebRTC server sent message to peer {}", peer_id);
                                                        }
                                                        Err(e) => {
                                                            error!("WebRTC server failed to send to peer {}: {}", peer_id, e);
                                                        }
                                                    }
                                                }
                                                ActionResult::CloseConnection => {
                                                    info!("WebRTC server closing connection to peer {}", peer_id);
                                                    let _ = dc.close().await;
                                                }
                                                ActionResult::WaitForMore => {
                                                    trace!("WebRTC server waiting for more data from peer {}", peer_id);
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("LLM error for WebRTC peer {}: {}", peer_id, e);
                                    }
                                }

                                // Reset state and process queued messages
                                let mut peers_lock = peers.lock().await;
                                if let Some(peer_data) = peers_lock.get_mut(&peer_id) {
                                    if !peer_data.queued_messages.is_empty() {
                                        peer_data.state = ConnectionState::Accumulating;
                                    } else {
                                        peer_data.state = ConnectionState::Idle;
                                    }
                                }
                            }
                            ConnectionState::Processing => {
                                // Queue the message
                                peer_data.queued_messages.push(message_text);
                                trace!("WebRTC server queued message from peer {} (already processing)", peer_id);
                            }
                            ConnectionState::Accumulating => {
                                // Add to queue
                                peer_data.queued_messages.push(message_text);
                            }
                        }
                    })
                }));
            })
        }));

        // Handle connection state changes
        let status_tx_state = status_tx.clone();
        let peer_id_state = peer_id.clone();
        let peers_state = Arc::clone(&self.peers);
        let app_state_state = Arc::clone(&app_state);

        peer_connection.on_peer_connection_state_change(Box::new(
            move |state: RTCPeerConnectionState| {
                let status_tx = status_tx_state.clone();
                let peer_id = peer_id_state.clone();
                let peers = Arc::clone(&peers_state);
                let app_state = Arc::clone(&app_state_state);

                Box::pin(async move {
                    info!(
                        "WebRTC server peer {} connection state: {:?}",
                        peer_id, state
                    );
                    match state {
                        RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed => {
                            let _ = status_tx.send(format!(
                                "[SERVER] WebRTC peer {} disconnected",
                                peer_id
                            ));
                            let _ = status_tx.send("__UPDATE_UI__".to_string());

                            // Remove peer from map
                            let connection_id = {
                                let mut peers_lock = peers.lock().await;
                                peers_lock.remove(&peer_id).map(|p| p.connection_id)
                            };

                            // Remove connection from server
                            if let Some(conn_id) = connection_id {
                                app_state
                                    .remove_connection_from_server(server_id, conn_id)
                                    .await;
                            }
                        }
                        _ => {}
                    }
                })
            },
        ));

        // Create answer
        let answer = peer_connection.create_answer(None).await?;
        peer_connection.set_local_description(answer).await?;

        // Wait for ICE gathering to complete
        let mut gather_complete = peer_connection.gathering_complete_promise().await;
        let _ = gather_complete.recv().await;

        // Get the local description with ICE candidates
        let local_desc = peer_connection
            .local_description()
            .await
            .context("No local description available")?;

        // Store peer data
        {
            let mut peers_lock = self.peers.lock().await;
            peers_lock.insert(
                peer_id.clone(),
                PeerData {
                    state: ConnectionState::Idle,
                    queued_messages: Vec::new(),
                    memory: String::new(),
                    peer_connection: Arc::clone(&peer_connection),
                    data_channel: None,
                    connection_id,
                },
            );
        }

        // Add connection to ServerInstance
        use crate::state::server::{
            ConnectionState as ServerConnectionState, ConnectionStatus, ProtocolConnectionInfo,
        };
        let now = std::time::Instant::now();
        let conn_state = ServerConnectionState {
            id: connection_id,
            remote_addr: "0.0.0.0:0".parse().unwrap(), // WebRTC is P2P, no direct remote addr
            local_addr: "0.0.0.0:0".parse().unwrap(),
            bytes_sent: 0,
            bytes_received: 0,
            packets_sent: 0,
            packets_received: 0,
            last_activity: now,
            status: ConnectionStatus::Active,
            status_changed_at: now,
            protocol_info: ProtocolConnectionInfo::new(serde_json::json!({
                "peer_id": peer_id,
                "state": "Idle"
            })),
        };
        app_state
            .add_connection_to_server(server_id, conn_state)
            .await;

        // Return the answer as JSON
        let answer_json = serde_json::to_string_pretty(&local_desc)?;
        info!("WebRTC server generated SDP answer for peer {}", peer_id);

        Ok(answer_json)
    }

    /// Send a message to a specific peer
    pub async fn send_to_peer(&self, peer_id: &str, message: String) -> Result<()> {
        let peers_lock = self.peers.lock().await;
        let peer_data = peers_lock
            .get(peer_id)
            .context("Peer not found")?;

        if let Some(dc) = &peer_data.data_channel {
            dc.send_text(message).await?;
            Ok(())
        } else {
            anyhow::bail!("Data channel not available for peer {}", peer_id);
        }
    }

    /// Close connection to a specific peer
    pub async fn close_peer(&self, peer_id: &str) -> Result<()> {
        let mut peers_lock = self.peers.lock().await;
        if let Some(peer_data) = peers_lock.remove(peer_id) {
            let _ = peer_data.peer_connection.close().await;
            info!("WebRTC server closed connection to peer {}", peer_id);
            Ok(())
        } else {
            anyhow::bail!("Peer {} not found", peer_id);
        }
    }

    /// List all active peer IDs
    pub async fn list_peers(&self) -> Vec<String> {
        self.peers.lock().await.keys().cloned().collect()
    }
}

/// WebRTC server that accepts incoming peer connections
pub struct WebRtcServer;

impl WebRtcServer {
    /// Spawn the WebRTC server with integrated LLM actions
    ///
    /// Note: WebRTC is peer-to-peer, so there's no traditional "listen" address.
    /// The server accepts SDP offers via LLM actions and creates answers.
    pub async fn spawn_with_llm_actions(
        _listen_addr: SocketAddr,
        _llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: ServerId,
    ) -> Result<SocketAddr> {
        info!("WebRTC server (action-based) initializing");
        let _ = status_tx.send("[INFO] WebRTC server ready to accept peer connections (paste SDP offers)".to_string());

        // Create server data
        let server_data = Arc::new(WebRtcServerData::new()?);

        // Store server data in AppState for action execution
        app_state
            .with_server_mut(server_id, |server| {
                server.set_protocol_field(
                    "server_data_ptr".to_string(),
                    serde_json::json!(Arc::into_raw(server_data) as usize),
                );
            })
            .await;

        info!("WebRTC server ready");

        // Return dummy address (WebRTC is P2P, no traditional listen address)
        Ok("0.0.0.0:0".parse().unwrap())
    }
}
