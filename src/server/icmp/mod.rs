//! ICMP (Internet Control Message Protocol) server implementation
//!
//! This module provides functionality to capture and respond to ICMP messages at the network layer.
//! It uses raw IP sockets via socket2 and pnet for ICMP packet handling.

pub mod actions;

use anyhow::{Context, Result};
use pnet::packet::icmp::echo_reply::EchoReplyPacket;
use pnet::packet::icmp::echo_request::EchoRequestPacket;
use pnet::packet::icmp::time_exceeded::TimeExceededPacket;
// Note: pnet doesn't provide timestamp packet types
use pnet::packet::icmp::{
    IcmpCode, IcmpPacket, IcmpTypes, MutableIcmpPacket,
};
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::ipv4::{Ipv4Packet, MutableIpv4Packet};
use pnet::packet::Packet;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::Ipv4Addr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::{console_error, console_info, console_trace};
use actions::{
    IcmpProtocol, ICMP_ECHO_REQUEST_EVENT, ICMP_OTHER_MESSAGE_EVENT,
    // ICMP_TIMESTAMP_REQUEST_EVENT, // TODO: Removed - timestamp support requires pnet timestamp packet types
};

/// ICMP server that captures and responds to ICMP messages
pub struct IcmpServer;

impl IcmpServer {
    /// Spawn ICMP server with integrated LLM handling
    pub async fn spawn_with_llm(
        interface: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<String> {
        console_info!(
            status_tx,
            "Starting ICMP capture on interface: {}",
            interface
        );

        let protocol = Arc::new(IcmpProtocol::new());

        // ICMP requires raw sockets, so we run it in a blocking task
        let interface_clone = interface.clone();
        let protocol_clone = protocol.clone();
        tokio::task::spawn_blocking(move || {
            // Create raw ICMP socket
            let socket = match Socket::new(Domain::IPV4, Type::RAW, Some(Protocol::ICMPV4)) {
                Ok(s) => s,
                Err(e) => {
                    console_error!(
                        status_tx,
                        "Failed to create raw ICMP socket (need root/CAP_NET_RAW): {}",
                        e
                    );
                    return;
                }
            };

            // Set socket to non-blocking for timeout handling
            if let Err(e) = socket.set_nonblocking(true) {
                console_error!(status_tx, "Failed to set socket non-blocking: {}", e);
                return;
            }

            // TODO: Bind to specific interface if needed
            // For now, we receive on all interfaces

            console_info!(
                status_tx,
                "ICMP server listening for packets on {}",
                interface_clone
            );

            let runtime = tokio::runtime::Handle::current();

            // Create a separate socket for sending (to avoid conflicts)
            let send_socket = match Socket::new(Domain::IPV4, Type::RAW, Some(Protocol::ICMPV4)) {
                Ok(s) => Arc::new(s),
                Err(e) => {
                    console_error!(status_tx, "Failed to create send socket: {}", e);
                    return;
                }
            };

            // Receive loop
            let mut buffer = vec![std::mem::MaybeUninit::uninit(); 65535];
            loop {
                // Use recv_from with timeout
                match socket.recv_from(&mut buffer) {
                    Ok((n, _src_addr)) => {
                        let data = unsafe {
                            std::slice::from_raw_parts(buffer.as_ptr() as *const u8, n).to_vec()
                        };

                        // DEBUG: Log raw received data
                        trace!("Raw socket received {} bytes: {}", n, hex::encode(&data[..std::cmp::min(n, 60)]));

                        // Parse IP packet
                        let ip_packet = match Ipv4Packet::new(&data) {
                            Some(p) => p,
                            None => {
                                debug!("Failed to parse IPv4 packet");
                                continue;
                            }
                        };

                        // DEBUG: Log IP packet details
                        trace!("IP header length: {}, payload length: {}",
                               ip_packet.get_header_length() * 4,
                               ip_packet.payload().len());

                        // Check if it's ICMP
                        if ip_packet.get_next_level_protocol() != IpNextHeaderProtocols::Icmp {
                            continue;
                        }

                        // DEBUG: Log the payload we're about to parse as ICMP
                        let ip_payload = ip_packet.payload();
                        trace!("IP payload (ICMP data): {}", hex::encode(ip_payload));

                        // Handle IP-in-IP encapsulation (common on loopback)
                        // If the payload starts with an IP header (0x45 = version 4, header length 5),
                        // parse it as an inner IP packet and use its payload instead
                        let icmp_data: Vec<u8>;
                        let icmp_payload = if ip_payload.len() >= 20 && ip_payload[0] == 0x45 {
                            if let Some(inner_ip) = Ipv4Packet::new(ip_payload) {
                                if inner_ip.get_next_level_protocol() == IpNextHeaderProtocols::Icmp {
                                    trace!("Detected IP-in-IP encapsulation on loopback, unwrapping inner packet");
                                    icmp_data = inner_ip.payload().to_vec();
                                    trace!("Inner IP payload (actual ICMP): {}", hex::encode(&icmp_data));
                                    icmp_data.as_slice()
                                } else {
                                    ip_payload
                                }
                            } else {
                                ip_payload
                            }
                        } else {
                            ip_payload
                        };

                        // Parse ICMP packet
                        let icmp_packet = match IcmpPacket::new(icmp_payload) {
                            Some(p) => p,
                            None => {
                                debug!("Failed to parse ICMP packet");
                                continue;
                            }
                        };

                        // Extract IP information
                        let source_ip = ip_packet.get_source();
                        let dest_ip = ip_packet.get_destination();
                        let ttl = ip_packet.get_ttl();
                        let icmp_type = icmp_packet.get_icmp_type();
                        let icmp_code = icmp_packet.get_icmp_code();

                        // DEBUG: Log ICMP type extraction
                        trace!("ICMP type extracted: {} (raw: {}), code: {}",
                               icmp_type.0, icmp_type.0, icmp_code.0);
                        trace!("First 4 bytes of icmp_payload: {:02x} {:02x} {:02x} {:02x}",
                               icmp_payload[0], icmp_payload[1],
                               icmp_payload.get(2).unwrap_or(&0), icmp_payload.get(3).unwrap_or(&0));

                        // DEBUG: Log summary
                        debug!(
                            "ICMP {} from {} to {} (TTL: {})",
                            icmp_type_to_string(icmp_type),
                            source_ip,
                            dest_ip,
                            ttl
                        );
                        let _ = status_tx.send(format!(
                            "[DEBUG] ICMP {} from {} to {} (TTL: {})",
                            icmp_type_to_string(icmp_type),
                            source_ip,
                            dest_ip,
                            ttl
                        ));

                        // TRACE: Log full packet
                        let hex_str = hex::encode(ip_packet.payload());
                        console_trace!(status_tx, "ICMP packet (hex): {}", hex_str);

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let protocol_task_clone = protocol_clone.clone();
                        let send_socket_clone = send_socket.clone();
                        // Store the full ICMP packet (including header) for parsing by specific packet types
                        // Use the unwrapped payload in case of IP-in-IP encapsulation
                        let icmp_packet_data = icmp_payload.to_vec();

                        // Spawn async task to handle packet with LLM
                        runtime.spawn(async move {
                            // Build event based on ICMP type
                            let event = match icmp_type {
                                IcmpTypes::EchoRequest => {
                                    // Parse echo request
                                    if let Some(echo_req) =
                                        EchoRequestPacket::new(&icmp_packet_data)
                                    {
                                        let identifier = echo_req.get_identifier();
                                        let sequence = echo_req.get_sequence_number();
                                        let payload_hex = hex::encode(echo_req.payload());

                                        Event::new(
                                            &ICMP_ECHO_REQUEST_EVENT,
                                            serde_json::json!({
                                                "source_ip": source_ip.to_string(),
                                                "destination_ip": dest_ip.to_string(),
                                                "identifier": identifier.to_string(),
                                                "sequence": sequence.to_string(),
                                                "payload_hex": payload_hex,
                                                "ttl": ttl,
                                            }),
                                        )
                                    } else {
                                        debug!("Failed to parse ICMP echo request");
                                        return;
                                    }
                                }
                                /* TODO: Timestamp support requires pnet to add timestamp packet types
                                IcmpTypes::Timestamp => {
                                    // Parse timestamp request
                                    if let Some(ts_req) = TimestampPacket::new(&icmp_payload) {
                                        let identifier = ts_req.get_identifier();
                                        let sequence = ts_req.get_sequence_number();
                                        let originate_timestamp = ts_req.get_originate_timestamp();

                                        Event::new(
                                            &ICMP_TIMESTAMP_REQUEST_EVENT,
                                            serde_json::json!({
                                                "source_ip": source_ip.to_string(),
                                                "destination_ip": dest_ip.to_string(),
                                                "identifier": identifier,
                                                "sequence": sequence,
                                                "originate_timestamp": originate_timestamp,
                                            }),
                                        )
                                    } else {
                                        debug!("Failed to parse ICMP timestamp request");
                                        return;
                                    }
                                }
                                */
                                _ => {
                                    // Other ICMP types
                                    Event::new(
                                        &ICMP_OTHER_MESSAGE_EVENT,
                                        serde_json::json!({
                                            "source_ip": source_ip.to_string(),
                                            "destination_ip": dest_ip.to_string(),
                                            "icmp_type": icmp_type.0,
                                            "icmp_code": icmp_code.0,
                                            "packet_hex": hex::encode(&icmp_packet_data),
                                        }),
                                    )
                                }
                            };

                            debug!("ICMP calling LLM for {} packet", icmp_type_to_string(icmp_type));
                            let _ = status_clone.send(format!(
                                "[DEBUG] ICMP calling LLM for {} packet",
                                icmp_type_to_string(icmp_type)
                            ));

                            match call_llm(
                                &llm_clone,
                                &state_clone,
                                server_id,
                                None,
                                &event,
                                protocol_task_clone.as_ref(),
                            )
                            .await
                            {
                                Ok(execution_result) => {
                                    for message in &execution_result.messages {
                                        info!("{}", message);
                                        let _ = status_clone.send(format!("[INFO] {}", message));
                                    }

                                    debug!(
                                        "ICMP got {} protocol results",
                                        execution_result.protocol_results.len()
                                    );
                                    let _ = status_clone.send(format!(
                                        "[DEBUG] ICMP got {} protocol results",
                                        execution_result.protocol_results.len()
                                    ));

                                    // Send ICMP replies if any
                                    for protocol_result in execution_result.protocol_results {
                                        if let Some(output_data) =
                                            protocol_result.get_all_output().first()
                                        {
                                            // Send packet via raw socket
                                            // Extract destination from the IP header in output_data
                                            if let Some(ip_pkt) = Ipv4Packet::new(output_data) {
                                                let dest_addr = std::net::SocketAddr::from((
                                                    ip_pkt.get_destination(),
                                                    0,
                                                ));

                                                match send_socket_clone.send_to(output_data, &dest_addr.into()) {
                                                    Ok(_) => {
                                                        debug!(
                                                            "ICMP sent {} bytes to {}",
                                                            output_data.len(),
                                                            dest_addr.ip()
                                                        );
                                                        let _ = status_clone.send(format!(
                                                            "[DEBUG] ICMP sent {} bytes to {}",
                                                            output_data.len(),
                                                            dest_addr.ip()
                                                        ));

                                                        trace!(
                                                            "ICMP reply (hex): {}",
                                                            hex::encode(output_data)
                                                        );
                                                        let _ = status_clone.send(format!(
                                                            "[TRACE] ICMP reply (hex): {}",
                                                            hex::encode(output_data)
                                                        ));
                                                    }
                                                    Err(e) => {
                                                        error!("Failed to send ICMP reply: {}", e);
                                                        let _ = status_clone.send(format!(
                                                            "[ERROR] Failed to send ICMP reply: {}",
                                                            e
                                                        ));
                                                    }
                                                }
                                            } else {
                                                error!("Failed to parse IP packet from LLM output");
                                            }
                                        }
                                    }

                                    let _ = status_clone.send(format!(
                                        "→ ICMP {} processed: {} -> {}",
                                        icmp_type_to_string(icmp_type),
                                        source_ip,
                                        dest_ip
                                    ));
                                }
                                Err(e) => {
                                    error!("ICMP LLM call failed: {}", e);
                                    let _ = status_clone.send(format!("✗ ICMP LLM error: {}", e));
                                }
                            }
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        // No data available, sleep briefly
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        continue;
                    }
                    Err(e) => {
                        console_error!(status_tx, "ICMP receive error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(interface)
    }

    /// Helper function to build an ICMP echo reply packet with IP header
    pub fn build_echo_reply(
        source_ip: Ipv4Addr,
        dest_ip: Ipv4Addr,
        identifier: u16,
        sequence: u16,
        payload: &[u8],
    ) -> Vec<u8> {
        use pnet::packet::icmp::echo_reply::MutableEchoReplyPacket;
        use pnet::packet::icmp::IcmpTypes;
        use pnet::packet::ipv4::checksum;

        // ICMP echo reply: 8 bytes header + payload
        let icmp_size = 8 + payload.len();
        let mut icmp_buffer = vec![0u8; icmp_size];

        {
            let mut echo_reply =
                MutableEchoReplyPacket::new(&mut icmp_buffer).unwrap();
            echo_reply.set_icmp_type(IcmpTypes::EchoReply);
            echo_reply.set_icmp_code(IcmpCode::new(0));
            echo_reply.set_identifier(identifier);
            echo_reply.set_sequence_number(sequence);
            echo_reply.set_payload(payload);
        }

        // Calculate ICMP checksum
        let icmp_checksum = {
            let icmp_packet = MutableIcmpPacket::new(&mut icmp_buffer).unwrap();
            pnet::packet::icmp::checksum(&icmp_packet.to_immutable())
        };

        {
            let mut echo_reply =
                MutableEchoReplyPacket::new(&mut icmp_buffer).unwrap();
            echo_reply.set_checksum(icmp_checksum);
        }

        // Wrap in IP packet
        let ip_size = 20 + icmp_size;
        let mut ip_buffer = vec![0u8; ip_size];

        {
            let mut ip_packet = MutableIpv4Packet::new(&mut ip_buffer).unwrap();
            ip_packet.set_version(4);
            ip_packet.set_header_length(5); // 5 * 4 = 20 bytes
            ip_packet.set_dscp(0);
            ip_packet.set_ecn(0);
            ip_packet.set_total_length(ip_size as u16);
            ip_packet.set_identification(0);
            ip_packet.set_flags(0);
            ip_packet.set_fragment_offset(0);
            ip_packet.set_ttl(64);
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

    /// Helper function to build an ICMP destination unreachable packet
    pub fn build_destination_unreachable(
        source_ip: Ipv4Addr,
        dest_ip: Ipv4Addr,
        code: u8,
        original_packet: &[u8],
    ) -> Vec<u8> {
        use pnet::packet::icmp::destination_unreachable::MutableDestinationUnreachablePacket;
        use pnet::packet::icmp::IcmpTypes;
        use pnet::packet::ipv4::checksum;

        // Take first 28 bytes of original packet (IP header + 8 bytes of data)
        let payload_len = std::cmp::min(original_packet.len(), 28);
        let payload = &original_packet[..payload_len];

        // ICMP destination unreachable: 8 bytes header + original packet fragment
        let icmp_size = 8 + payload.len();
        let mut icmp_buffer = vec![0u8; icmp_size];

        {
            let mut dest_unreach =
                MutableDestinationUnreachablePacket::new(&mut icmp_buffer).unwrap();
            dest_unreach.set_icmp_type(IcmpTypes::DestinationUnreachable);
            dest_unreach.set_icmp_code(IcmpCode::new(code));
            dest_unreach.set_payload(payload);
        }

        // Calculate ICMP checksum
        let icmp_checksum = {
            let icmp_packet = MutableIcmpPacket::new(&mut icmp_buffer).unwrap();
            pnet::packet::icmp::checksum(&icmp_packet.to_immutable())
        };

        {
            let mut dest_unreach =
                MutableDestinationUnreachablePacket::new(&mut icmp_buffer).unwrap();
            dest_unreach.set_checksum(icmp_checksum);
        }

        // Wrap in IP packet
        let ip_size = 20 + icmp_size;
        let mut ip_buffer = vec![0u8; ip_size];

        {
            let mut ip_packet = MutableIpv4Packet::new(&mut ip_buffer).unwrap();
            ip_packet.set_version(4);
            ip_packet.set_header_length(5);
            ip_packet.set_total_length(ip_size as u16);
            ip_packet.set_ttl(64);
            ip_packet.set_next_level_protocol(IpNextHeaderProtocols::Icmp);
            ip_packet.set_source(source_ip);
            ip_packet.set_destination(dest_ip);
            ip_packet.set_payload(&icmp_buffer);

            let ip_checksum = checksum(&ip_packet.to_immutable());
            ip_packet.set_checksum(ip_checksum);
        }

        ip_buffer
    }

    /// Helper function to build an ICMP time exceeded packet
    pub fn build_time_exceeded(
        source_ip: Ipv4Addr,
        dest_ip: Ipv4Addr,
        code: u8,
        original_packet: &[u8],
    ) -> Vec<u8> {
        use pnet::packet::icmp::time_exceeded::MutableTimeExceededPacket;
        use pnet::packet::icmp::IcmpTypes;
        use pnet::packet::ipv4::checksum;

        // Take first 28 bytes of original packet
        let payload_len = std::cmp::min(original_packet.len(), 28);
        let payload = &original_packet[..payload_len];

        let icmp_size = 8 + payload.len();
        let mut icmp_buffer = vec![0u8; icmp_size];

        {
            let mut time_exceeded = MutableTimeExceededPacket::new(&mut icmp_buffer).unwrap();
            time_exceeded.set_icmp_type(IcmpTypes::TimeExceeded);
            time_exceeded.set_icmp_code(IcmpCode::new(code));
            time_exceeded.set_payload(payload);
        }

        // Calculate ICMP checksum
        let icmp_checksum = {
            let icmp_packet = MutableIcmpPacket::new(&mut icmp_buffer).unwrap();
            pnet::packet::icmp::checksum(&icmp_packet.to_immutable())
        };

        {
            let mut time_exceeded = MutableTimeExceededPacket::new(&mut icmp_buffer).unwrap();
            time_exceeded.set_checksum(icmp_checksum);
        }

        // Wrap in IP packet
        let ip_size = 20 + icmp_size;
        let mut ip_buffer = vec![0u8; ip_size];

        {
            let mut ip_packet = MutableIpv4Packet::new(&mut ip_buffer).unwrap();
            ip_packet.set_version(4);
            ip_packet.set_header_length(5);
            ip_packet.set_total_length(ip_size as u16);
            ip_packet.set_ttl(64);
            ip_packet.set_next_level_protocol(IpNextHeaderProtocols::Icmp);
            ip_packet.set_source(source_ip);
            ip_packet.set_destination(dest_ip);
            ip_packet.set_payload(&icmp_buffer);

            let ip_checksum = checksum(&ip_packet.to_immutable());
            ip_packet.set_checksum(ip_checksum);
        }

        ip_buffer
    }

    /* TODO: Timestamp support requires pnet to add timestamp_reply packet types
    /// Helper function to build an ICMP timestamp reply packet
    pub fn build_timestamp_reply(
        source_ip: Ipv4Addr,
        dest_ip: Ipv4Addr,
        identifier: u16,
        sequence: u16,
        originate_timestamp: u32,
    ) -> Vec<u8> {
        use pnet::packet::icmp::timestamp_reply::MutableTimestampReplyPacket;
        use pnet::packet::icmp::IcmpTypes;
        use pnet::packet::ipv4::checksum;

        // Get current time in milliseconds since midnight UT
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u32;
        let receive_timestamp = now;
        let transmit_timestamp = now;

        let icmp_size = 20; // Timestamp reply is always 20 bytes
        let mut icmp_buffer = vec![0u8; icmp_size];

        {
            let mut ts_reply = MutableTimestampReplyPacket::new(&mut icmp_buffer).unwrap();
            ts_reply.set_icmp_type(IcmpTypes::TimestampReply);
            ts_reply.set_icmp_code(IcmpCode::new(0));
            ts_reply.set_identifier(identifier);
            ts_reply.set_sequence_number(sequence);
            ts_reply.set_originate_timestamp(originate_timestamp);
            ts_reply.set_receive_timestamp(receive_timestamp);
            ts_reply.set_transmit_timestamp(transmit_timestamp);

            let icmp_packet = MutableIcmpPacket::new(&mut icmp_buffer).unwrap();
            let checksum = pnet::packet::icmp::checksum(&icmp_packet.to_immutable());
            ts_reply.set_checksum(checksum);
        }

        // Wrap in IP packet
        let ip_size = 20 + icmp_size;
        let mut ip_buffer = vec![0u8; ip_size];

        {
            let mut ip_packet = MutableIpv4Packet::new(&mut ip_buffer).unwrap();
            ip_packet.set_version(4);
            ip_packet.set_header_length(5);
            ip_packet.set_total_length(ip_size as u16);
            ip_packet.set_ttl(64);
            ip_packet.set_next_level_protocol(IpNextHeaderProtocols::Icmp);
            ip_packet.set_source(source_ip);
            ip_packet.set_destination(dest_ip);
            ip_packet.set_payload(&icmp_buffer);

            let checksum = checksum(&ip_packet.to_immutable());
            ip_packet.set_checksum(checksum);
        }

        ip_buffer
    }
    */
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
