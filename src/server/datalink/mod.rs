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
use tracing::{debug, error, info};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::{console_debug, console_error, console_info, console_trace};
use actions::{DataLinkProtocol, DATALINK_PACKET_CAPTURED_EVENT};

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
        console_info!(
            status_tx,
            "Starting packet capture on interface: {}",
            interface
        );

        // Retained by this function; the capture task takes ownership of `status_tx`.
        let status_tx_ready = status_tx.clone();

        let protocol = Arc::new(DataLinkProtocol::new());

        // Datalink/pcap is blocking, so we run it in a blocking task.
        //
        // Opening the pcap handle needs privileges (root, or read access to /dev/bpf* on
        // macOS/BSD, or CAP_NET_RAW on Linux) and a valid BPF filter. Both are reported back
        // over a oneshot so `spawn_with_llm` can return Err and the server is marked Error,
        // rather than reporting Running while capturing nothing.
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<Result<()>>();

        let interface_clone = interface.clone();
        let protocol_clone = protocol.clone();
        tokio::task::spawn_blocking(move || {
            let open_capture = || -> Result<pcap::Capture<pcap::Active>> {
                let device = Self::find_device(&interface_clone)
                    .with_context(|| format!("no such capture device '{}'", interface_clone))?;

                let mut cap = Capture::from_device(device)
                    .map(|c| c.promisc(true).snaplen(65535).timeout(1000))
                    .and_then(|c| c.open())
                    .with_context(|| {
                        format!(
                            "failed to open pcap capture on '{}' (needs root, or \
                             read access to /dev/bpf* on macOS/BSD, or CAP_NET_RAW on Linux)",
                            interface_clone
                        )
                    })?;

                // Apply filter if provided
                if let Some(ref filter_str) = filter {
                    cap.filter(filter_str, true)
                        .with_context(|| format!("invalid BPF filter '{}'", filter_str))?;
                }

                Ok(cap)
            };

            let mut cap = match open_capture() {
                Ok(cap) => {
                    let _ = ready_tx.send(Ok(()));
                    cap
                }
                Err(e) => {
                    console_error!(status_tx, "DataLink capture startup failed: {:#}", e);
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };

            let runtime = tokio::runtime::Handle::current();

            // Capture loop
            loop {
                match cap.next_packet() {
                    Ok(packet) => {
                        let data = Bytes::copy_from_slice(packet.data);

                        // DEBUG: Log summary
                        console_debug!(status_tx, "Datalink received {} bytes", data.len());

                        // TRACE: Log full payload (always hex for datalink)
                        let hex_str = hex::encode(&data);
                        console_trace!(status_tx, "Datalink data (hex): {}", hex_str);

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_task_clone = protocol_clone.clone();

                        // Spawn async task to handle packet with LLM
                        runtime.spawn(async move {
                            // Build event data
                            let hex_str = hex::encode(&data);
                            let event = Event::new(
                                &DATALINK_PACKET_CAPTURED_EVENT,
                                serde_json::json!({
                                    "packet_length": data.len(),
                                    "packet_hex": hex_str
                                }),
                            );

                            debug!("Datalink calling LLM for packet ({} bytes)", data.len());
                            let _ = status_clone.send(format!(
                                "[DEBUG] Datalink calling LLM for packet ({} bytes)",
                                data.len()
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
                                        "Datalink got {} protocol results",
                                        execution_result.protocol_results.len()
                                    );
                                    let _ = status_clone.send(format!(
                                        "[DEBUG] Datalink got {} protocol results",
                                        execution_result.protocol_results.len()
                                    ));

                                    let _ = status_clone.send(format!(
                                        "→ Datalink packet processed: {} bytes",
                                        data.len()
                                    ));
                                }
                                Err(e) => {
                                    error!("Datalink LLM call failed: {}", e);
                                    let _ =
                                        status_clone.send(format!("✗ Datalink LLM error: {}", e));
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

        // Wait for the blocking task to report whether the capture actually came up.
        match ready_rx.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                return Err(anyhow::anyhow!(
                    "DataLink capture task on '{}' exited before signalling readiness",
                    interface
                ))
            }
        }

        console_info!(status_tx_ready, "DataLink capture active on {}", interface);

        Ok(interface)
    }
}
