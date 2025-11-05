//! IS-IS (Intermediate System to Intermediate System) routing protocol server
//!
//! Implementation of ISO/IEC 10589 and RFC 1195 (IS-IS over IP).
//! Operates at Layer 2 using raw sockets/pcap for true IS-IS protocol operation.
//! The LLM controls routing decisions, neighbor adjacencies, and LSP generation.

pub mod actions;

use crate::server::connection::ConnectionId;
use anyhow::{Context, Result};
use bytes::Bytes;
use pcap::{Capture, Device};
use std::sync::Arc;
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

// IS-IS Multicast MAC Addresses
const ISIS_ALL_L1_IS: [u8; 6] = [0x01, 0x80, 0xC2, 0x00, 0x00, 0x14]; // All Level 1 ISs
const ISIS_ALL_L2_IS: [u8; 6] = [0x01, 0x80, 0xC2, 0x00, 0x00, 0x15]; // All Level 2 ISs

// LLC/SNAP headers for IS-IS over Ethernet
const LLC_DSAP_ISO: u8 = 0xFE; // ISO CLNS
const LLC_SSAP_ISO: u8 = 0xFE;
const LLC_CTRL: u8 = 0x03; // Unnumbered Information

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

/// IS-IS server that handles routing protocol operations with LLM at Layer 2
pub struct IsisServer;

impl IsisServer {
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

    /// Spawn IS-IS server with integrated LLM actions (Layer 2 with pcap)
    pub async fn spawn_with_llm_actions(
        interface: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<String> {
        info!("IS-IS server starting on interface: {}", interface);
        let _ = status_tx.send(format!("[INFO] IS-IS server starting on interface: {}", interface));

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
        let interface_clone = interface.clone();
        let protocol_clone = protocol.clone();

        // pcap is blocking, so we run it in a blocking task
        tokio::task::spawn_blocking(move || {
            // Find device
            let device = match Self::find_device(&interface_clone) {
                Ok(d) => d,
                Err(e) => {
                    error!("Failed to find device: {}", e);
                    let _ = status_tx.send(format!("[ERROR] Failed to find device: {}", e));
                    return;
                }
            };

            // Get device MAC address (needed for source MAC in responses)
            let local_mac = match Self::get_interface_mac(&interface_clone) {
                Ok(mac) => mac,
                Err(e) => {
                    warn!("Failed to get interface MAC address: {}, using default", e);
                    [0x02, 0x00, 0x00, 0x00, 0x00, 0x01] // Local admin MAC
                }
            };

            // Open capture for IS-IS packets
            let mut cap = match Capture::from_device(device)
                .map(|c| c.promisc(true).snaplen(65535).timeout(1000))
                .and_then(|c| c.open())
            {
                Ok(c) => c,
                Err(e) => {
                    error!("Failed to open capture: {}", e);
                    let _ = status_tx.send(format!("[ERROR] Failed to open capture: {}", e));
                    return;
                }
            };

            // BPF filter for IS-IS packets (LLC DSAP/SSAP 0xFE)
            // This matches packets with IS-IS LLC headers
            let filter = "ether proto 0xfefe or (ether[14:2] = 0xfefe and ether[16:1] = 0x03)";
            if let Err(e) = cap.filter(filter, true) {
                warn!("Failed to apply IS-IS filter: {}", e);
                let _ = status_tx.send(format!("[WARN] Failed to apply IS-IS filter: {}", e));
            }

            info!("IS-IS listening on {}, MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                  interface_clone,
                  local_mac[0], local_mac[1], local_mac[2],
                  local_mac[3], local_mac[4], local_mac[5]);
            let _ = status_tx.send(format!("→ IS-IS ready on interface {}", interface_clone));

            let runtime = tokio::runtime::Handle::current();

            // Store capture handle in Arc for packet injection
            let cap_arc = Arc::new(std::sync::Mutex::new(cap));

            // Capture loop
            loop {
                let mut cap_guard = cap_arc.lock().unwrap();
                match cap_guard.next_packet() {
                    Ok(packet) => {
                        let data = Bytes::copy_from_slice(packet.data);
                        drop(cap_guard); // Release lock before async processing

                        // Parse Ethernet frame
                        if data.len() < 14 {
                            continue; // Too short for Ethernet header
                        }

                        let _dst_mac = &data[0..6];
                        let src_mac = &data[6..12];
                        let eth_type = u16::from_be_bytes([data[12], data[13]]);

                        // Check for IS-IS: LLC/SNAP headers after Ethernet
                        // Ethernet (14) + LLC (3) = 17 bytes minimum
                        if data.len() < 17 {
                            continue;
                        }

                        // Skip Ethernet header (14 bytes), check LLC
                        let llc_offset = 14;
                        let dsap = data[llc_offset];
                        let ssap = data[llc_offset + 1];
                        let ctrl = data[llc_offset + 2];

                        // IS-IS uses LLC DSAP/SSAP = 0xFE, Control = 0x03
                        if dsap != LLC_DSAP_ISO || ssap != LLC_SSAP_ISO || ctrl != LLC_CTRL {
                            continue; // Not IS-IS
                        }

                        // IS-IS PDU starts after LLC header (at offset 17)
                        if data.len() < 17 + ISIS_HEADER_LEN {
                            continue; // Too short for IS-IS header
                        }

                        // Clone the IS-IS PDU and source MAC for async task
                        let isis_pdu = data.slice(17..);
                        let src_mac_bytes: [u8; 6] = [
                            src_mac[0], src_mac[1], src_mac[2],
                            src_mac[3], src_mac[4], src_mac[5]
                        ];

                        let connection_id = ConnectionId::new();

                        // DEBUG: Log summary
                        debug!("IS-IS received {} bytes from {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                               data.len(),
                               src_mac[0], src_mac[1], src_mac[2],
                               src_mac[3], src_mac[4], src_mac[5]);
                        let _ = status_tx.send(format!("[DEBUG] IS-IS received {} bytes", data.len()));

                        // TRACE: Log full payload
                        let hex_str = hex::encode(&data);
                        trace!("IS-IS frame (hex): {}", hex_str);
                        let _ = status_tx.send(format!("[TRACE] IS-IS frame (hex): {}", hex_str));

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_task_clone = protocol_clone.clone();
                        let system_id_clone = system_id.clone();
                        let area_id_clone = area_id.clone();
                        let level_clone = level.clone();
                        let cap_clone = cap_arc.clone();
                        let local_mac_clone = local_mac.clone();

                        // Spawn async task to handle PDU with LLM
                        runtime.spawn(async move {
                            if let Err(e) = Self::handle_isis_pdu(
                                &isis_pdu,
                                &src_mac_bytes,
                                connection_id,
                                server_id,
                                &llm_clone,
                                &state_clone,
                                &status_clone,
                                &protocol_task_clone,
                                &system_id_clone,
                                &area_id_clone,
                                &level_clone,
                                cap_clone,
                                local_mac_clone,
                            ).await {
                                error!("IS-IS PDU handling error: {}", e);
                                let _ = status_clone.send(format!("[ERROR] IS-IS PDU error: {}", e));
                            }
                        });
                    }
                    Err(pcap::Error::TimeoutExpired) => {
                        drop(cap_guard);
                        continue;
                    }
                    Err(e) => {
                        error!("IS-IS capture error: {}", e);
                        let _ = status_tx.send(format!("[ERROR] IS-IS capture error: {}", e));
                        break;
                    }
                }
            }
        });

        Ok(interface)
    }

    /// Handle incoming IS-IS PDU
    async fn handle_isis_pdu(
        isis_pdu: &[u8],
        src_mac: &[u8],
        connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: &Arc<IsisProtocol>,
        system_id: &str,
        area_id: &str,
        level: &str,
        cap: Arc<std::sync::Mutex<Capture<pcap::Active>>>,
        local_mac: [u8; 6],
    ) -> Result<()> {
        // Parse common header
        let intradomain_routing_protocol_discriminator = isis_pdu[0];
        let _length_indicator = isis_pdu[1];
        let _version_protocol_id_extension = isis_pdu[2];
        let _id_length = isis_pdu[3];
        let pdu_type = isis_pdu[4];
        let version = isis_pdu[5];
        let _reserved = isis_pdu[6];
        let _max_area_addresses = isis_pdu[7];

        // Validate protocol discriminator
        if intradomain_routing_protocol_discriminator != ISIS_INTRADOMAIN_ROUTING_PROTOCOL_DISCRIMINATOR {
            warn!("IS-IS invalid protocol discriminator: 0x{:02x}", intradomain_routing_protocol_discriminator);
            return Ok(());
        }

        // Validate version (should be 1)
        if version != 1 {
            warn!("IS-IS unsupported version: {}", version);
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

        info!("IS-IS received {} ({}) from {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
              pdu_type_name, pdu_type,
              src_mac[0], src_mac[1], src_mac[2],
              src_mac[3], src_mac[4], src_mac[5]);
        let _ = status_tx.send(format!("→ IS-IS {} from {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                                        pdu_type_name,
                                        src_mac[0], src_mac[1], src_mac[2],
                                        src_mac[3], src_mac[4], src_mac[5]));

        // Add connection to ServerInstance
        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
        let now = std::time::Instant::now();
        let conn_state = ServerConnectionState {
            id: connection_id,
            remote_addr: format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:0",
                                src_mac[0], src_mac[1], src_mac[2],
                                src_mac[3], src_mac[4], src_mac[5])
                .parse()
                .unwrap_or("0.0.0.0:0".parse().unwrap()),
            local_addr: format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:0",
                               local_mac[0], local_mac[1], local_mac[2],
                               local_mac[3], local_mac[4], local_mac[5])
                .parse()
                .unwrap_or("0.0.0.0:0".parse().unwrap()),
            bytes_sent: 0,
            bytes_received: isis_pdu.len() as u64,
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

        // Handle based on PDU type (for now, only Hello)
        match pdu_type {
            ISIS_HELLO_LAN_L1 | ISIS_HELLO_LAN_L2 | ISIS_HELLO_P2P => {
                Self::handle_hello_pdu(
                    isis_pdu,
                    src_mac,
                    pdu_type_name,
                    connection_id,
                    server_id,
                    llm_client,
                    app_state,
                    status_tx,
                    protocol,
                    system_id,
                    area_id,
                    level,
                    cap,
                    local_mac,
                ).await?;
            }
            ISIS_LSP_L1 | ISIS_LSP_L2 => {
                info!("IS-IS LSP received (not yet handled)");
                let _ = status_tx.send("[INFO] IS-IS LSP received".to_string());
            }
            ISIS_CSNP_L1 | ISIS_CSNP_L2 => {
                info!("IS-IS CSNP received (not yet handled)");
            }
            ISIS_PSNP_L1 | ISIS_PSNP_L2 => {
                info!("IS-IS PSNP received (not yet handled)");
            }
            _ => {
                warn!("IS-IS unsupported PDU type: {}", pdu_type);
            }
        }

        Ok(())
    }

    /// Handle IS-IS Hello PDU
    async fn handle_hello_pdu(
        isis_pdu: &[u8],
        src_mac: &[u8],
        pdu_type_name: &str,
        _connection_id: ConnectionId,
        server_id: crate::state::ServerId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: &Arc<IsisProtocol>,
        _system_id: &str,
        _area_id: &str,
        level: &str,
        cap: Arc<std::sync::Mutex<Capture<pcap::Active>>>,
        local_mac: [u8; 6],
    ) -> Result<()> {
        // Parse TLVs from Hello PDU
        // Hello header varies by type, but TLVs start after fixed header
        // For LAN Hello: header is 27 bytes, for P2P: 20 bytes
        let tlv_start = if isis_pdu[4] == ISIS_HELLO_P2P { 20 } else { 27 };
        let tlvs = if tlv_start < isis_pdu.len() {
            Self::parse_tlvs(&isis_pdu[tlv_start..])
        } else {
            Vec::new()
        };

        // Extract information from TLVs
        let mut area_addresses = Vec::new();
        let mut protocols_supported = Vec::new();
        let mut ip_addresses = Vec::new();
        let mut hostname = None;

        for (tlv_type, tlv_value) in &tlvs {
            match *tlv_type {
                ISIS_TLV_AREA_ADDRESSES => {
                    area_addresses.push(hex::encode(tlv_value));
                }
                ISIS_TLV_PROTOCOLS_SUPPORTED => {
                    for proto in tlv_value {
                        protocols_supported.push(format!("0x{:02x}", proto));
                    }
                }
                ISIS_TLV_IP_INTERFACE_ADDRESSES => {
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
                    if let Ok(name) = String::from_utf8(tlv_value.clone()) {
                        hostname = Some(name);
                    }
                }
                _ => {}
            }
        }

        // Create event for LLM
        let mut event_data = serde_json::json!({
            "pdu_type": pdu_type_name,
            "packet_hex": hex::encode(isis_pdu),
            "src_mac": format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                              src_mac[0], src_mac[1], src_mac[2],
                              src_mac[3], src_mac[4], src_mac[5]),
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

        debug!("IS-IS calling LLM for Hello");
        let _ = status_tx.send("[DEBUG] IS-IS calling LLM for Hello".to_string());

        // Call LLM to decide response
        match call_llm(
            llm_client,
            app_state,
            server_id,
            None,
            &event,
            protocol.as_ref(),
        ).await {
            Ok(execution_result) => {
                // Display messages from LLM
                for message in &execution_result.messages {
                    info!("{}", message);
                    let _ = status_tx.send(format!("[INFO] {}", message));
                }

                // Process protocol results - send frames via pcap
                for protocol_result in execution_result.protocol_results {
                    if let Some(output_data) = protocol_result.get_all_output().first() {
                        // output_data is the IS-IS PDU, we need to wrap in Ethernet + LLC
                        let frame = Self::build_ethernet_frame(output_data, local_mac, level)?;

                        // Send frame via pcap
                        let mut cap_guard = cap.lock().unwrap();
                        if let Err(e) = cap_guard.sendpacket(frame.as_ref()) {
                            error!("Failed to send IS-IS frame: {}", e);
                            let _ = status_tx.send(format!("[ERROR] Failed to send IS-IS frame: {}", e));
                        } else {
                            debug!("IS-IS sent {} bytes", frame.len());
                            let _ = status_tx.send(format!("[DEBUG] IS-IS sent {} bytes", frame.len()));
                            trace!("IS-IS sent (hex): {}", hex::encode(&frame));
                        }
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
                break;
            }

            let tlv_value = data[offset..offset + tlv_len].to_vec();
            tlvs.push((tlv_type, tlv_value));
            offset += tlv_len;
        }

        tlvs
    }

    /// Build Ethernet frame with LLC header and IS-IS PDU
    fn build_ethernet_frame(isis_pdu: &[u8], src_mac: [u8; 6], level: &str) -> Result<Vec<u8>> {
        let mut frame = Vec::new();

        // Destination MAC (multicast based on level)
        let dst_mac = if level.contains("level-1") {
            ISIS_ALL_L1_IS
        } else {
            ISIS_ALL_L2_IS
        };
        frame.extend_from_slice(&dst_mac);

        // Source MAC
        frame.extend_from_slice(&src_mac);

        // Ethernet Type: Length field (for 802.3 with LLC)
        let length = (3 + isis_pdu.len()) as u16; // LLC (3) + IS-IS PDU
        frame.extend_from_slice(&length.to_be_bytes());

        // LLC Header (3 bytes)
        frame.push(LLC_DSAP_ISO); // DSAP = 0xFE (ISO CLNS)
        frame.push(LLC_SSAP_ISO); // SSAP = 0xFE
        frame.push(LLC_CTRL);     // Control = 0x03 (UI)

        // IS-IS PDU
        frame.extend_from_slice(isis_pdu);

        Ok(frame)
    }

    /// Get MAC address of interface (platform-specific)
    fn get_interface_mac(interface: &str) -> Result<[u8; 6]> {
        // Try to get MAC from system
        #[cfg(target_os = "linux")]
        {
            use std::fs;
            let path = format!("/sys/class/net/{}/address", interface);
            if let Ok(contents) = fs::read_to_string(&path) {
                let parts: Vec<&str> = contents.trim().split(':').collect();
                if parts.len() == 6 {
                    let mut mac = [0u8; 6];
                    for (i, part) in parts.iter().enumerate() {
                        if let Ok(byte) = u8::from_str_radix(part, 16) {
                            mac[i] = byte;
                        }
                    }
                    return Ok(mac);
                }
            }
        }

        // Fallback: use locally administered MAC
        Err(anyhow::anyhow!("Could not determine interface MAC address"))
    }
}
