//! OpenVPN honeypot implementation
//!
//! This is an OpenVPN *honeypot* that detects and logs OpenVPN connection attempts.
//! It does NOT implement full OpenVPN crypto (avoiding dependency conflicts).
//!
//! The LLM controls:
//! - Connection authentication decisions (log reconnaissance attempts)
//! - Response behavior (accept/reject/silent-drop)
//! - Traffic pattern analysis
//! - Honeypot simulation behavior

pub mod actions;

use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use actions::OPENVPN_HANDSHAKE_EVENT;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

/// Maximum OpenVPN packet size
const MAX_PACKET_SIZE: usize = 65535;

/// OpenVPN packet opcodes (from protocol spec)
/// Opcode is stored in the upper 5 bits of the first byte
const P_CONTROL_HARD_RESET_CLIENT_V1: u8 = 1;
const P_CONTROL_HARD_RESET_SERVER_V1: u8 = 2;
const P_CONTROL_SOFT_RESET_V1: u8 = 3;
const P_CONTROL_V1: u8 = 4;
const P_ACK_V1: u8 = 5;
const P_DATA_V1: u8 = 6;
const P_CONTROL_HARD_RESET_CLIENT_V2: u8 = 7;
const P_CONTROL_HARD_RESET_SERVER_V2: u8 = 8;
const P_DATA_V2: u8 = 9;

/// OpenVPN honeypot server
pub struct OpenvpnServer;

impl OpenvpnServer {
    /// Spawn OpenVPN honeypot with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        bind_addr: SocketAddr,
        llm_client: Arc<OllamaClient>,
        app_state: Arc<AppState>,
        _server_id: crate::state::ServerId,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<SocketAddr> {
        info!("Starting OpenVPN honeypot on {}", bind_addr);
        let _ = status_tx.send(format!(
            "[INFO] Starting OpenVPN honeypot on {} (reconnaissance detection only)",
            bind_addr
        ));

        // Bind UDP socket (OpenVPN can also run on TCP, but UDP is more common)
        let socket = UdpSocket::bind(bind_addr).await?;
        let local_addr = socket.local_addr()?;
        info!("OpenVPN honeypot listening on {}", local_addr);
        let _ = status_tx.send(format!(
            "[INFO] OpenVPN honeypot listening on {}",
            local_addr
        ));

        let socket = Arc::new(socket);

        // Spawn packet handler
        let socket_clone = socket.clone();
        tokio::spawn(async move {
            if let Err(e) =
                Self::handle_packets(socket_clone, llm_client, app_state, status_tx).await
            {
                error!("OpenVPN honeypot error: {}", e);
            }
        });

        Ok(local_addr)
    }

    /// Handle incoming OpenVPN packets
    async fn handle_packets(
        socket: Arc<UdpSocket>,
        llm_client: Arc<OllamaClient>,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let mut buf = vec![0u8; MAX_PACKET_SIZE];

        loop {
            // Receive packet
            let (len, peer_addr) = match socket.recv_from(&mut buf).await {
                Ok(result) => result,
                Err(e) => {
                    error!("UDP recv error: {}", e);
                    continue;
                }
            };

            let packet = &buf[..len];

            // Parse OpenVPN packet header
            if len == 0 {
                trace!("Received empty packet from {}", peer_addr);
                continue;
            }

            // Extract opcode (upper 5 bits) and key_id (lower 3 bits)
            let opcode = (packet[0] >> 3) & 0x1F;
            let key_id = packet[0] & 0x07;

            let (packet_type, is_handshake) = match opcode {
                P_CONTROL_HARD_RESET_CLIENT_V1 => ("ControlHardResetClientV1", true),
                P_CONTROL_HARD_RESET_SERVER_V1 => ("ControlHardResetServerV1", false),
                P_CONTROL_SOFT_RESET_V1 => ("ControlSoftResetV1", false),
                P_CONTROL_V1 => ("ControlV1", false),
                P_ACK_V1 => ("AckV1", false),
                P_DATA_V1 => ("DataV1", false),
                P_CONTROL_HARD_RESET_CLIENT_V2 => ("ControlHardResetClientV2", true),
                P_CONTROL_HARD_RESET_SERVER_V2 => ("ControlHardResetServerV2", false),
                P_DATA_V2 => ("DataV2", false),
                _ => ("Unknown", false),
            };

            trace!(
                "OpenVPN packet from {}: type={} (opcode={}), key_id={}, {} bytes",
                peer_addr,
                packet_type,
                opcode,
                key_id,
                len
            );
            let _ = status_tx.send(format!(
                "[TRACE] OpenVPN: {} packet from {} ({} bytes)",
                packet_type, peer_addr, len
            ));

            // For handshake initiation, ask LLM for honeypot decision
            if is_handshake {
                Self::handle_handshake_initiation(
                    peer_addr,
                    packet,
                    opcode,
                    key_id,
                    &socket,
                    &llm_client,
                    &app_state,
                    &status_tx,
                )
                .await;
            } else {
                // Log other packet types for reconnaissance detection
                debug!(
                    "OpenVPN {} packet from {} (honeypot: logged only)",
                    packet_type, peer_addr
                );
                let _ = status_tx.send(format!(
                    "[DEBUG] OpenVPN: {} from {} (logged)",
                    packet_type, peer_addr
                ));
            }
        }
    }

    /// Handle handshake initiation - honeypot intelligence
    async fn handle_handshake_initiation(
        peer_addr: SocketAddr,
        packet: &[u8],
        opcode: u8,
        key_id: u8,
        socket: &UdpSocket,
        llm_client: &OllamaClient,
        app_state: &AppState,
        status_tx: &mpsc::UnboundedSender<String>,
    ) {
        info!(
            "OpenVPN handshake attempt from {} (honeypot)",
            peer_addr
        );
        let _ = status_tx.send(format!(
            "[INFO] OpenVPN: Handshake reconnaissance from {}",
            peer_addr
        ));

        // Extract session ID if packet is long enough (v2 packets have 8-byte session ID)
        let session_id = if opcode == P_CONTROL_HARD_RESET_CLIENT_V2 && packet.len() >= 9 {
            Some(format!(
                "{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                packet[1], packet[2], packet[3], packet[4], packet[5], packet[6], packet[7],
                packet[8]
            ))
        } else {
            None
        };

        // Build event for LLM
        let event = Event::new(
            &OPENVPN_HANDSHAKE_EVENT,
            serde_json::json!({
                "peer_addr": peer_addr.to_string(),
                "packet_size": packet.len(),
                "packet_type": if opcode == P_CONTROL_HARD_RESET_CLIENT_V2 {
                    "ControlHardResetClientV2"
                } else {
                    "ControlHardResetClientV1"
                },
                "opcode": opcode,
                "key_id": key_id,
                "session_id": session_id,
                "honeypot_mode": true,
            }),
        );

        // TODO: Call LLM for VPN peer authorization decision when full server is implemented
        // For now, just log the handshake detection
        debug!("OpenVPN handshake detected from {} (honeypot mode)", peer_addr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_packet_opcode_constants() {
        assert_eq!(P_CONTROL_HARD_RESET_CLIENT_V1, 1);
        assert_eq!(P_CONTROL_HARD_RESET_SERVER_V1, 2);
        assert_eq!(P_CONTROL_SOFT_RESET_V1, 3);
        assert_eq!(P_CONTROL_V1, 4);
        assert_eq!(P_ACK_V1, 5);
        assert_eq!(P_DATA_V1, 6);
        assert_eq!(P_CONTROL_HARD_RESET_CLIENT_V2, 7);
        assert_eq!(P_CONTROL_HARD_RESET_SERVER_V2, 8);
        assert_eq!(P_DATA_V2, 9);
    }

    #[test]
    fn test_opcode_extraction() {
        // Opcode is in upper 5 bits
        let byte = 0b00111_000; // opcode=7 (HARD_RESET_CLIENT_V2), key_id=0
        let opcode = (byte >> 3) & 0x1F;
        let key_id = byte & 0x07;
        assert_eq!(opcode, 7);
        assert_eq!(key_id, 0);
    }
}
