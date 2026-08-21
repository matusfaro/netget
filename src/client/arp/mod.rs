//! ARP client implementation
//!
//! This module provides functionality to send ARP requests and monitor ARP traffic
//! using libpcap and pnet for packet construction and capture.

pub mod actions;

pub use actions::ArpClientProtocol;

use anyhow::{Context, Result};
use pcap::{Capture, Device};
use pnet::packet::arp::{ArpHardwareTypes, ArpOperations, ArpPacket, MutableArpPacket};
use pnet::packet::ethernet::{EtherTypes, EthernetPacket, MutableEthernetPacket};
use pnet::packet::Packet;
use pnet::util::MacAddr;
use std::net::{Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};

use crate::client::arp::actions::{ARP_CLIENT_RESPONSE_RECEIVED_EVENT, ARP_CLIENT_STARTED_EVENT};
use crate::client::llm_budget::call_llm_for_client;
use crate::llm::actions::client_trait::{Client, ClientActionResult};
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::logging::emit::Log;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::client_handles::{ClientCommand, ClientSendOutcome};
use crate::state::{AccessLogOwner, ClientId, ClientStatus};

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
}

/// One Ethernet frame handed to the pcap injection thread.
///
/// libpcap is a blocking API and its send handle is owned by a dedicated OS thread, so
/// nothing outside that thread can call `sendpacket`. `ack` is how an injected command
/// learns whether the frame really went out: the injection thread reports the result of
/// `sendpacket` back, which is what lets the command loop answer `Sent { bytes_sent }`
/// truthfully instead of guessing.
struct InjectedPacket {
    frame: Vec<u8>,
    ack: Option<tokio::sync::oneshot::Sender<std::result::Result<usize, String>>>,
}

/// How long an injected command waits for the pcap thread's acknowledgement.
const INJECTION_ACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Per-client data for LLM handling
struct ClientData {
    state: ConnectionState,
    memory: String,
}

/// ARP client that captures and sends ARP packets
pub struct ArpClient;

impl ArpClient {
    /// List available network interfaces
    pub fn list_devices() -> Result<Vec<Device>> {
        Device::list().context("Failed to list network devices")
    }

    /// Find a device by name
    pub fn find_device(name: &str) -> Result<Device> {
        let devices = Self::list_devices()?;
        devices
            .into_iter()
            .find(|d| d.name == name)
            .ok_or_else(|| anyhow::anyhow!("Device '{}' not found", name))
    }

    /// Start ARP client with integrated LLM actions
    pub async fn start_with_llm_actions(
        interface: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        info!(
            "ARP client {} starting on interface: {}",
            client_id, interface
        );
        let _ = status_tx.send(format!(
            "[CLIENT] ARP client {} starting on interface: {}",
            client_id, interface
        ));

        // Update client state
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Initialize client data
        let client_data = Arc::new(Mutex::new(ClientData {
            state: ConnectionState::Idle,
            memory: String::new(),
        }));

        // Get instruction for this client
        let instruction = app_state
            .get_instruction_for_client(client_id)
            .await
            .unwrap_or_else(|| "Monitor ARP traffic".to_string());

        // Send started event to LLM
        let protocol = Arc::new(ArpClientProtocol::new());
        let event = Event::new(
            &ARP_CLIENT_STARTED_EVENT,
            serde_json::json!({
                "interface": interface,
            }),
        );

        // Channel to the pcap injection thread. Created here, not inside the blocking
        // task, so the command loop can reach the send handle too - the LLM path and an
        // injected command hand frames to the same thread through the same queue.
        let (packet_tx, packet_rx) = mpsc::unbounded_channel::<InjectedPacket>();

        // Command channel for injected actions (the dashboard's [ send ]).
        // Registered BEFORE the started-event LLM call: a manual `*` routing rule can park
        // that call for minutes, and the operator must be able to reach the client while it
        // waits.
        let command_rx =
            crate::client::command_support::register_command_channel(&app_state, client_id).await;
        let cmd_task = tokio::spawn(Self::command_loop(
            command_rx,
            protocol.clone(),
            packet_tx.clone(),
            client_id,
            app_state.clone(),
            status_tx.clone(),
        ));
        app_state.register_client_task(client_id, cmd_task).await;

        // Call LLM with started event
        match call_llm_for_client(
            &llm_client,
            &app_state,
            client_id.to_string(),
            &instruction,
            &client_data.lock().await.memory,
            Some(&event),
            protocol.as_ref(),
            &status_tx,
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

                // Process initial actions (if any)
                for _action in actions {
                    debug!("ARP client {} processing initial action", client_id);
                }
            }
            Err(e) => {
                error!("ARP client {} initial LLM call failed: {}", client_id, e);
            }
        }

        // Spawn blocking task for packet capture (pcap is blocking)
        let interface_clone = interface.clone();
        let protocol_clone = protocol.clone();
        let app_state_clone = app_state.clone();
        let status_tx_clone = status_tx.clone();
        let llm_client_clone = llm_client.clone();
        let client_data_clone = client_data.clone();

        tokio::task::spawn_blocking(move || {
            // Find device
            let device = match Self::find_device(&interface_clone) {
                Ok(d) => d,
                Err(e) => {
                    Log::new(Some(&status_tx_clone)).error(format!("Failed to find device: {}", e));
                    return;
                }
            };

            // Open capture for receiving
            let mut cap_rx = match Capture::from_device(device.clone())
                .map(|c| c.promisc(true).snaplen(65535).timeout(1000))
                .and_then(|c| c.open())
            {
                Ok(c) => c,
                Err(e) => {
                    Log::new(Some(&status_tx_clone))
                        .error(format!("Failed to open capture: {}", e));
                    return;
                }
            };

            // Apply ARP filter to receiving capture
            if let Err(e) = cap_rx.filter("arp", true) {
                Log::new(Some(&status_tx_clone))
                    .error(format!("Failed to apply ARP filter: {}", e));
                return;
            }

            // Open capture for sending (separate instance)
            let mut cap_tx = match Capture::from_device(device)
                .map(|c| c.promisc(true).snaplen(65535).timeout(1000))
                .and_then(|c| c.open())
            {
                Ok(c) => c,
                Err(e) => {
                    Log::new(Some(&status_tx_clone))
                        .error(format!("Failed to open capture for sending: {}", e));
                    return;
                }
            };

            let runtime = tokio::runtime::Handle::current();

            // Spawn a dedicated OS thread to handle packet injection. It owns `cap_tx`;
            // `blocking_recv` is legal here because a plain `std::thread` is outside the
            // runtime's async context.
            let mut packet_rx = packet_rx;
            std::thread::spawn(move || {
                while let Some(packet) = packet_rx.blocking_recv() {
                    // This will block, but that's OK - we're in a dedicated thread
                    let len = packet.frame.len();
                    let result = match cap_tx.sendpacket(&packet.frame[..]) {
                        Ok(()) => Ok(len),
                        Err(e) => {
                            error!("Failed to send ARP packet: {}", e);
                            Err(e.to_string())
                        }
                    };
                    if let Some(ack) = packet.ack {
                        let _ = ack.send(result);
                    }
                }
            });

            // Capture loop
            loop {
                match cap_rx.next_packet() {
                    Ok(packet) => {
                        let data = packet.data.to_vec();

                        // Parse Ethernet frame
                        let eth_packet = match EthernetPacket::new(&data) {
                            Some(p) => p,
                            None => {
                                debug!("Failed to parse Ethernet packet");
                                continue;
                            }
                        };

                        // Check if it's an ARP packet
                        if eth_packet.get_ethertype() != EtherTypes::Arp {
                            continue;
                        }

                        // Parse ARP packet
                        let arp_packet = match ArpPacket::new(eth_packet.payload()) {
                            Some(p) => p,
                            None => {
                                debug!("Failed to parse ARP packet");
                                continue;
                            }
                        };

                        // Extract ARP information
                        let operation = arp_packet.get_operation();
                        let sender_mac = arp_packet.get_sender_hw_addr();
                        let sender_ip = arp_packet.get_sender_proto_addr();
                        let target_mac = arp_packet.get_target_hw_addr();
                        let target_ip = arp_packet.get_target_proto_addr();

                        // DEBUG: Log summary
                        Log::new(Some(&status_tx_clone)).debug(format!(
                            "ARP client {} received {} from {} ({}) for {} ({})",
                            client_id,
                            operation_to_string(operation),
                            sender_mac,
                            sender_ip,
                            target_mac,
                            target_ip
                        ));

                        // TRACE: Log full packet
                        let hex_str = hex::encode(&data);
                        trace!("ARP packet (hex): {}", hex_str);

                        let llm_clone = llm_client_clone.clone();
                        let state_clone = app_state_clone.clone();
                        let status_clone = status_tx_clone.clone();
                        let protocol_task_clone = protocol_clone.clone();
                        let packet_tx_clone = packet_tx.clone();
                        let client_data_task_clone = client_data_clone.clone();

                        // Spawn async task to handle packet with LLM
                        runtime.spawn(async move {
                            // Check state
                            let mut client_data_lock = client_data_task_clone.lock().await;

                            match client_data_lock.state {
                                ConnectionState::Idle => {
                                    // Process immediately
                                    client_data_lock.state = ConnectionState::Processing;
                                    let current_memory = client_data_lock.memory.clone();
                                    drop(client_data_lock);

                                    // Build event data
                                    let event = Event::new(
                                        &ARP_CLIENT_RESPONSE_RECEIVED_EVENT,
                                        serde_json::json!({
                                            "operation": operation_to_string(operation),
                                            "sender_mac": sender_mac.to_string(),
                                            "sender_ip": sender_ip.to_string(),
                                            "target_mac": target_mac.to_string(),
                                            "target_ip": target_ip.to_string(),
                                        }),
                                    );

                                    debug!(
                                        "ARP client {} calling LLM for {} packet",
                                        client_id,
                                        operation_to_string(operation)
                                    );

                                    // Get instruction for this client
                                    let instruction = state_clone
                                        .get_instruction_for_client(client_id)
                                        .await
                                        .unwrap_or_else(|| "Monitor ARP traffic".to_string());

                                    match call_llm_for_client(
                                        &llm_clone,
                                        &state_clone,
                                        client_id.to_string(),
                                        &instruction,
                                        &current_memory,
                                        Some(&event),
                                        protocol_task_clone.as_ref(),
                                        &status_clone,
                                    )
                                    .await
                                    {
                                        Ok(ClientLlmResult {
                                            actions,
                                            memory_updates,
                                        }) => {
                                            // Update memory
                                            if let Some(mem) = memory_updates {
                                                client_data_task_clone.lock().await.memory = mem;
                                            }

                                            // Execute actions
                                            for action in actions {
                                                match protocol_task_clone
                                                    .as_ref()
                                                    .execute_action(action)
                                                {
                                                    Ok(ClientActionResult::Custom {
                                                        name,
                                                        data,
                                                    }) => {
                                                        if let Some(packet) =
                                                            build_packet_for_custom_result(
                                                                &name, &data,
                                                            )
                                                        {
                                                            if packet_tx_clone
                                                                .send(InjectedPacket {
                                                                    frame: packet,
                                                                    ack: None,
                                                                })
                                                                .is_ok()
                                                            {
                                                                debug!(
                                                                    "ARP client {} queued {}",
                                                                    client_id, name
                                                                );
                                                            }
                                                        }
                                                    }
                                                    Ok(ClientActionResult::Disconnect) => {
                                                        info!(
                                                            "ARP client {} stopping capture",
                                                            client_id
                                                        );
                                                        state_clone
                                                            .update_client_status(
                                                                client_id,
                                                                ClientStatus::Disconnected,
                                                            )
                                                            .await;
                                                        let _ = status_clone
                                                            .send("__UPDATE_UI__".to_string());
                                                        return;
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            error!("LLM error for ARP client {}: {}", client_id, e);
                                        }
                                    }

                                    // Set state back to idle
                                    client_data_task_clone.lock().await.state =
                                        ConnectionState::Idle;
                                }
                                ConnectionState::Processing => {
                                    // Skip packet - already processing another one
                                    debug!(
                                        "ARP client {} is processing, skipping packet",
                                        client_id
                                    );
                                }
                            }
                        });
                    }
                    Err(pcap::Error::TimeoutExpired) => {
                        // Normal timeout, continue
                        continue;
                    }
                    Err(e) => {
                        Log::new(Some(&status_tx_clone)).error(format!(
                            "ARP client {} packet capture error: {}",
                            client_id, e
                        ));
                        break;
                    }
                }
            }

            // The capture is gone: drop the command handle so the dashboard stops offering
            // [ send ] on a client that can no longer put anything on the wire. This also
            // closes the command channel, which ends `command_loop`.
            runtime.block_on(app_state_clone.remove_client_handle(client_id));
            let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
        });

        // Return a dummy socket address (ARP doesn't use ports)
        Ok(SocketAddr::from(([0, 0, 0, 0], 0)))
    }

    /// Drain injected commands until the channel closes (client removed, or the capture
    /// thread exited and dropped the handle) or an injected `stop_capture` ends the session.
    ///
    /// The generic `command_support::handle_stream_client_command` cannot serve this client:
    /// there is no `AsyncWrite` half at all. An ARP frame is built here exactly as the LLM
    /// path builds it and handed to the same pcap injection thread, which acknowledges the
    /// `sendpacket` call so the reply can be truthful.
    async fn command_loop(
        mut command_rx: mpsc::Receiver<ClientCommand>,
        protocol: Arc<ArpClientProtocol>,
        packet_tx: mpsc::UnboundedSender<InjectedPacket>,
        client_id: ClientId,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) {
        use crate::llm::actions::protocol_trait::Protocol;

        while let Some(command) = command_rx.recv().await {
            let action = command.action.clone();
            let outcome = Self::execute_injected_action(&protocol, &packet_tx, &action).await;

            let outcome_json = match &outcome {
                Ok(outcome) => serde_json::to_value(outcome).unwrap_or(serde_json::Value::Null),
                Err(e) => serde_json::json!({"error": e.to_string()}),
            };
            app_state
                .record_access_log(
                    AccessLogOwner::Client(client_id.as_u32()),
                    protocol.protocol_name(),
                    None,
                    "injected_action",
                    action,
                    vec![outcome_json],
                )
                .await;

            let disconnect = matches!(outcome, Ok(ClientSendOutcome::Disconnected));
            if let Err(e) = &outcome {
                error!("ARP client {} injected action failed: {}", client_id, e);
                let _ = status_tx.send(format!(
                    "[WARN] Client {} injected action failed: {}",
                    client_id, e
                ));
            }
            let _ = status_tx.send("__UPDATE_UI__".to_string());
            crate::client::command_support::reply(command, outcome);

            if disconnect {
                app_state.remove_client_handle(client_id).await;
                app_state
                    .update_client_status(client_id, ClientStatus::Disconnected)
                    .await;
                let _ = status_tx.send("__UPDATE_UI__".to_string());
                break;
            }
        }
    }

    /// Execute one injected action and report exactly what happened to it.
    ///
    /// - `Sent { bytes_sent }` only after the pcap injection thread has acknowledged a
    ///   successful `sendpacket` for that many bytes.
    /// - `Executed { detail }` when the frame could not be handed over at all - which is
    ///   what an unprivileged run looks like, because the capture never opened and the
    ///   injection thread is not running.
    /// - `Rejected { error }` for an action the protocol refuses, or whose MAC/IP fields
    ///   are not parseable into a frame.
    /// - `Err` when pcap accepted the frame and failed to transmit it.
    async fn execute_injected_action(
        protocol: &Arc<ArpClientProtocol>,
        packet_tx: &mpsc::UnboundedSender<InjectedPacket>,
        action: &serde_json::Value,
    ) -> Result<ClientSendOutcome> {
        let result = match protocol.as_ref().execute_action(action.clone()) {
            Ok(result) => result,
            Err(e) => {
                return Ok(ClientSendOutcome::Rejected {
                    error: e.to_string(),
                })
            }
        };

        let (name, data) = match result {
            ClientActionResult::Custom { name, data } => (name, data),
            ClientActionResult::Disconnect => return Ok(ClientSendOutcome::Disconnected),
            ClientActionResult::WaitForMore => {
                return Ok(ClientSendOutcome::Executed {
                    detail: "wait_for_more".to_string(),
                })
            }
            other => {
                return Ok(ClientSendOutcome::Executed {
                    detail: format!("{other:?} builds no ARP frame"),
                })
            }
        };

        let frame = match build_packet_for_custom_result(&name, &data) {
            Some(frame) => frame,
            None => {
                return Ok(ClientSendOutcome::Rejected {
                    error: format!(
                        "could not build an ARP frame for '{name}': check the MAC and IP fields"
                    ),
                })
            }
        };

        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
        if packet_tx
            .send(InjectedPacket {
                frame,
                ack: Some(ack_tx),
            })
            .is_err()
        {
            // The blocking task returned before opening a capture (no such device, or -
            // the usual case off-root - libpcap could not open the interface), so its
            // injection thread never started and the receiver is gone.
            return Ok(ClientSendOutcome::Executed {
                detail: format!(
                    "'{name}' frame built but not injected: the pcap injection thread is not \
                     running (the capture failed to open - ARP capture and injection need \
                     root / BPF access)"
                ),
            });
        }

        match tokio::time::timeout(INJECTION_ACK_TIMEOUT, ack_rx).await {
            Ok(Ok(Ok(bytes_sent))) => Ok(ClientSendOutcome::Sent { bytes_sent }),
            Ok(Ok(Err(e))) => Err(anyhow::anyhow!("pcap sendpacket failed: {e}")),
            Ok(Err(_)) => Ok(ClientSendOutcome::Executed {
                detail: format!(
                    "'{name}' frame handed to the pcap injection thread, which exited \
                     without reporting the result"
                ),
            }),
            Err(_) => Ok(ClientSendOutcome::Executed {
                detail: format!(
                    "'{name}' frame queued, but the pcap injection thread did not \
                     acknowledge it within {}s",
                    INJECTION_ACK_TIMEOUT.as_secs()
                ),
            }),
        }
    }
}

/// Build the Ethernet frame for one of the ARP protocol's `Custom` results, or `None` when
/// the action's MAC/IP fields do not parse. Shared by the LLM path and injected commands so
/// the frame layout exists in one place.
fn build_packet_for_custom_result(name: &str, data: &serde_json::Value) -> Option<Vec<u8>> {
    match name {
        "send_arp_request" => build_arp_request_from_action(data),
        "send_arp_reply" => build_arp_reply_from_action(data),
        _ => None,
    }
}

/// Convert ARP operation to human-readable string
fn operation_to_string(op: pnet::packet::arp::ArpOperation) -> &'static str {
    match op {
        ArpOperations::Request => "REQUEST",
        ArpOperations::Reply => "REPLY",
        _ => "UNKNOWN",
    }
}

/// Build an ARP request packet from action data
fn build_arp_request_from_action(data: &serde_json::Value) -> Option<Vec<u8>> {
    let sender_mac = data["sender_mac"].as_str()?;
    let sender_ip = data["sender_ip"].as_str()?;
    let target_ip = data["target_ip"].as_str()?;

    let sender_mac = MacAddr::from_str(sender_mac).ok()?;
    let sender_ip = Ipv4Addr::from_str(sender_ip).ok()?;
    let target_ip = Ipv4Addr::from_str(target_ip).ok()?;

    Some(build_arp_request(sender_mac, sender_ip, target_ip))
}

/// Build an ARP reply packet from action data
fn build_arp_reply_from_action(data: &serde_json::Value) -> Option<Vec<u8>> {
    let sender_mac = data["sender_mac"].as_str()?;
    let sender_ip = data["sender_ip"].as_str()?;
    let target_mac = data["target_mac"].as_str()?;
    let target_ip = data["target_ip"].as_str()?;

    let sender_mac = MacAddr::from_str(sender_mac).ok()?;
    let sender_ip = Ipv4Addr::from_str(sender_ip).ok()?;
    let target_mac = MacAddr::from_str(target_mac).ok()?;
    let target_ip = Ipv4Addr::from_str(target_ip).ok()?;

    Some(build_arp_reply(
        sender_mac, sender_ip, target_mac, target_ip,
    ))
}

/// Helper function to build an ARP request packet
fn build_arp_request(sender_mac: MacAddr, sender_ip: Ipv4Addr, target_ip: Ipv4Addr) -> Vec<u8> {
    // Ethernet header (14 bytes) + ARP packet (28 bytes) = 42 bytes
    let mut eth_buffer = vec![0u8; 42];

    // Build Ethernet frame
    {
        let mut eth_packet = MutableEthernetPacket::new(&mut eth_buffer).unwrap();
        eth_packet.set_destination(MacAddr::broadcast()); // Broadcast for ARP request
        eth_packet.set_source(sender_mac);
        eth_packet.set_ethertype(EtherTypes::Arp);

        // Build ARP packet
        let mut arp_buffer = vec![0u8; 28];
        {
            let mut arp_packet = MutableArpPacket::new(&mut arp_buffer).unwrap();
            arp_packet.set_hardware_type(ArpHardwareTypes::Ethernet);
            arp_packet.set_protocol_type(EtherTypes::Ipv4);
            arp_packet.set_hw_addr_len(6);
            arp_packet.set_proto_addr_len(4);
            arp_packet.set_operation(ArpOperations::Request);
            arp_packet.set_sender_hw_addr(sender_mac);
            arp_packet.set_sender_proto_addr(sender_ip);
            arp_packet.set_target_hw_addr(MacAddr::zero()); // Unknown for request
            arp_packet.set_target_proto_addr(target_ip);
        }

        eth_packet.set_payload(&arp_buffer);
    }

    eth_buffer
}

/// Helper function to build an ARP reply packet
fn build_arp_reply(
    sender_mac: MacAddr,
    sender_ip: Ipv4Addr,
    target_mac: MacAddr,
    target_ip: Ipv4Addr,
) -> Vec<u8> {
    // Ethernet header (14 bytes) + ARP packet (28 bytes) = 42 bytes
    let mut eth_buffer = vec![0u8; 42];

    // Build Ethernet frame
    {
        let mut eth_packet = MutableEthernetPacket::new(&mut eth_buffer).unwrap();
        eth_packet.set_destination(target_mac);
        eth_packet.set_source(sender_mac);
        eth_packet.set_ethertype(EtherTypes::Arp);

        // Build ARP packet
        let mut arp_buffer = vec![0u8; 28];
        {
            let mut arp_packet = MutableArpPacket::new(&mut arp_buffer).unwrap();
            arp_packet.set_hardware_type(ArpHardwareTypes::Ethernet);
            arp_packet.set_protocol_type(EtherTypes::Ipv4);
            arp_packet.set_hw_addr_len(6);
            arp_packet.set_proto_addr_len(4);
            arp_packet.set_operation(ArpOperations::Reply);
            arp_packet.set_sender_hw_addr(sender_mac);
            arp_packet.set_sender_proto_addr(sender_ip);
            arp_packet.set_target_hw_addr(target_mac);
            arp_packet.set_target_proto_addr(target_ip);
        }

        eth_packet.set_payload(&arp_buffer);
    }

    eth_buffer
}
