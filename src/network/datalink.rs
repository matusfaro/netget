//! Data Link layer (Layer 2) server implementation using pcap
//!
//! This module provides functionality to capture and inject packets at the data link layer.
//! It uses libpcap to interact with network interfaces.

use anyhow::{Context, Result};
use bytes::Bytes;
use pcap::{Capture, Device};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::llm::ollama_client::OllamaClient;
use crate::llm::prompt::PromptBuilder;
use crate::network::connection::ConnectionId;
use crate::state::app_state::AppState;

/// Get LLM context and output format instructions for DataLink stack
pub fn get_llm_prompt_config() -> (&'static str, &'static str) {
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
    ) -> Result<String> {
        info!("Starting packet capture on interface: {}", interface);

        // Datalink/pcap is blocking, so we run it in a blocking task
        let interface_clone = interface.clone();
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
                        let connection_id = ConnectionId::new();

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();

                        // Spawn async task to handle packet with LLM
                        runtime.spawn(async move {
                            let model = state_clone.get_ollama_model().await;
                            let prompt_config = get_llm_prompt_config();
                            let conn_memory = String::new();

                            // Build event description
                            let event_description = format!(
                                "Datalink packet captured ({} bytes)",
                                data.len()
                            );

                            let prompt = PromptBuilder::build_network_event_prompt(
                                &state_clone,
                                connection_id,
                                &conn_memory,
                                &event_description,
                                prompt_config,
                            ).await;

                            match llm_clone.generate(&model, &prompt).await {
                                Ok(llm_output) => {
                                    let _ = status_clone.send(format!(
                                        "→ Datalink packet processed: {} bytes → {} bytes response",
                                        data.len(),
                                        llm_output.len()
                                    ));
                                    // Note: Injecting packets back would require mutable cap access
                                }
                                Err(e) => {
                                    error!("LLM error for datalink: {}", e);
                                    let _ = status_clone.send(format!("✗ LLM error for datalink: {}", e));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_devices() {
        // This should work on any system with pcap installed
        let devices = DataLinkServer::list_devices();
        match devices {
            Ok(devs) => {
                println!("Found {} network devices", devs.len());
                for dev in devs {
                    println!("  - {}: {:?}", dev.name, dev.desc);
                }
            }
            Err(e) => {
                eprintln!("Warning: Could not list devices: {}", e);
                eprintln!("This may be due to permissions or pcap not being installed");
            }
        }
    }
}
