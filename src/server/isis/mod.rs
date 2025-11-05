//! IS-IS (Intermediate System to Intermediate System) routing protocol server
//!
//! Implementation of ISO/IEC 10589 and RFC 1195 (IS-IS over IP).
//! The LLM controls routing decisions, neighbor adjacencies, and LSP generation.

pub mod actions;

use crate::server::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace, warn};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use actions::ISIS_HELLO_EVENT;
use crate::server::IsisProtocol;
use crate::protocol::Event;
use crate::state::app_state::AppState;

// IS-IS Constants (ISO/IEC 10589, RFC 1195)
const ISIS_INTRADOMAIN_ROUTING_PROTOCOL_DISCRIMINATOR: u8 = 0x83;
const ISIS_HEADER_LEN: usize = 8; // Common header length

// IS-IS PDU Types (RFC 1195 Section 4)
const ISIS_HELLO_LAN_L1: u8 = 15; // Level 1 LAN Hello
const ISIS_HELLO_LAN_L2: u8 = 16; // Level 2 LAN Hello
const ISIS_HELLO_P2P: u8 = 17;    // Point-to-Point Hello
const ISIS_LSP_L1: u8 = 18;       // Level 1 Link State PDU
const ISIS_LSP_L2: u8 = 20;       // Level 2 Link State PDU
const ISIS_CSNP_L1: u8 = 24;      // Level 1 Complete Sequence Number PDU
const ISIS_CSNP_L2: u8 = 25;      // Level 2 Complete Sequence Number PDU
const ISIS_PSNP_L1: u8 = 26;      // Level 1 Partial Sequence Number PDU
const ISIS_PSNP_L2: u8 = 27;      // Level 2 Partial Sequence Number PDU

// IS-IS TLV Types (commonly used)
const ISIS_TLV_AREA_ADDRESSES: u8 = 1;
const ISIS_TLV_NEIGHBORS: u8 = 6;
const ISIS_TLV_PADDING: u8 = 8;
const ISIS_TLV_LSP_ENTRIES: u8 = 9;
const ISIS_TLV_PROTOCOLS_SUPPORTED: u8 = 129;
const ISIS_TLV_IP_INTERFACE_ADDRESSES: u8 = 132;
const ISIS_TLV_IP_INTERNAL_REACHABILITY: u8 = 128;
const ISIS_TLV_EXTENDED_REACHABILITY: u8 = 22;
const ISIS_TLV_HOSTNAME: u8 = 137;

/// IS-IS server that handles routing protocol operations with LLM
pub struct IsisServer;

impl IsisServer {
    /// Spawn IS-IS server with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        info!("IS-IS server listening on {}", local_addr);
        let _ = status_tx.send(format!("[INFO] IS-IS server listening on {}", local_addr));

        // Extract configuration from startup params
        let (system_id, area_id, level) = if let Some(ref params) = startup_params {
            let sys_id = params.get_optional_string("system_id")
                .unwrap_or_else(|| "0000.0000.0001".to_string());
            let area = params.get_optional_string("area_id")
                .unwrap_or_else(|| "49.0001".to_string());
            let lvl = params.get_optional_string("level")
                .unwrap_or_else(|| "level-2".to_string());

            info!("IS-IS configured: system_id={}, area={}, level={}", sys_id, area, lvl);
            let _ = status_tx.send(format!(
                "[INFO] IS-IS configured: system_id={}, area={}, level={}",
                sys_id, area, lvl
            ));
            (sys_id, area, lvl)
        } else {
            // Defaults
            ("0000.0000.0001".to_string(), "49.0001".to_string(), "level-2".to_string())
        };

        let protocol = Arc::new(IsisProtocol::new());

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 1500]; // MTU size for IS-IS packets

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let connection_id = ConnectionId::new();

                        // Parse IS-IS PDU
                        if let Err(e) = Self::handle_isis_pdu(
                            &data,
                            peer_addr,
                            connection_id,
                            server_id,
                            &llm_client,
                            &app_state,
                            &status_tx,
                            &socket,
                            local_addr,
                            &protocol,
                            &system_id,
                            &area_id,
                            &level,
                        ).await {
                            error!("IS-IS PDU handling error: {}", e);
                            let _ = status_tx.send(format!("[ERROR] IS-IS PDU error: {}", e));
                        }
                    }
                    Err(e) => {
                        error!("IS-IS receive error: {}", e);
                        let _ = status_tx.send(format!("[ERROR] IS-IS receive error: {}", e));
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Handle incoming IS-IS PDU
    async fn handle_isis_pdu(
        data: &[u8],
        peer_addr: SocketAddr,
        connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        socket: &Arc<UdpSocket>,
        local_addr: SocketAddr,
        protocol: &Arc<IsisProtocol>,
        system_id: &str,
        area_id: &str,
        level: &str,
    ) -> Result<()> {
        // Validate minimum header length
        if data.len() < ISIS_HEADER_LEN {
            warn!("IS-IS packet too short: {} bytes", data.len());
            let _ = status_tx.send(format!("[WARN] IS-IS packet too short: {} bytes", data.len()));
            return Ok(());
        }

        // Parse common header
        let intradomain_routing_protocol_discriminator = data[0];
        let length_indicator = data[1];
        let version_protocol_id_extension = data[2];
        let id_length = data[3];
        let pdu_type = data[4];
        let version = data[5];
        let _reserved = data[6];
        let max_area_addresses = data[7];

        // Validate protocol discriminator
        if intradomain_routing_protocol_discriminator != ISIS_INTRADOMAIN_ROUTING_PROTOCOL_DISCRIMINATOR {
            warn!("IS-IS invalid protocol discriminator: 0x{:02x}", intradomain_routing_protocol_discriminator);
            let _ = status_tx.send(format!(
                "[WARN] IS-IS invalid protocol discriminator: 0x{:02x}",
                intradomain_routing_protocol_discriminator
            ));
            return Ok(());
        }

        // Validate version (should be 1)
        if version != 1 {
            warn!("IS-IS unsupported version: {}", version);
            let _ = status_tx.send(format!("[WARN] IS-IS unsupported version: {}", version));
            return Ok(());
        }

        // Log PDU type
        let pdu_type_name = match pdu_type {
            ISIS_HELLO_LAN_L1 => "LAN Hello L1",
            ISIS_HELLO_LAN_L2 => "LAN Hello L2",
            ISIS_HELLO_P2P => "P2P Hello",
            ISIS_LSP_L1 => "LSP L1",
            ISIS_LSP_L2 => "LSP L2",
            ISIS_CSNP_L1 => "CSNP L1",
            ISIS_CSNP_L2 => "CSNP L2",
            ISIS_PSNP_L1 => "PSNP L1",
            ISIS_PSNP_L2 => "PSNP L2",
            _ => "Unknown",
        };

        debug!("IS-IS received {} ({}) from {}, {} bytes",
               pdu_type_name, pdu_type, peer_addr, data.len());
        let _ = status_tx.send(format!(
            "[DEBUG] IS-IS received {} from {}, {} bytes",
            pdu_type_name, peer_addr, data.len()
        ));

        // TRACE: Log full payload
        let hex_str = hex::encode(data);
        trace!("IS-IS PDU (hex): {}", hex_str);
        let _ = status_tx.send(format!("[TRACE] IS-IS PDU (hex): {}", hex_str));

        // Add connection to ServerInstance
        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
        let now = std::time::Instant::now();
        let conn_state = ServerConnectionState {
            id: connection_id,
            remote_addr: peer_addr,
            local_addr,
            bytes_sent: 0,
            bytes_received: data.len() as u64,
            packets_sent: 0,
            packets_received: 1,
            last_activity: now,
            status: ConnectionStatus::Active,
            status_changed_at: now,
            protocol_info: ProtocolConnectionInfo::Isis {
                adjacency_state: "init".to_string(),
                neighbor_system_id: None,
                level: level.to_string(),
            },
        };
        app_state.add_connection_to_server(server_id, conn_state).await;
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Parse TLVs (start after common header + PDU-specific header)
        let tlv_start = length_indicator as usize;
        let tlvs = if tlv_start < data.len() {
            Self::parse_tlvs(&data[tlv_start..])
        } else {
            Vec::new()
        };

        debug!("IS-IS parsed {} TLVs", tlvs.len());
        let _ = status_tx.send(format!("[DEBUG] IS-IS parsed {} TLVs", tlvs.len()));

        // Handle based on PDU type
        match pdu_type {
            ISIS_HELLO_LAN_L1 | ISIS_HELLO_LAN_L2 | ISIS_HELLO_P2P => {
                Self::handle_hello_pdu(
                    data,
                    pdu_type,
                    pdu_type_name,
                    &tlvs,
                    peer_addr,
                    connection_id,
                    server_id,
                    llm_client,
                    app_state,
                    status_tx,
                    socket,
                    protocol,
                    system_id,
                    area_id,
                    level,
                ).await?;
            }
            ISIS_LSP_L1 | ISIS_LSP_L2 => {
                info!("IS-IS LSP received (not yet handled)");
                let _ = status_tx.send("[INFO] IS-IS LSP received (forwarding to LLM)".to_string());
                // Future: Parse LSP and forward to LLM
            }
            ISIS_CSNP_L1 | ISIS_CSNP_L2 => {
                info!("IS-IS CSNP received (not yet handled)");
                let _ = status_tx.send("[INFO] IS-IS CSNP received".to_string());
            }
            ISIS_PSNP_L1 | ISIS_PSNP_L2 => {
                info!("IS-IS PSNP received (not yet handled)");
                let _ = status_tx.send("[INFO] IS-IS PSNP received".to_string());
            }
            _ => {
                warn!("IS-IS unsupported PDU type: {}", pdu_type);
                let _ = status_tx.send(format!("[WARN] IS-IS unsupported PDU type: {}", pdu_type));
            }
        }

        Ok(())
    }

    /// Handle IS-IS Hello PDU
    async fn handle_hello_pdu(
        data: &[u8],
        pdu_type: u8,
        pdu_type_name: &str,
        tlvs: &[(u8, Vec<u8>)],
        peer_addr: SocketAddr,
        connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        socket: &Arc<UdpSocket>,
        protocol: &Arc<IsisProtocol>,
        system_id: &str,
        area_id: &str,
        level: &str,
    ) -> Result<()> {
        info!("IS-IS {} from {}", pdu_type_name, peer_addr);
        let _ = status_tx.send(format!("→ IS-IS {} from {}", pdu_type_name, peer_addr));

        // Extract information from TLVs
        let mut area_addresses = Vec::new();
        let mut protocols_supported = Vec::new();
        let mut ip_addresses = Vec::new();
        let mut hostname = None;

        for (tlv_type, tlv_value) in tlvs {
            match *tlv_type {
                ISIS_TLV_AREA_ADDRESSES => {
                    // Parse area addresses
                    area_addresses.push(hex::encode(tlv_value));
                }
                ISIS_TLV_PROTOCOLS_SUPPORTED => {
                    // Protocol IDs: 0xCC = IPv4, 0x8E = IPv6
                    for proto in tlv_value {
                        protocols_supported.push(format!("0x{:02x}", proto));
                    }
                }
                ISIS_TLV_IP_INTERFACE_ADDRESSES => {
                    // Parse IPv4 addresses (4 bytes each)
                    let mut i = 0;
                    while i + 3 < tlv_value.len() {
                        let ip = format!("{}.{}.{}.{}",
                                       tlv_value[i], tlv_value[i+1],
                                       tlv_value[i+2], tlv_value[i+3]);
                        ip_addresses.push(ip);
                        i += 4;
                    }
                }
                ISIS_TLV_HOSTNAME => {
                    // Parse hostname
                    if let Ok(name) = String::from_utf8(tlv_value.clone()) {
                        hostname = Some(name);
                    }
                }
                _ => {
                    // Other TLVs not parsed
                }
            }
        }

        // Create event for LLM
        let mut event_data = serde_json::json!({
            "pdu_type": pdu_type_name,
            "pdu_type_code": pdu_type,
            "peer_addr": peer_addr.to_string(),
            "packet_hex": hex::encode(data),
        });

        if !area_addresses.is_empty() {
            event_data["area_addresses"] = serde_json::json!(area_addresses);
        }
        if !protocols_supported.is_empty() {
            event_data["protocols_supported"] = serde_json::json!(protocols_supported);
        }
        if !ip_addresses.is_empty() {
            event_data["ip_addresses"] = serde_json::json!(ip_addresses);
        }
        if let Some(ref name) = hostname {
            event_data["hostname"] = serde_json::json!(name);
        }

        let event = Event::new(&ISIS_HELLO_EVENT, event_data);

        debug!("IS-IS calling LLM for Hello from {}", peer_addr);
        let _ = status_tx.send(format!("[DEBUG] IS-IS calling LLM for Hello from {}", peer_addr));

        // Call LLM to decide response
        match call_llm(
            llm_client,
            app_state,
            server_id,
            Some(connection_id),
            &event,
            protocol.as_ref(),
        ).await {
            Ok(execution_result) => {
                // Display messages from LLM
                for message in &execution_result.messages {
                    info!("{}", message);
                    let _ = status_tx.send(format!("[INFO] {}", message));
                }

                debug!("IS-IS parsed {} actions", execution_result.raw_actions.len());
                let _ = status_tx.send(format!("[DEBUG] IS-IS parsed {} actions", execution_result.raw_actions.len()));

                // Process protocol results
                for protocol_result in execution_result.protocol_results {
                    if let Some(output_data) = protocol_result.get_all_output().first() {
                        let _ = socket.send_to(output_data, peer_addr).await;

                        debug!("IS-IS sent {} bytes to {}", output_data.len(), peer_addr);
                        let _ = status_tx.send(format!("[DEBUG] IS-IS sent {} bytes to {}", output_data.len(), peer_addr));

                        trace!("IS-IS sent (hex): {}", hex::encode(output_data));
                        let _ = status_tx.send(format!("[TRACE] IS-IS sent (hex): {}", hex::encode(output_data)));

                        let _ = status_tx.send(format!("→ IS-IS response to {} ({} bytes)", peer_addr, output_data.len()));
                    }
                }
            }
            Err(e) => {
                error!("IS-IS LLM call failed: {}", e);
                let _ = status_tx.send(format!("✗ IS-IS LLM error: {}", e));
            }
        }

        Ok(())
    }

    /// Parse IS-IS TLVs from packet data
    fn parse_tlvs(data: &[u8]) -> Vec<(u8, Vec<u8>)> {
        let mut tlvs = Vec::new();
        let mut offset = 0;

        while offset + 1 < data.len() {
            let tlv_type = data[offset];
            let tlv_len = data[offset + 1] as usize;
            offset += 2;

            if offset + tlv_len > data.len() {
                // Malformed TLV
                break;
            }

            let tlv_value = data[offset..offset + tlv_len].to_vec();
            tlvs.push((tlv_type, tlv_value));
            offset += tlv_len;
        }

        tlvs
    }
}
