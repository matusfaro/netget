//! Data Link layer (Layer 2) server implementation using pcap
//!
//! This module provides functionality to capture and inject packets at the data link layer.
//! It uses libpcap to interact with network interfaces.

pub mod actions;

use anyhow::{Context, Result};
use bytes::Bytes;
use pcap::{Capture, Device};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use actions::{DataLinkProtocol, DATALINK_PACKET_CAPTURED_EVENT};
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// Get LLM context and output format instructions for DataLink stack
pub fn get_llm_protocol_prompt() -> (&'static str, &'static str) {
    let context = r#"You are handling Data Link layer (Layer 2) packets via pcap.
You can capture and inject Ethernet frames, handle ARP requests/responses, and work with raw MAC addresses.
Common use cases: ARP spoofing detection, custom Ethernet protocols, network monitoring."#;

    let output_format = r#"IMPORTANT: Respond with a JSON object:
{
  "output": "Ethernet frame data as hex (null if no response to inject)",
  "message": null  // Optional message for user
}"#;

    (context, output_format)
}

/// Data Link layer server that captures and injects packets
pub struct DataLinkServer;

impl DataLinkServer {
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

    /// Spawn datalink server with integrated LLM handling (async wrapper for blocking pcap)
    pub async fn spawn_with_llm(
        interface: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        filter: Option<String>,
        server_id: crate::state::ServerId,
    ) -> Result<String> {
        info!("Starting packet capture on interface: {}", interface);

        let protocol = Arc::new(DataLinkProtocol::new());

        // Datalink/pcap is blocking, so we run it in a blocking task
        let interface_clone = interface.clone();
        let protocol_clone = protocol.clone();
        tokio::task::spawn_blocking(move || {
            // Find device
            let device = match Self::find_device(&interface_clone) {
                Ok(d) => d,
                Err(e) => {
                    error!("Failed to find device: {}", e);
                    return;
                }
            };

            // Open capture
            let mut cap = match Capture::from_device(device)
                .map(|c| c.promisc(true).snaplen(65535).timeout(1000))
                .and_then(|c| c.open())
            {
                Ok(c) => c,
                Err(e) => {
                    error!("Failed to open capture: {}", e);
                    return;
                }
            };

            // Apply filter if provided
            if let Some(ref filter_str) = filter {
                if let Err(e) = cap.filter(filter_str, true) {
                    error!("Failed to apply filter: {}", e);
                    return;
                }
            }

            let runtime = tokio::runtime::Handle::current();

            // Capture loop
            loop {
                match cap.next_packet() {
                    Ok(packet) => {
                        let data = Bytes::copy_from_slice(packet.data);

                        // DEBUG: Log summary
                        console_debug!(status_tx, "[DEBUG] Datalink received {} bytes", data.len());

                        // TRACE: Log full payload (always hex for datalink)
                        let hex_str = hex::encode(&data);
                        console_trace!(status_tx, "[TRACE] Datalink data (hex): {}", hex_str);

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_task_clone = protocol_clone.clone();

                        // Spawn async task to handle packet with LLM
                        runtime.spawn(async move {
                            // Build event data
                            let hex_str = hex::encode(&data);
                            let event = Event::new(&DATALINK_PACKET_CAPTURED_EVENT, serde_json::json!({
                                "packet_length": data.len(),
                                "packet_hex": hex_str
                            }));

                            debug!("Datalink calling LLM for packet ({} bytes)", data.len());
                            let _ = status_clone.send(format!("[DEBUG] Datalink calling LLM for packet ({} bytes)", data.len()));

                            match call_llm(
                                &llm_clone,
                                &state_clone,
                                server_id,
                                None,
                                &event,
                                protocol_task_clone.as_ref(),
                            ).await {
                                Ok(execution_result) => {
                                    for message in &execution_result.messages {
                                        info!("{}", message);
                                        let _ = status_clone.send(format!("[INFO] {}", message));
                                    }

                                    debug!("Datalink got {} protocol results", execution_result.protocol_results.len());
                                    let _ = status_clone.send(format!("[DEBUG] Datalink got {} protocol results", execution_result.protocol_results.len()));

                                    let _ = status_clone.send(format!(
                                        "→ Datalink packet processed: {} bytes",
                                        data.len()
                                    ));
                                }
                                Err(e) => {
                                    error!("Datalink LLM call failed: {}", e);
                                    let _ = status_clone.send(format!("✗ Datalink LLM error: {}", e));
                                }
                            }
                        });
                    }
                    Err(pcap::Error::TimeoutExpired) => {
                        // Normal timeout, continue
                        continue;
                    }
                    Err(e) => {
                        error!("Packet capture error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(interface)
    }
}

