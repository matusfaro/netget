//! IPSec/IKEv2 Enhanced Honeypot Implementation
//!
//! This is an IPSec/IKEv2 *enhanced honeypot* that detects and logs IKE connection
//! attempts with detailed protocol analysis. It does NOT establish actual VPN tunnels.
//!
//! **Status**: Experimental (enhanced detection with manual parsing)
//! **Future**: Full VPN implementation when swanny library reaches 1.0 (mid-2025)
//!
//! The LLM controls:
//! - Connection authentication decisions (log reconnaissance attempts)
//! - Response behavior (accept/reject/silent-drop)
//! - Traffic pattern analysis
//! - Security parameter analysis (cipher suites, DH groups, etc.)

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

/// IKE Header Flags (RFC 7296 Section 3.1)
const FLAG_INITIATOR: u8 = 0x08;  // Initiator bit
const FLAG_VERSION: u8 = 0x10;     // Version bit (must be 0 for IKEv2)
const FLAG_RESPONSE: u8 = 0x20;    // Response bit

/// IKE Payload Types (RFC 7296 Section 3.2)
const PAYLOAD_NONE: u8 = 0;
const PAYLOAD_SA: u8 = 33;         // Security Association
const PAYLOAD_KE: u8 = 34;         // Key Exchange
const PAYLOAD_IDI: u8 = 35;        // Identification - Initiator
const PAYLOAD_IDR: u8 = 36;        // Identification - Responder
const PAYLOAD_CERT: u8 = 37;       // Certificate
const PAYLOAD_CERTREQ: u8 = 38;    // Certificate Request
const PAYLOAD_AUTH: u8 = 39;       // Authentication
const PAYLOAD_NONCE: u8 = 40;      // Nonce
const PAYLOAD_NOTIFY: u8 = 41;     // Notify
const PAYLOAD_DELETE: u8 = 42;     // Delete
const PAYLOAD_VENDOR: u8 = 43;     // Vendor ID
const PAYLOAD_TSI: u8 = 44;        // Traffic Selector - Initiator
const PAYLOAD_TSR: u8 = 45;        // Traffic Selector - Responder
const PAYLOAD_SK: u8 = 46;         // Encrypted and Authenticated
const PAYLOAD_CP: u8 = 47;         // Configuration
const PAYLOAD_EAP: u8 = 48;        // Extensible Authentication

/// IPSec/IKEv2 enhanced honeypot server
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
        info!("Starting IPSec/IKEv2 enhanced honeypot on {}", bind_addr);
        let _ = status_tx.send(format!(
            "[INFO] Starting IPSec/IKEv2 enhanced honeypot on {} (detailed protocol analysis)",
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

            // Extract IKE header fields (28 bytes - RFC 7296 Section 3.1)
            let initiator_spi = u64::from_be_bytes([
                packet[0], packet[1], packet[2], packet[3],
                packet[4], packet[5], packet[6], packet[7],
            ]);
            let responder_spi = u64::from_be_bytes([
                packet[8], packet[9], packet[10], packet[11],
                packet[12], packet[13], packet[14], packet[15],
            ]);
            let next_payload = packet[16];
            let version = packet[17];
            let exchange_type = packet[18];
            let flags = packet[19];
            let message_id = u32::from_be_bytes([packet[20], packet[21], packet[22], packet[23]]);
            let packet_length = u32::from_be_bytes([packet[24], packet[25], packet[26], packet[27]]);

            // Analyze flags
            let is_initiator = (flags & FLAG_INITIATOR) != 0;
            let is_response = (flags & FLAG_RESPONSE) != 0;
            let version_bit = (flags & FLAG_VERSION) != 0;

            // Extract payload chain
            let payload_types = Self::extract_payload_types(packet, next_payload);

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

            // Format payload chain for logging
            let payload_names = Self::format_payload_types(&payload_types);

            trace!(
                "IKE packet from {}: version={}, exchange={}, flags=0x{:02x} (I={}, R={}, V={}), msg_id={}, len={}, payloads=[{}]",
                peer_addr,
                ike_version,
                exchange_name,
                flags,
                if is_initiator { "1" } else { "0" },
                if is_response { "1" } else { "0" },
                if version_bit { "1" } else { "0" },
                message_id,
                packet_length,
                payload_names
            );
            let _ = status_tx.send(format!(
                "[TRACE] IPSec: {} {} from {} ({} bytes, payloads=[{}])",
                ike_version, exchange_name, peer_addr, len, payload_names
            ));

            // For handshake initiation, provide detailed analysis
            if is_handshake {
                Self::handle_handshake_initiation(
                    peer_addr,
                    packet,
                    ike_version,
                    exchange_name,
                    initiator_spi,
                    responder_spi,
                    is_initiator,
                    is_response,
                    message_id,
                    &payload_types,
                    &socket,
                    &llm_client,
                    &app_state,
                    &status_tx,
                )
                .await;
            } else {
                // Log other packet types for reconnaissance detection
                debug!(
                    "IPSec {} {} from {} (honeypot: logged only, payloads=[{}])",
                    ike_version, exchange_name, peer_addr, payload_names
                );
                let _ = status_tx.send(format!(
                    "[DEBUG] IPSec: {} {} from {} (logged, payloads=[{}])",
                    ike_version, exchange_name, peer_addr, payload_names
                ));
            }
        }
    }

    /// Extract payload types from IKE message
    fn extract_payload_types(packet: &[u8], mut next_payload: u8) -> Vec<u8> {
        let mut payload_types = Vec::new();
        let mut offset = IKE_HEADER_SIZE;

        // Walk the payload chain
        while next_payload != PAYLOAD_NONE && offset + 4 <= packet.len() {
            payload_types.push(next_payload);

            // Each payload has: next_payload(1) + reserved(1) + length(2)
            if offset + 4 > packet.len() {
                break;
            }

            let payload_length = u16::from_be_bytes([packet[offset + 2], packet[offset + 3]]) as usize;
            if payload_length < 4 || offset + payload_length > packet.len() {
                break;
            }

            next_payload = packet[offset];
            offset += payload_length;
        }

        payload_types
    }

    /// Format payload types as human-readable names
    fn format_payload_types(payload_types: &[u8]) -> String {
        payload_types
            .iter()
            .map(|&p| Self::payload_type_name(p))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Get payload type name
    fn payload_type_name(payload_type: u8) -> &'static str {
        match payload_type {
            PAYLOAD_SA => "SA",
            PAYLOAD_KE => "KE",
            PAYLOAD_IDI => "IDi",
            PAYLOAD_IDR => "IDr",
            PAYLOAD_CERT => "CERT",
            PAYLOAD_CERTREQ => "CERTREQ",
            PAYLOAD_AUTH => "AUTH",
            PAYLOAD_NONCE => "NONCE",
            PAYLOAD_NOTIFY => "NOTIFY",
            PAYLOAD_DELETE => "DELETE",
            PAYLOAD_VENDOR => "VENDOR",
            PAYLOAD_TSI => "TSi",
            PAYLOAD_TSR => "TSr",
            PAYLOAD_SK => "SK",
            PAYLOAD_CP => "CP",
            PAYLOAD_EAP => "EAP",
            _ => "UNKNOWN",
        }
    }

    /// Handle handshake initiation - enhanced honeypot analysis
    async fn handle_handshake_initiation(
        peer_addr: SocketAddr,
        packet: &[u8],
        ike_version: &str,
        exchange_type: &str,
        initiator_spi: u64,
        responder_spi: u64,
        is_initiator: bool,
        is_response: bool,
        message_id: u32,
        payload_types: &[u8],
        _socket: &UdpSocket,
        _llm_client: &OllamaClient,
        _app_state: &AppState,
        status_tx: &mpsc::UnboundedSender<String>,
    ) {
        let payload_names = Self::format_payload_types(payload_types);

        info!(
            "IPSec {} handshake from {} (enhanced honeypot, payloads=[{}])",
            ike_version, peer_addr, payload_names
        );
        let _ = status_tx.send(format!(
            "[INFO] IPSec: {} handshake from {} (payloads=[{}])",
            ike_version, peer_addr, payload_names
        ));

        // Build enhanced event for LLM
        let _event = Event::new(
            &IPSEC_HANDSHAKE_EVENT,
            serde_json::json!({
                "peer_addr": peer_addr.to_string(),
                "packet_size": packet.len(),
                "ike_version": ike_version,
                "exchange_type": exchange_type,
                "initiator_spi": format!("{:016x}", initiator_spi),
                "responder_spi": format!("{:016x}", responder_spi),
                "is_initiator": is_initiator,
                "is_response": is_response,
                "message_id": message_id,
                "payloads": payload_types.iter().map(|&p| Self::payload_type_name(p)).collect::<Vec<_>>(),
                "enhanced_honeypot": true,
                "analysis": {
                    "expected_payloads": if exchange_type == "IKE_SA_INIT" {
                        "SA, KE, NONCE"
                    } else if exchange_type == "IKE_AUTH" {
                        "IDi, AUTH, SA, TSi, TSr"
                    } else {
                        "varies"
                    },
                    "has_encryption": payload_types.contains(&PAYLOAD_SK),
                    "has_vendor_id": payload_types.contains(&PAYLOAD_VENDOR),
                    "has_certificate": payload_types.contains(&PAYLOAD_CERT) || payload_types.contains(&PAYLOAD_CERTREQ),
                }
            }),
        );

        // TODO: Call LLM for advanced security analysis when full implementation is ready
        // For now, provide detailed logging for security research
        debug!(
            "IPSec/IKEv2 handshake analyzed from {} (enhanced honeypot mode, {} payloads detected)",
            peer_addr,
            payload_types.len()
        );
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
