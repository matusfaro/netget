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
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::{Client, ClientActionResult};
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
}

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
                    error!("Failed to find device: {}", e);
                    let _ = status_tx_clone.send(format!("[ERROR] Failed to find device: {}", e));
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
                    error!("Failed to open capture: {}", e);
                    let _ = status_tx_clone.send(format!("[ERROR] Failed to open capture: {}", e));
                    return;
                }
            };

            // Apply ARP filter to receiving capture
            if let Err(e) = cap_rx.filter("arp", true) {
                error!("Failed to apply ARP filter: {}", e);
                let _ = status_tx_clone.send(format!("[ERROR] Failed to apply ARP filter: {}", e));
                return;
            }

            // Open capture for sending (separate instance)
            let mut cap_tx = match Capture::from_device(device)
                .map(|c| c.promisc(true).snaplen(65535).timeout(1000))
                .and_then(|c| c.open())
            {
                Ok(c) => c,
                Err(e) => {
                    error!("Failed to open capture for sending: {}", e);
                    let _ = status_tx_clone
                        .send(format!("[ERROR] Failed to open capture for sending: {}", e));
                    return;
                }
            };

            let runtime = tokio::runtime::Handle::current();

            // Channel for sending packets from async tasks back to blocking thread
            let (packet_tx, packet_rx) = std::sync::mpsc::channel::<Vec<u8>>();

            // Spawn a task to handle packet injection
            std::thread::spawn(move || {
                while let Ok(packet) = packet_rx.recv() {
                    // This will block, but that's OK - we're in a dedicated thread
                    if let Err(e) = cap_tx.sendpacket(packet) {
                        error!("Failed to send ARP packet: {}", e);
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
                        debug!(
                            "ARP client {} received {} from {} ({}) for {} ({})",
                            client_id,
                            operation_to_string(operation),
                            sender_mac,
                            sender_ip,
                            target_mac,
                            target_ip
                        );
                        let _ = status_tx_clone.send(format!(
                            "[DEBUG] ARP client {} received {} from {} ({}) for {} ({})",
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
                                                match protocol_task_clone.as_ref().execute_action(action) {
                                                    Ok(ClientActionResult::Custom { name, data }) => {
                                                        if name == "send_arp_request" {
                                                            if let Some(packet) =
                                                                build_arp_request_from_action(&data)
                                                            {
                                                                if packet_tx_clone.send(packet).is_ok() {
                                                                    debug!(
                                                                        "ARP client {} queued ARP request",
                                                                        client_id
                                                                    );
                                                                }
                                                            }
                                                        } else if name == "send_arp_reply" {
                                                            if let Some(packet) =
                                                                build_arp_reply_from_action(&data)
                                                            {
                                                                if packet_tx_clone.send(packet).is_ok() {
                                                                    debug!(
                                                                        "ARP client {} queued ARP reply",
                                                                        client_id
                                                                    );
                                                                }
                                                            }
                                                        }
                                                    }
                                                    Ok(ClientActionResult::Disconnect) => {
                                                        info!("ARP client {} stopping capture", client_id);
                                                        state_clone
                                                            .update_client_status(
                                                                client_id,
                                                                ClientStatus::Disconnected,
                                                            )
                                                            .await;
                                                        let _ = status_clone.send("__UPDATE_UI__".to_string());
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
                                    client_data_task_clone.lock().await.state = ConnectionState::Idle;
                                }
                                ConnectionState::Processing => {
                                    // Skip packet - already processing another one
                                    debug!("ARP client {} is processing, skipping packet", client_id);
                                }
                            }
                        });
                    }
                    Err(pcap::Error::TimeoutExpired) => {
                        // Normal timeout, continue
                        continue;
                    }
                    Err(e) => {
                        error!("ARP client {} packet capture error: {}", client_id, e);
                        let _ = status_tx_clone.send(format!(
                            "[ERROR] ARP client {} packet capture error: {}",
                            client_id, e
                        ));
                        break;
                    }
                }
            }
        });

        // Return a dummy socket address (ARP doesn't use ports)
        Ok(SocketAddr::from(([0, 0, 0, 0], 0)))
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
