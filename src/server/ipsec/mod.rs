//! IPSec/IKEv2 VPN honeypot implementation
//!
//! This is an IPSec/IKEv2 *honeypot* that detects and logs IKE connection attempts.
//! It does NOT implement full IPSec crypto (avoiding dependency conflicts).
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
use actions::IPSEC_HANDSHAKE_EVENT;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

/// Maximum IKE packet size
const MAX_PACKET_SIZE: usize = 65535;

/// IKEv2 header minimum size (28 bytes)
const IKE_HEADER_SIZE: usize = 28;

/// IKEv2 version (major=2, minor=0)
const IKEV2_VERSION: u8 = 0x20;

/// IKEv2 Exchange Types (from RFC 7296)
const IKE_SA_INIT: u8 = 34;
const IKE_AUTH: u8 = 35;
const CREATE_CHILD_SA: u8 = 36;
const INFORMATIONAL: u8 = 37;

/// IKEv1 Exchange Types (for detection)
const IKEV1_IDENTITY_PROTECTION: u8 = 2;
const IKEV1_AGGRESSIVE: u8 = 4;

/// IPSec/IKEv2 honeypot server
pub struct IpsecServer;

impl IpsecServer {
    /// Spawn IPSec/IKEv2 honeypot with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        bind_addr: SocketAddr,
        llm_client: Arc<OllamaClient>,
        app_state: Arc<AppState>,
        _server_id: crate::state::ServerId,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<SocketAddr> {
        info!("Starting IPSec/IKEv2 honeypot on {}", bind_addr);
        let _ = status_tx.send(format!(
            "[INFO] Starting IPSec/IKEv2 honeypot on {} (reconnaissance detection only)",
            bind_addr
        ));

        // Bind UDP socket (IKE uses UDP port 500, NAT-T uses 4500)
        let socket = UdpSocket::bind(bind_addr).await?;
        let local_addr = socket.local_addr()?;
        info!("IPSec/IKEv2 honeypot listening on {}", local_addr);
        let _ = status_tx.send(format!(
            "[INFO] IPSec/IKEv2 honeypot listening on {}",
            local_addr
        ));

        let socket = Arc::new(socket);

        // Spawn packet handler
        let socket_clone = socket.clone();
        tokio::spawn(async move {
            if let Err(e) =
                Self::handle_packets(socket_clone, llm_client, app_state, status_tx).await
            {
                error!("IPSec/IKEv2 honeypot error: {}", e);
            }
        });

        Ok(local_addr)
    }

    /// Handle incoming IKE packets
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

            // Parse IKE header
            if len < IKE_HEADER_SIZE {
                trace!("Received undersized packet from {} ({} bytes)", peer_addr, len);
                continue;
            }

            // Extract IKE header fields
            let initiator_spi = u64::from_be_bytes([
                packet[0], packet[1], packet[2], packet[3],
                packet[4], packet[5], packet[6], packet[7],
            ]);
            let responder_spi = u64::from_be_bytes([
                packet[8], packet[9], packet[10], packet[11],
                packet[12], packet[13], packet[14], packet[15],
            ]);
            let _next_payload = packet[16];
            let version = packet[17];
            let exchange_type = packet[18];
            let flags = packet[19];
            let message_id = u32::from_be_bytes([packet[20], packet[21], packet[22], packet[23]]);
            let packet_length = u32::from_be_bytes([packet[24], packet[25], packet[26], packet[27]]);

            // Determine IKE version and exchange type
            let (ike_version, exchange_name, is_handshake) = if version == IKEV2_VERSION {
                let (name, handshake) = match exchange_type {
                    IKE_SA_INIT => ("IKE_SA_INIT", true),
                    IKE_AUTH => ("IKE_AUTH", true),
                    CREATE_CHILD_SA => ("CREATE_CHILD_SA", false),
                    INFORMATIONAL => ("INFORMATIONAL", false),
                    _ => ("Unknown", false),
                };
                ("IKEv2", name, handshake)
            } else {
                let (name, handshake) = match exchange_type {
                    IKEV1_IDENTITY_PROTECTION => ("Identity Protection", true),
                    IKEV1_AGGRESSIVE => ("Aggressive Mode", true),
                    _ => ("Unknown", false),
                };
                ("IKEv1", name, handshake)
            };

            trace!(
                "IKE packet from {}: version={}, exchange={}, flags=0x{:02x}, msg_id={}, len={}",
                peer_addr,
                ike_version,
                exchange_name,
                flags,
                message_id,
                packet_length
            );
            let _ = status_tx.send(format!(
                "[TRACE] IPSec: {} {} from {} ({} bytes)",
                ike_version, exchange_name, peer_addr, len
            ));

            // For handshake initiation, ask LLM for honeypot decision
            if is_handshake {
                Self::handle_handshake_initiation(
                    peer_addr,
                    packet,
                    ike_version,
                    exchange_name,
                    initiator_spi,
                    responder_spi,
                    &socket,
                    &llm_client,
                    &app_state,
                    &status_tx,
                )
                .await;
            } else {
                // Log other packet types for reconnaissance detection
                debug!(
                    "IPSec {} {} from {} (honeypot: logged only)",
                    ike_version, exchange_name, peer_addr
                );
                let _ = status_tx.send(format!(
                    "[DEBUG] IPSec: {} {} from {} (logged)",
                    ike_version, exchange_name, peer_addr
                ));
            }
        }
    }

    /// Handle handshake initiation - honeypot intelligence
    async fn handle_handshake_initiation(
        peer_addr: SocketAddr,
        packet: &[u8],
        ike_version: &str,
        exchange_type: &str,
        initiator_spi: u64,
        responder_spi: u64,
        _socket: &UdpSocket,
        _llm_client: &OllamaClient,
        _app_state: &AppState,
        status_tx: &mpsc::UnboundedSender<String>,
    ) {
        info!(
            "IPSec {} handshake attempt from {} (honeypot)",
            ike_version, peer_addr
        );
        let _ = status_tx.send(format!(
            "[INFO] IPSec: {} handshake reconnaissance from {}",
            ike_version, peer_addr
        ));

        // Build event for LLM
        let _event = Event::new(
            &IPSEC_HANDSHAKE_EVENT,
            serde_json::json!({
                "peer_addr": peer_addr.to_string(),
                "packet_size": packet.len(),
                "ike_version": ike_version,
                "exchange_type": exchange_type,
                "initiator_spi": format!("{:016x}", initiator_spi),
                "responder_spi": format!("{:016x}", responder_spi),
                "honeypot_mode": true,
            }),
        );

        // TODO: Call LLM for VPN peer authorization decision when full server is implemented
        // For now, just log the handshake detection
        debug!("IPSec/IKEv2 handshake detected from {} (honeypot mode)", peer_addr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ike_constants() {
        assert_eq!(IKEV2_VERSION, 0x20);
        assert_eq!(IKE_SA_INIT, 34);
        assert_eq!(IKE_AUTH, 35);
        assert_eq!(CREATE_CHILD_SA, 36);
        assert_eq!(INFORMATIONAL, 37);
    }

    #[test]
    fn test_ike_header_size() {
        assert_eq!(IKE_HEADER_SIZE, 28);
    }
}
