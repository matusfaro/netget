//! DataLink client implementation for raw Ethernet frame injection
pub mod actions;

pub use actions::DataLinkClientProtocol;

use anyhow::{Context, Result};
use pcap::{Capture, Device};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, trace};

use crate::client::datalink::actions::DATALINK_CLIENT_FRAME_CAPTURED_EVENT;
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::{Event, StartupParams};
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

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
    queued_frames: Vec<Vec<u8>>,
    memory: String,
}

/// Channel for sending frame injection commands to the pcap thread
#[derive(Clone)]
struct InjectionCommand {
    frame: Vec<u8>,
}

/// DataLink client that injects raw Ethernet frames
pub struct DataLinkClient;

impl DataLinkClient {
    /// Connect to a network interface for frame injection with integrated LLM actions
    pub async fn connect_with_llm_actions(
        _remote_addr: String, // Not used for DataLink (interface instead)
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<StartupParams>,
    ) -> Result<SocketAddr> {
        // Extract interface from startup_params
        let params = startup_params.as_ref().ok_or_else(|| {
            anyhow::anyhow!("DataLink client requires startup parameters (interface)")
        })?;

        let interface = params.get_string("interface");
        let promiscuous = params.get_optional_bool("promiscuous").unwrap_or(false);

        info!(
            "DataLink client {} opening interface: {} (promiscuous: {})",
            client_id, interface, promiscuous
        );

        // Create channel for frame injection commands
        let (inject_tx, mut inject_rx) = mpsc::unbounded_channel::<InjectionCommand>();
        let inject_tx_arc = Arc::new(Mutex::new(inject_tx));

        // Update client state
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!(
            "[CLIENT] DataLink client {} connected to interface {}",
            client_id, interface
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Initialize client data for capture handling
        let client_data = Arc::new(Mutex::new(ClientData {
            state: ConnectionState::Idle,
            queued_frames: Vec::new(),
            memory: String::new(),
        }));

        // Clone for the blocking task
        let interface_clone = interface.clone();
        let status_tx_clone = status_tx.clone();
        let app_state_clone = app_state.clone();
        let llm_client_clone = llm_client.clone();
        let client_data_clone = client_data.clone();

        // Spawn blocking task for pcap operations
        tokio::task::spawn_blocking(move || {
            // Find device
            let device = match Self::find_device(&interface_clone) {
                Ok(d) => d,
                Err(e) => {
                    error!("DataLink client {} failed to find device: {}", client_id, e);
                    let _ = status_tx_clone.send(format!(
                        "[ERROR] DataLink client {} failed to find device: {}",
                        client_id, e
                    ));
                    return;
                }
            };

            // Open capture
            let mut cap = match Capture::from_device(device)
                .map(|c| c.promisc(promiscuous).snaplen(65535).timeout(100))
                .and_then(|c| c.open())
            {
                Ok(c) => c,
                Err(e) => {
                    error!(
                        "DataLink client {} failed to open capture: {}",
                        client_id, e
                    );
                    let _ = status_tx_clone.send(format!(
                        "[ERROR] DataLink client {} failed to open capture: {}",
                        client_id, e
                    ));
                    return;
                }
            };

            info!("DataLink client {} capture opened successfully", client_id);
            let _ = status_tx_clone.send(format!(
                "[INFO] DataLink client {} ready for frame injection",
                client_id
            ));

            let runtime = tokio::runtime::Handle::current();

            // Main loop: handle injection commands and optionally capture frames
            loop {
                // Check for injection commands (non-blocking)
                if let Ok(cmd) = inject_rx.try_recv() {
                    match cap.sendpacket(&cmd.frame[..]) {
                        Ok(_) => {
                            trace!(
                                "DataLink client {} injected frame ({} bytes)",
                                client_id,
                                cmd.frame.len()
                            );
                            let _ = status_tx_clone.send(format!(
                                "[TRACE] DataLink client {} injected frame ({} bytes)",
                                client_id,
                                cmd.frame.len()
                            ));
                        }
                        Err(e) => {
                            error!(
                                "DataLink client {} frame injection failed: {}",
                                client_id, e
                            );
                            let _ = status_tx_clone.send(format!(
                                "[ERROR] DataLink client {} frame injection failed: {}",
                                client_id, e
                            ));
                        }
                    }
                }

                // If promiscuous mode, capture frames
                if promiscuous {
                    match cap.next_packet() {
                        Ok(packet) => {
                            let frame = packet.data.to_vec();
                            trace!(
                                "DataLink client {} captured frame ({} bytes)",
                                client_id,
                                frame.len()
                            );

                            // Handle frame with LLM
                            let state_clone = app_state_clone.clone();
                            let llm_clone = llm_client_clone.clone();
                            let status_clone = status_tx_clone.clone();
                            let client_data_task = client_data_clone.clone();
                            let inject_tx_task = inject_tx_arc.clone();

                            runtime.spawn(async move {
                                let mut client_data_lock = client_data_task.lock().await;

                                match client_data_lock.state {
                                    ConnectionState::Idle => {
                                        // Process immediately
                                        client_data_lock.state = ConnectionState::Processing;
                                        drop(client_data_lock);

                                        // Call LLM
                                        if let Some(instruction) = state_clone.get_instruction_for_client(client_id).await {
                                            let protocol = Arc::new(crate::client::datalink::actions::DataLinkClientProtocol::new());
                                            let event = Event::new(
                                                &DATALINK_CLIENT_FRAME_CAPTURED_EVENT,
                                                serde_json::json!({
                                                    "frame_hex": hex::encode(&frame),
                                                    "frame_length": frame.len(),
                                                }),
                                            );

                                            match call_llm_for_client(
                                                &llm_clone,
                                                &state_clone,
                                                client_id.to_string(),
                                                &instruction,
                                                &client_data_task.lock().await.memory,
                                                Some(&event),
                                                protocol.as_ref(),
                                                &status_clone,
                                            ).await {
                                                Ok(ClientLlmResult { actions, memory_updates }) => {
                                                    // Update memory
                                                    if let Some(mem) = memory_updates {
                                                        client_data_task.lock().await.memory = mem;
                                                    }

                                                    // Execute actions
                                                    for action in actions {
                                                        use crate::llm::actions::client_trait::Client;
                                                        match protocol.as_ref().execute_action(action) {
                                                            Ok(crate::llm::actions::client_trait::ClientActionResult::SendData(frame_bytes)) => {
                                                                // Send frame injection command
                                                                let _ = inject_tx_task.lock().await.send(InjectionCommand { frame: frame_bytes });
                                                            }
                                                            Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                                                                info!("DataLink client {} disconnecting", client_id);
                                                                // Exit will be handled by loop break
                                                            }
                                                            _ => {}
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    error!("LLM error for DataLink client {}: {}", client_id, e);
                                                }
                                            }
                                        }

                                        // Process queued frames if any
                                        let mut client_data_lock = client_data_task.lock().await;
                                        if !client_data_lock.queued_frames.is_empty() {
                                            client_data_lock.queued_frames.clear();
                                        }
                                        client_data_lock.state = ConnectionState::Idle;
                                    }
                                    ConnectionState::Processing => {
                                        // Queue frame
                                        client_data_lock.queued_frames.push(frame);
                                        client_data_lock.state = ConnectionState::Accumulating;
                                    }
                                    ConnectionState::Accumulating => {
                                        // Continue queuing
                                        client_data_lock.queued_frames.push(frame);
                                    }
                                }
                            });
                        }
                        Err(pcap::Error::TimeoutExpired) => {
                            // Normal timeout, continue
                        }
                        Err(e) => {
                            error!("DataLink client {} capture error: {}", client_id, e);
                            break;
                        }
                    }
                }

                // Small sleep to avoid busy loop when not capturing
                if !promiscuous {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }

            info!("DataLink client {} disconnected", client_id);
            runtime.block_on(async {
                app_state_clone
                    .update_client_status(client_id, ClientStatus::Disconnected)
                    .await;
                let _ = status_tx_clone.send(format!(
                    "[CLIENT] DataLink client {} disconnected",
                    client_id
                ));
                let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
            });
        });

        // For DataLink, we return a dummy socket address since we're not using TCP/UDP
        // The interface name is stored in the client metadata
        Ok(SocketAddr::from(([127, 0, 0, 1], 0)))
    }

    /// Find a network device by name
    fn find_device(name: &str) -> Result<Device> {
        let devices = Device::list().context("Failed to list network devices")?;
        devices
            .into_iter()
            .find(|d| d.name == name)
            .ok_or_else(|| anyhow::anyhow!("Network device '{}' not found", name))
    }
}
