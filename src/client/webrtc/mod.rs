//! WebRTC client implementation (data channels only, no media)
pub mod actions;

pub use actions::WebRtcClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, trace};
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::interceptor::registry::Registry;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::{console_trace, console_debug, console_info, console_warn, console_error};
use crate::client::webrtc::actions::{
    WEBRTC_CLIENT_CONNECTED_EVENT, WEBRTC_CLIENT_MESSAGE_RECEIVED_EVENT,
};

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-client data for LLM handling
struct ClientData {
    state: ConnectionState,
    queued_messages: Vec<String>,
    memory: String,
}

/// WebRTC client that connects via data channels (no media)
pub struct WebRtcClient;

impl WebRtcClient {
    /// Connect to a WebRTC peer with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        info!("WebRTC client {} initializing for {}", client_id, remote_addr);

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

        // Configure ICE servers (Google STUN)
        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }],
            ..Default::default()
        };

        // Create peer connection
        let peer_connection = Arc::new(api.new_peer_connection(config).await?);
        info!("WebRTC client {} created peer connection", client_id);

        // Create data channel
        let data_channel = peer_connection.create_data_channel("netget", None).await?;
        info!("WebRTC client {} created data channel", client_id);

        // Clone for callbacks
        let _pc_clone = Arc::clone(&peer_connection);
        let dc_clone = Arc::clone(&data_channel);
        let app_state_clone = Arc::clone(&app_state);
        let status_tx_clone = status_tx.clone();
        let llm_client_clone = llm_client.clone();

        // Initialize client data
        let client_data = Arc::new(Mutex::new(ClientData {
            state: ConnectionState::Idle,
            queued_messages: Vec::new(),
            memory: String::new(),
        }));

        // Set up data channel callbacks
        let client_data_on_open = Arc::clone(&client_data);
        let app_state_on_open = Arc::clone(&app_state_clone);
        let status_tx_on_open = status_tx_clone.clone();
        let llm_on_open = llm_client_clone.clone();

        data_channel.on_open(Box::new(move || {
            let app_state = Arc::clone(&app_state_on_open);
            let status_tx = status_tx_on_open.clone();
            let client_data = Arc::clone(&client_data_on_open);
            let llm_client = llm_on_open.clone();

            Box::pin(async move {
                app_state.update_client_status(client_id, ClientStatus::Connected).await;
                console_info!(status_tx, "[CLIENT] WebRTC client {} connected", client_id);
                console_info!(status_tx, "__UPDATE_UI__");

                // Call LLM with connected event
                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                    let protocol = Arc::new(crate::client::webrtc::actions::WebRtcClientProtocol::new());
                    let event = Event::new(
                        &WEBRTC_CLIENT_CONNECTED_EVENT,
                        serde_json::json!({
                            "channel_label": "netget",
                        }),
                    );

                    let memory = client_data.lock().await.memory.clone();

                    match call_llm_for_client(
                        &llm_client,
                        &app_state,
                        client_id.to_string(),
                        &instruction,
                        &memory,
                        Some(&event),
                        protocol.as_ref(),
                        &status_tx,
                    ).await {
                        Ok(ClientLlmResult { actions: _, memory_updates }) => {
                            if let Some(mem) = memory_updates {
                                client_data.lock().await.memory = mem;
                            }
                        }
                        Err(e) => {
                            error!("LLM error for WebRTC client {} on open: {}", client_id, e);
                        }
                    }
                }
            })
        }));

        // Set up message handler
        let client_data_on_msg = Arc::clone(&client_data);
        let app_state_on_msg = Arc::clone(&app_state_clone);
        let status_tx_on_msg = status_tx_clone.clone();
        let llm_on_msg = llm_client_clone.clone();
        let dc_on_msg = Arc::clone(&dc_clone);

        data_channel.on_message(Box::new(move |msg: DataChannelMessage| {
            let app_state = Arc::clone(&app_state_on_msg);
            let status_tx = status_tx_on_msg.clone();
            let client_data = Arc::clone(&client_data_on_msg);
            let llm_client = llm_on_msg.clone();
            let dc = Arc::clone(&dc_on_msg);

            Box::pin(async move {
                let message_text = String::from_utf8_lossy(&msg.data).to_string();
                trace!("WebRTC client {} received message: {}", client_id, message_text);

                // Handle data with LLM
                let mut client_data_lock = client_data.lock().await;

                match client_data_lock.state {
                    ConnectionState::Idle => {
                        // Process immediately
                        client_data_lock.state = ConnectionState::Processing;
                        drop(client_data_lock);

                        // Call LLM
                        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                            let protocol = Arc::new(crate::client::webrtc::actions::WebRtcClientProtocol::new());
                            let event = Event::new(
                                &WEBRTC_CLIENT_MESSAGE_RECEIVED_EVENT,
                                serde_json::json!({
                                    "message": message_text,
                                    "is_binary": msg.data.len() > 0 && !message_text.is_ascii(),
                                }),
                            );

                            match call_llm_for_client(
                                &llm_client,
                                &app_state,
                                client_id.to_string(),
                                &instruction,
                                &client_data.lock().await.memory,
                                Some(&event),
                                protocol.as_ref(),
                                &status_tx,
                            ).await {
                                Ok(ClientLlmResult { actions, memory_updates }) => {
                                    // Update memory
                                    if let Some(mem) = memory_updates {
                                        client_data.lock().await.memory = mem;
                                    }

                                    // Execute actions
                                    for action in actions {
                                        use crate::llm::actions::client_trait::Client;
                                        match protocol.as_ref().execute_action(action) {
                                            Ok(crate::llm::actions::client_trait::ClientActionResult::SendData(bytes)) => {
                                                match dc.send_text(String::from_utf8_lossy(&bytes).to_string()).await {
                                                    Ok(_) => {
                                                        trace!("WebRTC client {} sent message", client_id);
                                                    }
                                                    Err(e) => {
                                                        error!("WebRTC client {} failed to send: {}", client_id, e);
                                                    }
                                                }
                                            }
                                            Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                                                info!("WebRTC client {} closing data channel", client_id);
                                                let _ = dc.close().await;
                                            }
                                            Ok(crate::llm::actions::client_trait::ClientActionResult::WaitForMore) => {
                                                trace!("WebRTC client {} waiting for more data", client_id);
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("LLM error for WebRTC client {}: {}", client_id, e);
                                }
                            }
                        }

                        // Reset state and process queued messages
                        let mut client_data_lock = client_data.lock().await;
                        if !client_data_lock.queued_messages.is_empty() {
                            client_data_lock.state = ConnectionState::Accumulating;
                        } else {
                            client_data_lock.state = ConnectionState::Idle;
                        }
                    }
                    ConnectionState::Processing => {
                        // Queue the message
                        client_data_lock.queued_messages.push(message_text);
                        trace!("WebRTC client {} queued message (already processing)", client_id);
                    }
                    ConnectionState::Accumulating => {
                        // Add to queue
                        client_data_lock.queued_messages.push(message_text);
                    }
                }
            })
        }));

        // Handle connection state changes
        let status_tx_state = status_tx_clone.clone();
        peer_connection.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
            let status_tx = status_tx_state.clone();
            Box::pin(async move {
                match state {
                    RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed => {
                        console_info!(status_tx, "[CLIENT] WebRTC client {} disconnected", client_id);
                        console_info!(status_tx, "__UPDATE_UI__");
                    }
                    _ => {}
                }
            })
        }));

        // Create offer
        let offer = peer_connection.create_offer(None).await?;
        peer_connection.set_local_description(offer).await?;

        // Wait for ICE gathering to complete
        let mut gather_complete = peer_connection.gathering_complete_promise().await;
        let _ = gather_complete.recv().await;

        // Get the local description with ICE candidates
        let local_desc = peer_connection.local_description().await
            .context("No local description available")?;

        // Store the SDP offer for the user to exchange with peer
        let offer_json = serde_json::to_string_pretty(&local_desc)?;
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field("sdp_offer".to_string(), serde_json::json!(offer_json));
        }).await;

        console_info!(status_tx, "[CLIENT] WebRTC client {} waiting for SDP answer", client_id);
        console_info!(status_tx, "SDP Offer (send to peer):\n{}", offer_json);
        console_info!(status_tx, "__UPDATE_UI__");

        // Store peer connection and data channel for later use
        let pc_ptr = Arc::into_raw(peer_connection) as usize;
        let dc_ptr = Arc::into_raw(data_channel) as usize;
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field("peer_connection_ptr".to_string(), serde_json::json!(pc_ptr));
            client.set_protocol_field("data_channel_ptr".to_string(), serde_json::json!(dc_ptr));
        }).await;

        // Spawn a task to monitor for answer
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state_clone.get_client(client_id).await.is_none() {
                    info!("WebRTC client {} stopped", client_id);
                    // Clean up Arc pointers
                    unsafe {
                        if pc_ptr != 0 {
                            let _ = Arc::from_raw(pc_ptr as *const RTCPeerConnection);
                        }
                        if dc_ptr != 0 {
                            let _ = Arc::from_raw(dc_ptr as *const RTCDataChannel);
                        }
                    }
                    break;
                }
            }
        });

        // Return a dummy local address (WebRTC is peer-to-peer)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Apply remote SDP answer to complete the connection
    pub async fn apply_answer(
        client_id: ClientId,
        answer_json: String,
        app_state: Arc<AppState>,
    ) -> Result<()> {
        info!("WebRTC client {} applying SDP answer", client_id);

        // Get peer connection pointer
        let pc_ptr = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("peer_connection_ptr")
                .and_then(|v| v.as_u64())
                .map(|p| p as usize)
        }).await.flatten().context("No peer connection found")?;

        // Reconstruct Arc (temporarily)
        let peer_connection = unsafe { Arc::from_raw(pc_ptr as *const RTCPeerConnection) };
        let pc_clone = Arc::clone(&peer_connection);
        // Prevent drop
        let _ = Arc::into_raw(peer_connection);

        // Parse answer
        let answer: RTCSessionDescription = serde_json::from_str(&answer_json)
            .context("Failed to parse SDP answer JSON")?;

        // Set remote description
        pc_clone.set_remote_description(answer).await?;

        info!("WebRTC client {} connection established", client_id);

        Ok(())
    }
}
