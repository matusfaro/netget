//! ARP (Address Resolution Protocol) server implementation
//!
//! This module provides functionality to capture and respond to ARP requests at the data link layer.
//! It uses libpcap via pnet to interact with network interfaces and handle ARP packets.

pub mod actions;

use anyhow::{Context, Result};
use pcap::{Capture, Device};
use pnet::packet::arp::{ArpHardwareTypes, ArpOperations, ArpPacket, MutableArpPacket};
use pnet::packet::ethernet::{EtherTypes, EthernetPacket, MutableEthernetPacket};
use pnet::packet::Packet;
use pnet::util::MacAddr;
use std::net::Ipv4Addr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::{console_error, console_info, console_trace};
use actions::{ArpProtocol, ARP_REQUEST_RECEIVED_EVENT};

/// Get LLM context and output format instructions for ARP stack
pub fn get_llm_protocol_prompt() -> (&'static str, &'static str) {
    let context = r#"You are handling ARP (Address Resolution Protocol) requests at Layer 2.
ARP is used to map IP addresses to MAC addresses on local networks.
You can respond to ARP requests (who has IP X.X.X.X?) with ARP replies containing MAC addresses.
Common use cases: network reconnaissance detection, ARP spoofing simulation, custom IP-to-MAC mappings."#;

    let output_format = r#"IMPORTANT: Respond with a JSON object:
{
  "output": "Ethernet frame containing ARP reply as hex (null if no response)",
  "message": null  // Optional message for user
}"#;

    (context, output_format)
}

/// ARP server that captures and responds to ARP requests
pub struct ArpServer;

impl ArpServer {
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

    /// Spawn ARP server with integrated LLM handling (async wrapper for blocking pcap)
    pub async fn spawn_with_llm(
        interface: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<String> {
        console_info!(
            status_tx,
            "Starting ARP capture on interface: {}",
            interface
        );

        let protocol = Arc::new(ArpProtocol::new());

        // ARP/pcap is blocking, so we run it in a blocking task
        let interface_clone = interface.clone();
        let protocol_clone = protocol.clone();
        tokio::task::spawn_blocking(move || {
            // Find device
            let device = match Self::find_device(&interface_clone) {
                Ok(d) => d,
                Err(e) => {
                    console_error!(status_tx, "Failed to find device: {}", e);
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
                    console_error!(status_tx, "Failed to open capture: {}", e);
                    return;
                }
            };

            // Apply ARP filter to receiving capture
            if let Err(e) = cap_rx.filter("arp", true) {
                console_error!(status_tx, "Failed to apply ARP filter: {}", e);
                return;
            }

            // Open capture for sending (separate instance)
            let mut cap_tx = match Capture::from_device(device)
                .map(|c| c.promisc(true).snaplen(65535).timeout(1000))
                .and_then(|c| c.open())
            {
                Ok(c) => c,
                Err(e) => {
                    console_error!(status_tx, "Failed to open capture for sending: {}", e);
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
                            "ARP {} from {} ({}) for {} ({})",
                            operation_to_string(operation),
                            sender_mac,
                            sender_ip,
                            target_mac,
                            target_ip
                        );
                        let _ = status_tx.send(format!(
                            "[DEBUG] ARP {} from {} ({}) for {} ({})",
                            operation_to_string(operation),
                            sender_mac,
                            sender_ip,
                            target_mac,
                            target_ip
                        ));

                        // TRACE: Log full packet
                        let hex_str = hex::encode(&data);
                        console_trace!(status_tx, "ARP packet (hex): {}", hex_str);

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_task_clone = protocol_clone.clone();
                        let packet_tx_clone = packet_tx.clone();

                        // Spawn async task to handle packet with LLM
                        runtime.spawn(async move {
                            // Build event data
                            let event = Event::new(
                                &ARP_REQUEST_RECEIVED_EVENT,
                                serde_json::json!({
                                    "operation": operation_to_string(operation),
                                    "sender_mac": sender_mac.to_string(),
                                    "sender_ip": sender_ip.to_string(),
                                    "target_mac": target_mac.to_string(),
                                    "target_ip": target_ip.to_string(),
                                    "packet_hex": hex::encode(&data)
                                }),
                            );

                            debug!(
                                "ARP calling LLM for {} packet",
                                operation_to_string(operation)
                            );
                            let _ = status_clone.send(format!(
                                "[DEBUG] ARP calling LLM for {} packet",
                                operation_to_string(operation)
                            ));

                            match call_llm(
                                &llm_clone,
                                &state_clone,
                                server_id,
                                None,
                                &event,
                                protocol_task_clone.as_ref(),
                            )
                            .await
                            {
                                Ok(execution_result) => {
                                    for message in &execution_result.messages {
                                        info!("{}", message);
                                        let _ = status_clone.send(format!("[INFO] {}", message));
                                    }

                                    debug!(
                                        "ARP got {} protocol results",
                                        execution_result.protocol_results.len()
                                    );
                                    let _ = status_clone.send(format!(
                                        "[DEBUG] ARP got {} protocol results",
                                        execution_result.protocol_results.len()
                                    ));

                                    // Send ARP replies if any via channel
                                    for protocol_result in execution_result.protocol_results {
                                        if let Some(output_data) =
                                            protocol_result.get_all_output().first()
                                        {
                                            // Send packet via channel to injection thread
                                            if packet_tx_clone.send(output_data.clone()).is_ok() {
                                                debug!(
                                                    "ARP queued {} bytes for sending",
                                                    output_data.len()
                                                );
                                                let _ = status_clone.send(format!(
                                                    "[DEBUG] ARP queued {} bytes for sending",
                                                    output_data.len()
                                                ));

                                                trace!(
                                                    "ARP reply (hex): {}",
                                                    hex::encode(output_data)
                                                );
                                                let _ = status_clone.send(format!(
                                                    "[TRACE] ARP reply (hex): {}",
                                                    hex::encode(output_data)
                                                ));
                                            } else {
                                                error!("Failed to queue ARP reply");
                                                let _ = status_clone.send(
                                                    "[ERROR] Failed to queue ARP reply".to_string(),
                                                );
                                            }
                                        }
                                    }

                                    let _ = status_clone.send(format!(
                                        "→ ARP {} processed: {} -> {}",
                                        operation_to_string(operation),
                                        sender_ip,
                                        target_ip
                                    ));
                                }
                                Err(e) => {
                                    error!("ARP LLM call failed: {}", e);
                                    let _ = status_clone.send(format!("✗ ARP LLM error: {}", e));
                                }
                            }
                        });
                    }
                    Err(pcap::Error::TimeoutExpired) => {
                        // Normal timeout, continue
                        continue;
                    }
                    Err(e) => {
                        console_error!(status_tx, "Packet capture error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(interface)
    }

    /// Helper function to build an ARP reply packet
    pub fn build_arp_reply(
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
}

/// Convert ARP operation to human-readable string
fn operation_to_string(op: pnet::packet::arp::ArpOperation) -> &'static str {
    match op {
        ArpOperations::Request => "REQUEST",
        ArpOperations::Reply => "REPLY",
        _ => "UNKNOWN",
    }
}
