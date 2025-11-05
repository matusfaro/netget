//! IGMP server implementation using raw sockets
pub mod actions;

use crate::server::connection::ConnectionId;
use actions::IgmpProtocol;
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace, warn};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use actions::{IGMP_QUERY_RECEIVED_EVENT, IGMP_REPORT_RECEIVED_EVENT, IGMP_LEAVE_RECEIVED_EVENT};
use crate::protocol::Event;
use crate::state::app_state::AppState;

/// IGMP message types (RFC 2236)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IgmpMessageType {
    /// Membership Query (0x11)
    MembershipQuery = 0x11,
    /// IGMPv1 Membership Report (0x12)
    V1MembershipReport = 0x12,
    /// IGMPv2 Membership Report (0x16)
    V2MembershipReport = 0x16,
    /// Leave Group (0x17)
    LeaveGroup = 0x17,
    /// IGMPv3 Membership Report (0x22)
    V3MembershipReport = 0x22,
}

impl IgmpMessageType {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x11 => Some(Self::MembershipQuery),
            0x12 => Some(Self::V1MembershipReport),
            0x16 => Some(Self::V2MembershipReport),
            0x17 => Some(Self::LeaveGroup),
            0x22 => Some(Self::V3MembershipReport),
            _ => None,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::MembershipQuery => "Membership Query",
            Self::V1MembershipReport => "IGMPv1 Membership Report",
            Self::V2MembershipReport => "IGMPv2 Membership Report",
            Self::LeaveGroup => "Leave Group",
            Self::V3MembershipReport => "IGMPv3 Membership Report",
        }
    }
}

/// Parsed IGMP message
#[derive(Debug, Clone)]
pub struct IgmpMessage {
    pub msg_type: IgmpMessageType,
    pub max_response_time: u8,
    pub group_address: Ipv4Addr,
    pub raw_data: Vec<u8>,
}

impl IgmpMessage {
    /// Parse an IGMP message from raw bytes
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 8 {
            return Err(anyhow::anyhow!("IGMP message too short: {} bytes", data.len()));
        }

        let msg_type = IgmpMessageType::from_u8(data[0])
            .context("Unknown IGMP message type")?;

        let max_response_time = data[1];

        // Checksum is at bytes 2-3 (we don't verify it for now)

        let group_address = Ipv4Addr::new(data[4], data[5], data[6], data[7]);

        Ok(Self {
            msg_type,
            max_response_time,
            group_address,
            raw_data: data.to_vec(),
        })
    }

    /// Check if this is a general query (group address is 0.0.0.0)
    pub fn is_general_query(&self) -> bool {
        self.msg_type == IgmpMessageType::MembershipQuery
            && self.group_address.is_unspecified()
    }

    /// Get human-readable description
    pub fn description(&self) -> String {
        format!(
            "{} for group {} (max_resp={})",
            self.msg_type.as_str(),
            self.group_address,
            self.max_response_time
        )
    }
}

/// IGMP server state
pub struct IgmpServerState {
    /// Set of multicast groups we've joined
    pub joined_groups: HashSet<Ipv4Addr>,
}

impl IgmpServerState {
    fn new() -> Self {
        Self {
            joined_groups: HashSet::new(),
        }
    }
}

/// IGMP server that manages multicast group membership
pub struct IgmpServer;

impl IgmpServer {
    /// Spawn IGMP server with action-based LLM handling
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        // For IGMP, we need raw socket with IP_HDRINCL or IPPROTO_IGMP
        // Since this requires root privileges and is complex, we'll use a UDP socket
        // on a non-standard port as a placeholder for testing.
        // In production, this would use socket2 with raw sockets.

        info!("IGMP server starting (note: requires raw socket support)");
        let _ = status_tx.send("[INFO] IGMP server starting (note: requires raw socket support)".to_string());

        // IMPORTANT: IGMP uses IP protocol 2, not UDP. This is a simplified implementation
        // for demonstration. A full implementation would use:
        // - socket2::Socket with Domain::IPV4, Type::RAW, Protocol::IGMPV2
        // - Setting IP_HDRINCL or using IPPROTO_IGMP
        // - Joining multicast groups via IP_ADD_MEMBERSHIP

        warn!("IGMP server: Using UDP socket for testing (production requires raw sockets with root)");
        let _ = status_tx.send("[WARN] IGMP server: Using UDP socket for testing (production requires raw sockets with root)".to_string());

        use tokio::net::UdpSocket;
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        info!("IGMP server listening on {} (action-based)", local_addr);

        let protocol = Arc::new(IgmpProtocol::new());
        let server_state = Arc::new(Mutex::new(IgmpServerState::new()));

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 65535];

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let connection_id = ConnectionId::new();

                        // Add connection to ServerInstance
                        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
                        let now = std::time::Instant::now();
                        let state = server_state.lock().await;
                        let conn_state = ServerConnectionState {
                            id: connection_id,
                            remote_addr: peer_addr,
                            local_addr,
                            bytes_sent: 0,
                            bytes_received: n as u64,
                            packets_sent: 0,
                            packets_received: 1,
                            last_activity: now,
                            status: ConnectionStatus::Active,
                            status_changed_at: now,
                            protocol_info: ProtocolConnectionInfo::Igmp {
                                joined_groups: state.joined_groups.iter().copied().collect(),
                            },
                        };
                        drop(state);
                        app_state.add_connection_to_server(server_id, conn_state).await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        // Parse IGMP message
                        let igmp_msg = match IgmpMessage::parse(&data) {
                            Ok(msg) => msg,
                            Err(e) => {
                                debug!("IGMP received non-IGMP packet ({} bytes): {}", n, e);
                                let _ = status_tx.send(format!("[DEBUG] IGMP received non-IGMP packet ({} bytes): {}", n, e));
                                continue;
                            }
                        };

                        // DEBUG: Log summary
                        debug!("IGMP received from {}: {}", peer_addr, igmp_msg.description());
                        let _ = status_tx.send(format!("[DEBUG] IGMP received from {}: {}", peer_addr, igmp_msg.description()));

                        // TRACE: Log full payload
                        let hex_str = hex::encode(&data);
                        trace!("IGMP data (hex): {}", hex_str);
                        let _ = status_tx.send(format!("[TRACE] IGMP data (hex): {}", hex_str));

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();
                        let protocol_clone = protocol.clone();
                        let server_state_clone = server_state.clone();

                        tokio::spawn(async move {
                            // Determine event type and build event data
                            let (event, _event_type_ref) = match igmp_msg.msg_type {
                                IgmpMessageType::MembershipQuery => {
                                    let query_type = if igmp_msg.is_general_query() {
                                        "General"
                                    } else {
                                        "Group-Specific"
                                    };
                                    (
                                        Event::new(&IGMP_QUERY_RECEIVED_EVENT, serde_json::json!({
                                            "query_type": query_type,
                                            "group_address": igmp_msg.group_address.to_string(),
                                            "max_response_time": igmp_msg.max_response_time
                                        })),
                                        &IGMP_QUERY_RECEIVED_EVENT
                                    )
                                }
                                IgmpMessageType::V1MembershipReport | IgmpMessageType::V2MembershipReport | IgmpMessageType::V3MembershipReport => {
                                    (
                                        Event::new(&IGMP_REPORT_RECEIVED_EVENT, serde_json::json!({
                                            "group_address": igmp_msg.group_address.to_string()
                                        })),
                                        &IGMP_REPORT_RECEIVED_EVENT
                                    )
                                }
                                IgmpMessageType::LeaveGroup => {
                                    (
                                        Event::new(&IGMP_LEAVE_RECEIVED_EVENT, serde_json::json!({
                                            "group_address": igmp_msg.group_address.to_string()
                                        })),
                                        &IGMP_LEAVE_RECEIVED_EVENT
                                    )
                                }
                            };

                            debug!("IGMP calling LLM for {} from {}", igmp_msg.msg_type.as_str(), peer_addr);
                            let _ = status_clone.send(format!("[DEBUG] IGMP calling LLM for {} from {}", igmp_msg.msg_type.as_str(), peer_addr));

                            match call_llm(
                                &llm_clone,
                                &state_clone,
                                server_id,
                                None,
                                &event,
                                protocol_clone.as_ref(),
                            ).await {
                                Ok(execution_result) => {
                                    for message in &execution_result.messages {
                                        info!("{}", message);
                                        let _ = status_clone.send(format!("[INFO] {}", message));
                                    }

                                    debug!("IGMP got {} protocol results", execution_result.protocol_results.len());
                                    let _ = status_clone.send(format!("[DEBUG] IGMP got {} protocol results", execution_result.protocol_results.len()));

                                    // Process protocol results
                                    for protocol_result in &execution_result.protocol_results {
                                        if let Some(output_data) = protocol_result.get_all_output().first() {
                                            // Send the IGMP response packet
                                            // In a real implementation, this would use raw sockets
                                            // For now, we send via UDP to the peer
                                            if let Err(e) = socket_clone.send_to(output_data, peer_addr).await {
                                                error!("Failed to send IGMP response: {}", e);
                                            } else {
                                                debug!("IGMP sent {} bytes to {}", output_data.len(), peer_addr);
                                                let _ = status_clone.send(format!("[DEBUG] IGMP sent {} bytes to {}", output_data.len(), peer_addr));

                                                // TRACE: Log full payload
                                                let hex_str = hex::encode(output_data);
                                                trace!("IGMP sent (hex): {}", hex_str);
                                                let _ = status_clone.send(format!("[TRACE] IGMP sent (hex): {}", hex_str));

                                                let _ = status_clone.send(format!(
                                                    "→ IGMP response to {} ({} bytes)",
                                                    peer_addr, output_data.len()
                                                ));
                                            }
                                        }
                                    }

                                    // Process async custom actions (join_group/leave_group)
                                    use crate::llm::actions::protocol_trait::ActionResult;
                                    for protocol_result in &execution_result.protocol_results {
                                        if let ActionResult::Custom { name, data } = protocol_result {
                                            match name.as_str() {
                                                "igmp_join_group" => {
                                                    if let Some(group_str) = data.get("group_address")
                                                        .and_then(|v| v.as_str()) {
                                                        if let Ok(group_addr) = group_str.parse::<Ipv4Addr>() {
                                                            let mut state = server_state_clone.lock().await;
                                                            state.joined_groups.insert(group_addr);
                                                            info!("IGMP joined multicast group {}", group_addr);
                                                            let _ = status_clone.send(format!("[INFO] IGMP joined multicast group {}", group_addr));

                                                            // In a real implementation, we would call:
                                                            // socket.join_multicast_v4(&group_addr, &interface_addr)?;
                                                        }
                                                    }
                                                }
                                                "igmp_leave_group" => {
                                                    if let Some(group_str) = data.get("group_address")
                                                        .and_then(|v| v.as_str()) {
                                                        if let Ok(group_addr) = group_str.parse::<Ipv4Addr>() {
                                                            let mut state = server_state_clone.lock().await;
                                                            state.joined_groups.remove(&group_addr);
                                                            info!("IGMP left multicast group {}", group_addr);
                                                            let _ = status_clone.send(format!("[INFO] IGMP left multicast group {}", group_addr));

                                                            // In a real implementation, we would call:
                                                            // socket.leave_multicast_v4(&group_addr, &interface_addr)?;
                                                        }
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("IGMP LLM call failed: {}", e);
                                    let _ = status_clone.send(format!("✗ IGMP LLM error: {}", e));
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("IGMP receive error: {}", e);
                    }
                }
            }
        });

        Ok(local_addr)
    }
}
