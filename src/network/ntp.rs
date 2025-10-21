//! NTP server implementation

use crate::events::types::{AppEvent, NetworkEvent};
use crate::network::connection::ConnectionId;
use anyhow::Result;
use bytes::Bytes;
use std::net::SocketAddr;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::info;

/// NTP packet structure (simplified)
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct NtpPacket {
    pub leap_indicator: u8,
    pub version: u8,
    pub mode: u8,
    pub stratum: u8,
    pub poll_interval: u8,
    pub precision: i8,
    pub root_delay: u32,
    pub root_dispersion: u32,
    pub reference_id: u32,
    pub reference_timestamp: u64,
    pub origin_timestamp: u64,
    pub receive_timestamp: u64,
    pub transmit_timestamp: u64,
}

impl NtpPacket {
    /// Parse NTP packet from bytes
    fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 48 {
            return None;
        }

        let first_byte = data[0];
        Some(Self {
            leap_indicator: (first_byte >> 6) & 0x3,
            version: (first_byte >> 3) & 0x7,
            mode: first_byte & 0x7,
            stratum: data[1],
            poll_interval: data[2],
            precision: data[3] as i8,
            root_delay: u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
            root_dispersion: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
            reference_id: u32::from_be_bytes([data[12], data[13], data[14], data[15]]),
            reference_timestamp: u64::from_be_bytes([
                data[16], data[17], data[18], data[19], data[20], data[21], data[22], data[23],
            ]),
            origin_timestamp: u64::from_be_bytes([
                data[24], data[25], data[26], data[27], data[28], data[29], data[30], data[31],
            ]),
            receive_timestamp: u64::from_be_bytes([
                data[32], data[33], data[34], data[35], data[36], data[37], data[38], data[39],
            ]),
            transmit_timestamp: u64::from_be_bytes([
                data[40], data[41], data[42], data[43], data[44], data[45], data[46], data[47],
            ]),
        })
    }
}

/// NTP server that forwards requests to LLM
pub struct NtpServer {
    addr: SocketAddr,
    event_tx: mpsc::UnboundedSender<AppEvent>,
}

impl NtpServer {
    /// Create a new NTP server
    pub async fn new(
        addr: SocketAddr,
        event_tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Result<Self> {
        Ok(Self { addr, event_tx })
    }

    /// Start the NTP server
    pub async fn start(self) -> Result<()> {
        // NTP typically uses port 123
        let socket = UdpSocket::bind(self.addr).await?;
        info!("NTP server listening on {}", socket.local_addr()?);

        // Send listening event
        self.event_tx.send(AppEvent::Network(NetworkEvent::Listening {
            addr: socket.local_addr()?,
        }))?;

        let mut buffer = vec![0u8; 128]; // NTP packets are typically 48 bytes

        loop {
            match socket.recv_from(&mut buffer).await {
                Ok((n, peer_addr)) => {
                    // Create connection ID for this NTP request
                    let connection_id = ConnectionId::new();

                    // Send connection event
                    let _ = self.event_tx.send(AppEvent::Network(NetworkEvent::Connected {
                        connection_id,
                        remote_addr: peer_addr,
                    }));

                    // Parse NTP packet if possible
                    let ntp_info = if let Some(packet) = NtpPacket::from_bytes(&buffer[..n]) {
                        format!(
                            "NTP Request: Version={}, Mode={}, Stratum={}",
                            packet.version,
                            match packet.mode {
                                1 => "symmetric active",
                                2 => "symmetric passive",
                                3 => "client",
                                4 => "server",
                                5 => "broadcast",
                                6 => "control",
                                7 => "reserved",
                                _ => "unknown",
                            },
                            packet.stratum
                        )
                    } else {
                        format!("NTP Request: {} bytes (unparseable)", n)
                    };

                    // Send data received event
                    let _ = self
                        .event_tx
                        .send(AppEvent::Network(NetworkEvent::DataReceived {
                            connection_id,
                            data: Bytes::from(ntp_info),
                        }));
                }
                Err(e) => {
                    tracing::error!("NTP receive error: {}", e);
                }
            }
        }
    }
}