//! TUN/TAP device management for VPN protocols
//!
//! This module provides shared infrastructure for creating and managing TUN/TAP
//! virtual network interfaces used by VPN protocols like WireGuard, OpenVPN, etc.
//!
//! IMPORTANT: Creating TUN/TAP devices requires elevated privileges (root/CAP_NET_ADMIN)

use std::io;
use std::net::IpAddr;
use tokio_tun::Tun;
use tracing::{debug, error, info};

/// TUN device manager
///
/// Manages a TUN (layer 3) virtual network interface for VPN tunneling.
/// Requires root privileges or CAP_NET_ADMIN capability.
pub struct TunManager {
    /// The TUN device
    pub device: Tun,
    /// MTU (Maximum Transmission Unit) size
    pub mtu: usize,
    /// IP address assigned to the TUN interface
    pub address: Option<IpAddr>,
    /// Device name (e.g., "tun0")
    pub name: String,
}

impl TunManager {
    /// Create a new TUN device
    ///
    /// # Arguments
    /// * `name` - Device name (e.g., "wg0", "tun0"). If None, OS assigns automatically.
    /// * `mtu` - Maximum Transmission Unit size (typically 1420 for WireGuard, 1500 for others)
    /// * `address` - Optional IP address to assign to the interface
    ///
    /// # Errors
    /// Returns error if:
    /// - Insufficient privileges (not root/CAP_NET_ADMIN)
    /// - Device name already in use
    /// - System doesn't support TUN devices
    ///
    /// # Example
    /// ```ignore
    /// let tun = TunManager::create(Some("wg0"), 1420, None).await?;
    /// ```
    pub async fn create(
        name: Option<&str>,
        mtu: usize,
        address: Option<IpAddr>,
    ) -> io::Result<Self> {
        info!(
            "Creating TUN device: name={:?}, mtu={}, addr={:?}",
            name, mtu, address
        );

        // Create TUN device using tokio-tun
        let mut builder = Tun::builder();

        if let Some(dev_name) = name {
            builder.name(dev_name);
        }

        // Set TUN mode (layer 3)
        builder.packet_info(false);

        let device = builder.try_build().map_err(|e| {
            error!("Failed to create TUN device: {}", e);
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "TUN device creation failed: {}. Are you running as root?",
                    e
                ),
            )
        })?;

        let actual_name = device.name().unwrap_or("unknown").to_string();

        debug!("TUN device created: {}", actual_name);

        // Note: Setting IP address and bringing interface up requires system commands
        // This is platform-specific and should be done via ip/ifconfig commands
        if address.is_some() {
            info!(
                "Note: IP address configuration requires manual setup via 'ip addr add' or similar"
            );
        }

        Ok(Self {
            device,
            mtu,
            address,
            name: actual_name,
        })
    }

    /// Get the device name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the MTU
    pub fn mtu(&self) -> usize {
        self.mtu
    }

    /// Get the assigned IP address
    pub fn address(&self) -> Option<IpAddr> {
        self.address
    }

    /// Read a packet from the TUN device
    ///
    /// Returns the packet data as a Vec<u8>
    pub async fn read_packet(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        use tokio::io::AsyncReadExt;
        self.device.read(buf).await
    }

    /// Write a packet to the TUN device
    ///
    /// # Arguments
    /// * `packet` - The IP packet to write to the tunnel
    pub async fn write_packet(&mut self, packet: &[u8]) -> io::Result<usize> {
        use tokio::io::AsyncWriteExt;
        self.device.write(packet).await
    }
}

impl Drop for TunManager {
    fn drop(&mut self) {
        debug!("Closing TUN device: {}", self.name);
    }
}
