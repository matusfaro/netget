//! ICMP (Internet Control Message Protocol) client implementation
//!
//! This module provides functionality to send ICMP messages and receive responses.
//! It uses raw IP sockets via socket2 and pnet for ICMP packet handling.

pub mod actions;

use anyhow::{Context, Result};
use pnet::packet::icmp::echo_reply::EchoReplyPacket;
use pnet::packet::icmp::echo_request::MutableEchoRequestPacket;
use pnet::packet::icmp::time_exceeded::TimeExceededPacket;
// Note: pnet doesn't provide timestamp_reply packet types
use pnet::packet::icmp::{destination_unreachable::DestinationUnreachablePacket, IcmpPacket};
use pnet::packet::icmp::{IcmpCode, IcmpTypes, MutableIcmpPacket};
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::ipv4::{Ipv4Packet, MutableIpv4Packet};
use pnet::packet::Packet;
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};

use crate::state::ClientId;
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::{Client, ClientActionResult};
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::{console_debug, console_info, console_trace};

pub use actions::IcmpClientProtocol;
use actions::{
    ICMP_CLIENT_CONNECTED_EVENT, ICMP_DEST_UNREACHABLE_EVENT, ICMP_ECHO_REPLY_EVENT,
    ICMP_TIME_EXCEEDED_EVENT, ICMP_TIMEOUT_EVENT,
};

/// Connection state for LLM processing
#[derive(Debug, Clone, PartialEq)]
enum ConnectionState {
    Idle,
    Processing,
    Accumulating,
}

/// Per-client data for LLM handling
struct ClientData {
    state: ConnectionState,
    memory: String,
}

/// ICMP client that sends requests and receives responses
pub struct IcmpClient;

/// Pending ICMP request tracking
#[derive(Clone)]
struct PendingRequest {
    sent_at: Instant,
    identifier: u16,
    sequence: u16,
    destination_ip: Ipv4Addr,
}

impl IcmpClient {
    /// Connect ICMP client with LLM action handling
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Parse target IP from remote_addr
        let target_ip: Ipv4Addr = remote_addr
            .split(':')
            .next()
            .context("Invalid remote address format")?
            .parse()
            .context("Invalid IPv4 address")?;

        console_info!(status_tx, "ICMP client connecting to {}", target_ip);

        // Create raw ICMP socket
        let socket = Socket::new(Domain::IPV4, Type::RAW, Some(Protocol::ICMPV4))
            .context("Failed to create raw ICMP socket (need root/CAP_NET_RAW)")?;

        // Set socket to non-blocking
        socket
            .set_nonblocking(true)
            .context("Failed to set socket non-blocking")?;

        let local_addr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);

        // Shared state for pending requests
        let pending_requests: Arc<Mutex<HashMap<(u16, u16), PendingRequest>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Initialize client data
        let client_data = Arc::new(Mutex::new(ClientData {
            state: ConnectionState::Idle,
            memory: String::new(),
        }));

        let socket = Arc::new(socket);
        let protocol = Arc::new(IcmpClientProtocol::new());

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let event = Event::new(
                &ICMP_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "local_addr": local_addr.to_string(),
                    "target_ip": target_ip.to_string(),
                }),
            );

            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &client_data.lock().await.memory,
                Some(&event),
                protocol.as_ref(),
                &status_tx,
            )
            .await
            {
                Ok(result) => {
                    // Update memory if provided
                    if let Some(new_memory) = result.memory_updates {
                        client_data.lock().await.memory = new_memory;
                    }

                    // Execute initial actions
                    Self::execute_actions(
                        result.actions,
                        &socket,
                        &pending_requests,
                        target_ip,
                        &status_tx,
                        protocol.as_ref(),
                    )
                    .await?;
                }
                Err(e) => {
                    error!("ICMP client LLM call failed: {}", e);
                }
            }
        }

        // Spawn receive loop
        let socket_clone = socket.clone();
        let llm_clone = llm_client.clone();
        let state_clone = app_state.clone();
        let status_clone = status_tx.clone();
        let protocol_clone = protocol.clone();
        let pending_clone = pending_requests.clone();
        let client_data_clone = client_data.clone();

        tokio::spawn(async move {
            let mut buffer = vec![std::mem::MaybeUninit::uninit(); 65535];

            loop {
                // Try to receive packet (non-blocking)
                match socket_clone.recv_from(&mut buffer) {
                    Ok((n, _src_addr)) => {
                        let data = unsafe {
                            std::slice::from_raw_parts(buffer.as_ptr() as *const u8, n).to_vec()
                        };

                        // Parse IP packet
                        let ip_packet = match Ipv4Packet::new(&data) {
                            Some(p) => p,
                            None => continue,
                        };

                        // Check if it's ICMP
                        if ip_packet.get_next_level_protocol() != IpNextHeaderProtocols::Icmp {
                            continue;
                        }

                        // Parse ICMP packet
                        let icmp_packet = match IcmpPacket::new(ip_packet.payload()) {
                            Some(p) => p,
                            None => continue,
                        };

                        let source_ip = ip_packet.get_source();
                        let ttl = ip_packet.get_ttl();
                        let icmp_type = icmp_packet.get_icmp_type();
                        let icmp_payload = icmp_packet.payload();

                        console_debug!(
                            status_clone,
                            "ICMP client received {} from {}",
                            icmp_type_to_string(icmp_type),
                            source_ip
                        );

                        // Process based on ICMP type
                        let event_opt = match icmp_type {
                            IcmpTypes::EchoReply => {
                                if let Some(echo_reply) = EchoReplyPacket::new(icmp_payload) {
                                    let identifier = echo_reply.get_identifier();
                                    let sequence = echo_reply.get_sequence_number();
                                    let payload_hex = hex::encode(echo_reply.payload());

                                    // Calculate RTT
                                    let rtt_ms = {
                                        let mut pending = pending_clone.lock().await;
                                        if let Some(req) = pending.remove(&(identifier, sequence)) {
                                            req.sent_at.elapsed().as_millis() as u64
                                        } else {
                                            0
                                        }
                                    };

                                    Some(Event::new(
                                        &ICMP_ECHO_REPLY_EVENT,
                                        serde_json::json!({
                                            "source_ip": source_ip.to_string(),
                                            "identifier": identifier,
                                            "sequence": sequence,
                                            "rtt_ms": rtt_ms,
                                            "ttl": ttl,
                                            "payload_hex": payload_hex,
                                        }),
                                    ))
                                } else {
                                    None
                                }
                            }
                            IcmpTypes::DestinationUnreachable => {
                                if let Some(dest_unreach) =
                                    DestinationUnreachablePacket::new(icmp_payload)
                                {
                                    let code = dest_unreach.get_icmp_code().0;
                                    Some(Event::new(
                                        &ICMP_DEST_UNREACHABLE_EVENT,
                                        serde_json::json!({
                                            "source_ip": source_ip.to_string(),
                                            "code": code,
                                        }),
                                    ))
                                } else {
                                    None
                                }
                            }
                            IcmpTypes::TimeExceeded => {
                                if let Some(time_exceeded) =
                                    TimeExceededPacket::new(icmp_payload)
                                {
                                    let code = time_exceeded.get_icmp_code().0;
                                    Some(Event::new(
                                        &ICMP_TIME_EXCEEDED_EVENT,
                                        serde_json::json!({
                                            "source_ip": source_ip.to_string(),
                                            "code": code,
                                        }),
                                    ))
                                } else {
                                    None
                                }
                            }
                            /* TODO: Timestamp support requires pnet to add timestamp_reply packet types
                            IcmpTypes::TimestampReply => {
                                if let Some(_ts_reply) = TimestampReplyPacket::new(icmp_payload) {
                                    // Could add timestamp reply event here
                                    None
                                } else {
                                    None
                                }
                            }
                            */
                            _ => None,
                        };

                        if let Some(event) = event_opt {
                            // Get instruction for LLM call
                            if let Some(instruction) = state_clone.get_instruction_for_client(client_id).await {
                                // Call LLM
                                match call_llm_for_client(
                                    &llm_clone,
                                    &state_clone,
                                    client_id.to_string(),
                                    &instruction,
                                    &client_data_clone.lock().await.memory,
                                    Some(&event),
                                    protocol_clone.as_ref(),
                                    &status_clone,
                                )
                                .await
                                {
                                    Ok(result) => {
                                        // Update memory if provided
                                        if let Some(new_memory) = result.memory_updates {
                                            client_data_clone.lock().await.memory = new_memory;
                                        }

                                        // Execute actions
                                        if let Err(e) = Self::execute_actions(
                                            result.actions,
                                            &socket_clone,
                                            &pending_clone,
                                            target_ip,
                                            &status_clone,
                                            protocol_clone.as_ref(),
                                        )
                                        .await
                                        {
                                            error!("Failed to execute ICMP action: {}", e);
                                        }
                                    }
                                    Err(e) => {
                                        error!("ICMP client LLM call failed: {}", e);
                                    }
                                }
                            }
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        // No data available
                        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                        continue;
                    }
                    Err(e) => {
                        error!("ICMP receive error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Execute actions from LLM
    async fn execute_actions(
        actions: Vec<serde_json::Value>,
        socket: &Arc<Socket>,
        pending_requests: &Arc<Mutex<HashMap<(u16, u16), PendingRequest>>>,
        target_ip: Ipv4Addr,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: &IcmpClientProtocol,
    ) -> Result<()> {
        for action in actions {
            match protocol.execute_action(action)? {
                ClientActionResult::Custom { name, data } => {
                    if name == "send_echo_request" {
                        let dest_ip: Ipv4Addr = data["destination_ip"]
                            .as_str()
                            .unwrap_or(&target_ip.to_string())
                            .parse()?;
                        let identifier = data["identifier"].as_u64().unwrap_or(1234) as u16;
                        let sequence = data["sequence"].as_u64().unwrap_or(1) as u16;
                        let payload_hex = data["payload_hex"].as_str().unwrap_or("");
                        let ttl = data["ttl"].as_u64().unwrap_or(64) as u8;

                        let payload = if payload_hex.is_empty() {
                            Vec::new()
                        } else {
                            hex::decode(payload_hex)?
                        };

                        // Build and send echo request
                        let packet = Self::build_echo_request(
                            Ipv4Addr::UNSPECIFIED,
                            dest_ip,
                            identifier,
                            sequence,
                            &payload,
                            ttl,
                        );

                        let dest_addr = SocketAddr::new(std::net::IpAddr::V4(dest_ip), 0);
                        socket.send_to(&packet, &dest_addr.into())?;

                        // Track pending request
                        {
                            let mut pending = pending_requests.lock().await;
                            pending.insert(
                                (identifier, sequence),
                                PendingRequest {
                                    sent_at: Instant::now(),
                                    identifier,
                                    sequence,
                                    destination_ip: dest_ip,
                                },
                            );
                        }

                        console_debug!(
                            status_tx,
                            "ICMP sent echo request to {} (id={}, seq={})",
                            dest_ip,
                            identifier,
                            sequence
                        );
                    /* TODO: Timestamp support requires pnet to add timestamp packet types
                    } else if name == "send_timestamp_request" {
                        // TODO: Implement timestamp request
                        debug!("Timestamp request not yet implemented");
                    */
                    }
                }
                ClientActionResult::WaitForMore => {
                    // Just continue listening
                    debug!("ICMP client waiting for more responses");
                }
                ClientActionResult::Disconnect => {
                    debug!("ICMP client disconnect requested");
                    break;
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Build an ICMP echo request packet with IP header
    fn build_echo_request(
        source_ip: Ipv4Addr,
        dest_ip: Ipv4Addr,
        identifier: u16,
        sequence: u16,
        payload: &[u8],
        ttl: u8,
    ) -> Vec<u8> {
        use pnet::packet::icmp::echo_request::MutableEchoRequestPacket;
        use pnet::packet::ipv4::checksum;

        // ICMP echo request: 8 bytes header + payload
        let icmp_size = 8 + payload.len();
        let mut icmp_buffer = vec![0u8; icmp_size];

        {
            let mut echo_req = MutableEchoRequestPacket::new(&mut icmp_buffer).unwrap();
            echo_req.set_icmp_type(IcmpTypes::EchoRequest);
            echo_req.set_icmp_code(IcmpCode::new(0));
            echo_req.set_identifier(identifier);
            echo_req.set_sequence_number(sequence);
            echo_req.set_payload(payload);
        }

        // Calculate ICMP checksum
        let icmp_checksum = {
            let icmp_packet = MutableIcmpPacket::new(&mut icmp_buffer).unwrap();
            pnet::packet::icmp::checksum(&icmp_packet.to_immutable())
        };

        {
            let mut echo_req = MutableEchoRequestPacket::new(&mut icmp_buffer).unwrap();
            echo_req.set_checksum(icmp_checksum);
        }

        // Wrap in IP packet
        let ip_size = 20 + icmp_size;
        let mut ip_buffer = vec![0u8; ip_size];

        {
            let mut ip_packet = MutableIpv4Packet::new(&mut ip_buffer).unwrap();
            ip_packet.set_version(4);
            ip_packet.set_header_length(5);
            ip_packet.set_dscp(0);
            ip_packet.set_ecn(0);
            ip_packet.set_total_length(ip_size as u16);
            ip_packet.set_identification(0);
            ip_packet.set_flags(0);
            ip_packet.set_fragment_offset(0);
            ip_packet.set_ttl(ttl);
            ip_packet.set_next_level_protocol(IpNextHeaderProtocols::Icmp);
            ip_packet.set_source(source_ip);
            ip_packet.set_destination(dest_ip);
            ip_packet.set_payload(&icmp_buffer);

            // Calculate IP checksum
            let ip_checksum = checksum(&ip_packet.to_immutable());
            ip_packet.set_checksum(ip_checksum);
        }

        ip_buffer
    }
}

/// Convert ICMP type to human-readable string
fn icmp_type_to_string(icmp_type: pnet::packet::icmp::IcmpType) -> &'static str {
    match icmp_type {
        IcmpTypes::EchoReply => "ECHO_REPLY",
        IcmpTypes::EchoRequest => "ECHO_REQUEST",
        IcmpTypes::DestinationUnreachable => "DEST_UNREACHABLE",
        IcmpTypes::SourceQuench => "SOURCE_QUENCH",
        IcmpTypes::RedirectMessage => "REDIRECT",
        IcmpTypes::TimeExceeded => "TIME_EXCEEDED",
        IcmpTypes::ParameterProblem => "PARAMETER_PROBLEM",
        IcmpTypes::Timestamp => "TIMESTAMP",
        IcmpTypes::TimestampReply => "TIMESTAMP_REPLY",
        IcmpTypes::InformationRequest => "INFO_REQUEST",
        IcmpTypes::InformationReply => "INFO_REPLY",
        IcmpTypes::AddressMaskRequest => "ADDRMASK_REQUEST",
        IcmpTypes::AddressMaskReply => "ADDRMASK_REPLY",
        IcmpTypes::Traceroute => "TRACEROUTE",
        _ => "UNKNOWN",
    }
}
