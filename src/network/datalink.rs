//! Data Link layer (Layer 2) server implementation using pcap
//!
//! This module provides functionality to capture and inject packets at the data link layer.
//! It uses libpcap to interact with network interfaces.

use anyhow::{Context, Result};
use bytes::Bytes;
use pcap::{Active, Capture, Device};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::events::types::NetworkEvent;

/// Data Link layer server that captures and injects packets
pub struct DataLinkServer {
    /// The network interface name
    interface: String,
    /// Event sender
    event_tx: mpsc::UnboundedSender<NetworkEvent>,
    /// Pcap capture handle (optional, set when listening)
    capture: Option<Capture<Active>>,
}

impl DataLinkServer {
    /// Create a new DataLink server for the specified interface
    pub fn new(interface: String, event_tx: mpsc::UnboundedSender<NetworkEvent>) -> Self {
        Self {
            interface,
            event_tx,
            capture: None,
        }
    }

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

    /// Start capturing packets on the interface
    pub fn start_capture(&mut self, filter: Option<&str>) -> Result<()> {
        info!("Starting packet capture on interface: {}", self.interface);

        // Find the device
        let device = Self::find_device(&self.interface)?;

        // Open the device
        let mut cap = Capture::from_device(device)
            .context("Failed to create capture from device")?
            .promisc(true) // Enable promiscuous mode
            .snaplen(65535) // Capture full packets
            .timeout(1000) // 1 second timeout for next_packet()
            .open()
            .context("Failed to open capture")?;

        // Apply filter if provided
        if let Some(filter_str) = filter {
            info!("Applying BPF filter: {}", filter_str);
            cap.filter(filter_str, true)
                .context("Failed to set packet filter")?;
        }

        info!(
            "Started capturing on {} in promiscuous mode",
            self.interface
        );

        self.capture = Some(cap);

        // Send listening event (using interface as the "address")
        let _ = self.event_tx.send(NetworkEvent::Listening {
            addr: format!("{}:0", self.interface).parse().unwrap_or_else(|_| {
                // Fallback to localhost if interface name can't be parsed as address
                "127.0.0.1:0".parse().unwrap()
            }),
        });

        Ok(())
    }

    /// Capture the next packet from the interface
    /// Returns None if timeout occurs without a packet
    pub fn next_packet(&mut self) -> Result<Option<Bytes>> {
        if let Some(cap) = &mut self.capture {
            match cap.next_packet() {
                Ok(packet) => {
                    let data = Bytes::copy_from_slice(packet.data);
                    debug!("Captured packet: {} bytes", data.len());
                    Ok(Some(data))
                }
                Err(pcap::Error::TimeoutExpired) => {
                    // This is normal - just means no packet arrived within timeout
                    Ok(None)
                }
                Err(e) => {
                    error!("Error capturing packet: {}", e);
                    Err(e.into())
                }
            }
        } else {
            Err(anyhow::anyhow!("Capture not started"))
        }
    }

    /// Inject a raw packet onto the interface
    pub fn inject_packet(&mut self, data: &[u8]) -> Result<()> {
        if let Some(cap) = &mut self.capture {
            debug!("Injecting packet: {} bytes", data.len());
            cap.sendpacket(data)
                .context("Failed to inject packet")?;
            info!("Successfully injected {} byte packet", data.len());
            Ok(())
        } else {
            Err(anyhow::anyhow!("Capture not started, cannot inject"))
        }
    }

    /// Get the interface name
    pub fn interface(&self) -> &str {
        &self.interface
    }

    /// Check if currently capturing
    pub fn is_capturing(&self) -> bool {
        self.capture.is_some()
    }

    /// Stop capturing
    pub fn stop_capture(&mut self) {
        if self.capture.is_some() {
            info!("Stopping packet capture on {}", self.interface);
            self.capture = None;
        }
    }

    /// Run the capture loop, sending packets to the event channel
    /// This is a blocking operation and should be run in a separate task
    pub fn run_capture_loop(mut self) -> Result<()> {
        info!("Starting capture loop on {}", self.interface);

        loop {
            match self.next_packet() {
                Ok(Some(data)) => {
                    // Send packet received event
                    let _ = self.event_tx.send(NetworkEvent::PacketReceived {
                        interface: self.interface.clone(),
                        data,
                    });
                }
                Ok(None) => {
                    // Timeout - continue loop
                    continue;
                }
                Err(e) => {
                    error!("Error in capture loop: {}", e);
                    let _ = self.event_tx.send(NetworkEvent::Error {
                        connection_id: None,
                        error: e.to_string(),
                    });
                    break;
                }
            }
        }

        Ok(())
    }
}

impl Drop for DataLinkServer {
    fn drop(&mut self) {
        self.stop_capture();
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
