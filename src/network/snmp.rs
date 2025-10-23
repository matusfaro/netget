//! SNMP agent implementation using rasn-snmp library

use crate::events::types::{AppEvent, NetworkEvent};
use crate::network::connection::ConnectionId;
use anyhow::Result;
use bytes::Bytes;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{error, info, debug};

// SNMP protocol support
use rasn_snmp::{v1, v2c, v2};
use rasn::{ber, types::Integer};

/// Parsed SNMP message information
#[derive(Debug)]
pub struct ParsedSnmpInfo {
    pub description: String,
    pub request_type: String,
    pub version: u8,
    pub request_id: i32,
    pub community: Vec<u8>,
}

/// SNMP server that forwards requests to LLM
pub struct SnmpServer {
    event_tx: mpsc::UnboundedSender<AppEvent>,
    socket: Arc<UdpSocket>,
}

impl SnmpServer {
    /// Create a new SNMP agent
    pub async fn new(
        addr: SocketAddr,
        event_tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Result<Self> {
        // SNMP typically uses port 161
        let socket = Arc::new(UdpSocket::bind(addr).await?);
        info!("SNMP agent listening on {}", socket.local_addr()?);

        // Send listening event
        event_tx.send(AppEvent::Network(NetworkEvent::Listening {
            addr: socket.local_addr()?,
        }))?;

        Ok(Self {
            event_tx,
            socket,
        })
    }

    /// Start the SNMP agent
    pub async fn start(self) -> Result<()> {
        let mut buffer = vec![0u8; 65535]; // SNMP messages can be large
        let socket = self.socket.clone();

        loop {
            match self.socket.recv_from(&mut buffer).await {
                Ok((n, peer_addr)) => {
                    let data = buffer[..n].to_vec();

                    // Create connection ID for this SNMP request
                    let connection_id = ConnectionId::new();

                    // Don't send Connected event for UDP - it's connectionless

                    // Parse the SNMP message and get request details
                    let parsed = match Self::parse_snmp_message(&data) {
                        Ok(p) => p,
                        Err(e) => {
                            error!("Failed to parse SNMP message: {}", e);
                            continue;
                        }
                    };

                    // Create a oneshot channel for the response
                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();

                    // Send UDP request event with the parsed SNMP info and response channel
                    // Include instructions for the LLM on how to format the response
                    let llm_prompt = format!(
                        "{}\n\n\
                        IMPORTANT: You are acting as an SNMP agent. Based on the request above (type: {}), \
                        generate an appropriate SNMP response.\n\n\
                        Respond with a JSON object containing the SNMP data:\n\
                        {{\n\
                        \"error\": false,  // true if there's an error\n\
                        \"error_message\": \"\",  // error description if any\n\
                        \"variables\": [      // Array of OID-value pairs to return\n\
                            {{\n\
                                \"oid\": \"1.3.6.1.2.1.1.1.0\",  // System description OID\n\
                                \"type\": \"string\",  // string|integer|counter|gauge|timeticks|null\n\
                                \"value\": \"NetGet SNMP Agent v1.0 - LLM Controlled\"\n\
                            }},\n\
                            {{\n\
                                \"oid\": \"1.3.6.1.2.1.1.3.0\",  // System uptime OID\n\
                                \"type\": \"timeticks\",\n\
                                \"value\": 123456\n\
                            }}\n\
                        ]\n\
                        }}\n\n\
                        Common OIDs:\n\
                        - 1.3.6.1.2.1.1.1.0 = sysDescr (system description)\n\
                        - 1.3.6.1.2.1.1.2.0 = sysObjectID (system OID)\n\
                        - 1.3.6.1.2.1.1.3.0 = sysUpTime (uptime in timeticks)\n\
                        - 1.3.6.1.2.1.1.4.0 = sysContact (contact info)\n\
                        - 1.3.6.1.2.1.1.5.0 = sysName (system name)\n\
                        - 1.3.6.1.2.1.1.6.0 = sysLocation (location)\n\n\
                        Generate appropriate values for the requested OIDs.",
                        parsed.description,
                        parsed.request_type
                    );

                    let _ = self.event_tx.send(AppEvent::Network(NetworkEvent::UdpRequest {
                        connection_id,
                        peer_addr,
                        data: Bytes::from(data.to_vec()),  // Send raw SNMP data, not the prompt
                        response_tx,
                    }));

                    // Clone socket for response handling
                    let socket_clone = socket.clone();

                    // Extract fields needed for response building
                    let version = parsed.version;
                    let request_id = parsed.request_id;
                    let community = parsed.community.clone();

                    // Spawn task to handle the response
                    tokio::spawn(async move {
                        match response_rx.await {
                            Ok(udp_response) => {
                                let llm_output = String::from_utf8_lossy(&udp_response.data);
                                debug!("LLM SNMP response: {}", llm_output);

                                // Try to build SNMP response based on LLM output
                                match Self::build_snmp_response(&llm_output, version, request_id, &community) {
                                    Ok(snmp_response) => {
                                        if let Err(e) = socket_clone.send_to(&snmp_response, peer_addr).await {
                                            error!("Failed to send SNMP response: {}", e);
                                        } else {
                                            debug!("Sent SNMP response to {}: {} bytes", peer_addr, snmp_response.len());
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to build SNMP response: {}", e);
                                        // Send a simple error response
                                        if let Ok(error_response) = Self::build_error_response(version, request_id, &community) {
                                            let _ = socket_clone.send_to(&error_response, peer_addr).await;
                                        }
                                    }
                                }
                            }
                            Err(_) => {
                                debug!("SNMP response channel closed");
                            }
                        }
                    });
                }
                Err(e) => {
                    error!("SNMP receive error: {}", e);
                }
            }
        }
    }

    /// Parse SNMP message and extract relevant information
    pub fn parse_snmp_message(data: &[u8]) -> Result<ParsedSnmpInfo> {
        // Try to decode as SNMPv2c first (most common)
        if let Ok(msg) = ber::decode::<v2c::Message<v2::Pdus>>(data) {
            let request_type = Self::get_v2_pdu_type(&msg.data);
            let request_id = Self::get_v2_request_id(&msg.data);
            return Ok(ParsedSnmpInfo {
                description: Self::format_v2c_message(&msg),
                request_type,
                version: 1, // v2c uses version 1 in the packet
                request_id,
                community: msg.community.to_vec(),
            });
        }

        // Try SNMPv1
        if let Ok(msg) = ber::decode::<v1::Message<v1::Pdus>>(data) {
            let request_type = Self::get_v1_pdu_type(&msg.data);
            let request_id = Self::get_v1_request_id(&msg.data);
            return Ok(ParsedSnmpInfo {
                description: Self::format_v1_message(&msg),
                request_type,
                version: 0,
                request_id,
                community: msg.community.to_vec(),
            });
        }

        // If we can't parse it, return error
        Err(anyhow::anyhow!("Failed to parse SNMP message: {} bytes",
                    data.len()))
    }

    /// Get request ID for v2
    fn get_v2_request_id(pdu: &v2::Pdus) -> i32 {
        match pdu {
            v2::Pdus::GetRequest(p) => p.0.request_id,
            v2::Pdus::GetNextRequest(p) => p.0.request_id,
            v2::Pdus::GetBulkRequest(p) => p.0.request_id,
            v2::Pdus::SetRequest(p) => p.0.request_id,
            v2::Pdus::Response(p) => p.0.request_id,
            v2::Pdus::InformRequest(p) => p.0.request_id,
            v2::Pdus::Trap(p) => p.0.request_id,
            v2::Pdus::Report(p) => p.0.request_id,
        }
    }

    /// Get request ID for v1
    fn get_v1_request_id(pdu: &v1::Pdus) -> i32 {
        let integer = match pdu {
            v1::Pdus::GetRequest(p) => &p.0.request_id,
            v1::Pdus::GetNextRequest(p) => &p.0.request_id,
            v1::Pdus::GetResponse(p) => &p.0.request_id,
            v1::Pdus::SetRequest(p) => &p.0.request_id,
            _ => return 0,
        };

        // Convert Integer to i32
        match integer {
            Integer::Primitive(val) => *val as i32,
            Integer::Variable(big) => {
                // Try to convert BigInt to i32, default to 0 if out of range
                big.to_string().parse::<i32>().unwrap_or(0)
            }
        }
    }

    /// Get PDU type for v2
    fn get_v2_pdu_type(pdu: &v2::Pdus) -> String {
        match pdu {
            v2::Pdus::GetRequest(_) => "GetRequest",
            v2::Pdus::GetNextRequest(_) => "GetNextRequest",
            v2::Pdus::GetBulkRequest(_) => "GetBulkRequest",
            v2::Pdus::SetRequest(_) => "SetRequest",
            v2::Pdus::Response(_) => "Response",
            v2::Pdus::InformRequest(_) => "InformRequest",
            v2::Pdus::Trap(_) => "Trap",
            v2::Pdus::Report(_) => "Report",
        }.to_string()
    }

    /// Get PDU type for v1
    fn get_v1_pdu_type(pdu: &v1::Pdus) -> String {
        match pdu {
            v1::Pdus::GetRequest(_) => "GetRequest",
            v1::Pdus::GetNextRequest(_) => "GetNextRequest",
            v1::Pdus::GetResponse(_) => "GetResponse",
            v1::Pdus::SetRequest(_) => "SetRequest",
            v1::Pdus::Trap(_) => "Trap",
        }.to_string()
    }

    /// Format SNMPv2c message with OIDs
    fn format_v2c_message(msg: &v2c::Message<v2::Pdus>) -> String {
        let mut info = format!("SNMPv2c Message:\n");
        info.push_str(&format!("  Community: {}\n", String::from_utf8_lossy(&msg.community)));

        match &msg.data {
            v2::Pdus::GetRequest(pdu) => {
                info.push_str(&format!("  Type: GetRequest\n"));
                info.push_str(&format!("  Request ID: {}\n", pdu.0.request_id));
                info.push_str(&Self::format_v2_var_binds(&pdu.0.variable_bindings));
            },
            v2::Pdus::GetNextRequest(pdu) => {
                info.push_str(&format!("  Type: GetNextRequest\n"));
                info.push_str(&format!("  Request ID: {}\n", pdu.0.request_id));
                info.push_str(&Self::format_v2_var_binds(&pdu.0.variable_bindings));
            },
            v2::Pdus::GetBulkRequest(pdu) => {
                info.push_str(&format!("  Type: GetBulkRequest\n"));
                info.push_str(&format!("  Request ID: {}\n", pdu.0.request_id));
                info.push_str(&format!("  Non-repeaters: {}\n", pdu.0.non_repeaters));
                info.push_str(&format!("  Max-repetitions: {}\n", pdu.0.max_repetitions));
                info.push_str(&Self::format_v2_var_binds(&pdu.0.variable_bindings));
            },
            _ => {
                info.push_str(&format!("  Type: {}\n", Self::get_v2_pdu_type(&msg.data)));
            }
        }

        info
    }

    /// Format SNMPv1 message with OIDs
    fn format_v1_message(msg: &v1::Message<v1::Pdus>) -> String {
        let mut info = format!("SNMPv1 Message:\n");
        info.push_str(&format!("  Community: {}\n", String::from_utf8_lossy(&msg.community)));

        match &msg.data {
            v1::Pdus::GetRequest(pdu) => {
                info.push_str(&format!("  Type: GetRequest\n"));
                info.push_str(&format!("  Request ID: {}\n", pdu.0.request_id));
                info.push_str(&Self::format_v1_var_binds(&pdu.0.variable_bindings));
            },
            v1::Pdus::GetNextRequest(pdu) => {
                info.push_str(&format!("  Type: GetNextRequest\n"));
                info.push_str(&format!("  Request ID: {}\n", pdu.0.request_id));
                info.push_str(&Self::format_v1_var_binds(&pdu.0.variable_bindings));
            },
            _ => {
                info.push_str(&format!("  Type: {}\n", Self::get_v1_pdu_type(&msg.data)));
            }
        }

        info
    }

    /// Format v2 variable bindings
    fn format_v2_var_binds(bindings: &[v2::VarBind]) -> String {
        let mut result = String::from("  Requested OIDs:\n");
        if bindings.is_empty() {
            result.push_str("    (none - requesting all)\n");
        } else {
            for (i, bind) in bindings.iter().enumerate() {
                result.push_str(&format!("    [{}] {}\n", i + 1, bind.name));
            }
        }
        result
    }

    /// Format v1 variable bindings
    fn format_v1_var_binds(bindings: &[v1::VarBind]) -> String {
        let mut result = String::from("  Requested OIDs:\n");
        if bindings.is_empty() {
            result.push_str("    (none - requesting all)\n");
        } else {
            for (i, bind) in bindings.iter().enumerate() {
                result.push_str(&format!("    [{}] {}\n", i + 1, bind.name));
            }
        }
        result
    }

    /// Build SNMP response from LLM output using manual BER encoding
    pub fn build_snmp_response(
        llm_response: &str,
        version: u8,
        request_id: i32,
        community: &[u8],
    ) -> Result<Vec<u8>> {
        // Parse JSON response from LLM
        let response_data: serde_json::Value = serde_json::from_str(llm_response)
            .map_err(|e| anyhow::anyhow!("Failed to parse LLM JSON response: {}. Response was: {}", e, llm_response))?;

        // Check for error
        if response_data["error"].as_bool().unwrap_or(false) {
            let error_msg = response_data["error_message"].as_str().unwrap_or("Unknown error");
            debug!("LLM reported error: {}", error_msg);
            return Self::build_error_response(version, request_id, community);
        }

        // Build response with variable bindings
        let mut var_binds = Vec::new();

        if let Some(variables) = response_data["variables"].as_array() {
            for var in variables {
                let oid_str = var["oid"].as_str().unwrap_or("");
                let value_type = var["type"].as_str().unwrap_or("null");
                let value = &var["value"];

                // Encode each variable binding
                let var_bind = Self::encode_var_bind(oid_str, value_type, value)?;
                var_binds.push(var_bind);
            }
        }

        // Build the complete SNMP response message
        Self::build_response_message(version, request_id, community, 0, 0, var_binds)
    }

    /// Encode a single variable binding
    fn encode_var_bind(oid_str: &str, value_type: &str, value: &serde_json::Value) -> Result<Vec<u8>> {
        let mut result = Vec::new();

        // Encode OID
        let oid_bytes = Self::encode_oid(oid_str)?;

        // Encode value based on type
        let value_bytes = match value_type {
            "string" => {
                let s = value.as_str().unwrap_or("");
                Self::encode_octet_string(s.as_bytes())
            },
            "integer" => {
                let n = value.as_i64().unwrap_or(0) as i32;
                Self::encode_integer(n)
            },
            "counter" => {
                let n = value.as_u64().unwrap_or(0) as u32;
                Self::encode_counter(n)
            },
            "gauge" => {
                let n = value.as_u64().unwrap_or(0) as u32;
                Self::encode_gauge(n)
            },
            "timeticks" => {
                let n = value.as_u64().unwrap_or(0) as u32;
                Self::encode_timeticks(n)
            },
            "null" | _ => {
                vec![0x05, 0x00] // NULL
            }
        };

        // Construct SEQUENCE for variable binding
        result.push(0x30); // SEQUENCE tag
        let len = oid_bytes.len() + value_bytes.len();
        if len < 128 {
            result.push(len as u8);
        } else {
            // Long form length encoding
            result.push(0x81);
            result.push(len as u8);
        }
        result.extend_from_slice(&oid_bytes);
        result.extend_from_slice(&value_bytes);

        Ok(result)
    }

    /// Encode OID
    fn encode_oid(oid_str: &str) -> Result<Vec<u8>> {
        let parts: Vec<u32> = oid_str
            .split('.')
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();

        if parts.len() < 2 {
            return Err(anyhow::anyhow!("Invalid OID"));
        }

        let mut encoded = Vec::new();

        // First two components are encoded specially
        encoded.push((parts[0] * 40 + parts[1]) as u8);

        // Encode remaining components
        for &part in &parts[2..] {
            if part < 128 {
                encoded.push(part as u8);
            } else {
                // Multi-byte encoding for values >= 128
                let mut bytes = Vec::new();
                let mut val = part;

                while val > 0 {
                    bytes.push((val & 0x7F) as u8);
                    val >>= 7;
                }

                bytes.reverse();
                for (i, &byte) in bytes.iter().enumerate() {
                    if i < bytes.len() - 1 {
                        encoded.push(byte | 0x80);
                    } else {
                        encoded.push(byte);
                    }
                }
            }
        }

        // Wrap with OID tag
        let mut result = vec![0x06]; // OBJECT IDENTIFIER tag
        if encoded.len() < 128 {
            result.push(encoded.len() as u8);
        } else {
            result.push(0x81);
            result.push(encoded.len() as u8);
        }
        result.extend_from_slice(&encoded);

        Ok(result)
    }

    /// Encode integer
    fn encode_integer(value: i32) -> Vec<u8> {
        let bytes = value.to_be_bytes();
        let mut result = vec![0x02]; // INTEGER tag

        // Skip leading zeros/ones for minimal encoding
        let mut start = 0;
        if value >= 0 {
            while start < 3 && bytes[start] == 0 && (bytes[start + 1] & 0x80) == 0 {
                start += 1;
            }
        } else {
            while start < 3 && bytes[start] == 0xFF && (bytes[start + 1] & 0x80) != 0 {
                start += 1;
            }
        }

        let len = 4 - start;
        result.push(len as u8);
        result.extend_from_slice(&bytes[start..]);

        result
    }

    /// Encode octet string
    fn encode_octet_string(value: &[u8]) -> Vec<u8> {
        let mut result = vec![0x04]; // OCTET STRING tag
        if value.len() < 128 {
            result.push(value.len() as u8);
        } else {
            result.push(0x81);
            result.push(value.len() as u8);
        }
        result.extend_from_slice(value);
        result
    }

    /// Encode counter (application tag 1)
    fn encode_counter(value: u32) -> Vec<u8> {
        let bytes = value.to_be_bytes();
        let mut result = vec![0x41]; // Counter tag (application class, tag 1)

        // Skip leading zeros
        let mut start = 0;
        while start < 3 && bytes[start] == 0 {
            start += 1;
        }

        let len = 4 - start;
        result.push(len as u8);
        result.extend_from_slice(&bytes[start..]);

        result
    }

    /// Encode gauge (application tag 2)
    fn encode_gauge(value: u32) -> Vec<u8> {
        let bytes = value.to_be_bytes();
        let mut result = vec![0x42]; // Gauge tag (application class, tag 2)

        // Skip leading zeros
        let mut start = 0;
        while start < 3 && bytes[start] == 0 {
            start += 1;
        }

        let len = 4 - start;
        result.push(len as u8);
        result.extend_from_slice(&bytes[start..]);

        result
    }

    /// Encode timeticks (application tag 3)
    fn encode_timeticks(value: u32) -> Vec<u8> {
        let bytes = value.to_be_bytes();
        let mut result = vec![0x43]; // TimeTicks tag (application class, tag 3)

        // Skip leading zeros
        let mut start = 0;
        while start < 3 && bytes[start] == 0 {
            start += 1;
        }

        let len = 4 - start;
        result.push(len as u8);
        result.extend_from_slice(&bytes[start..]);

        result
    }

    /// Build complete SNMP response message
    fn build_response_message(
        version: u8,
        request_id: i32,
        community: &[u8],
        error_status: u8,
        error_index: u8,
        var_binds: Vec<Vec<u8>>,
    ) -> Result<Vec<u8>> {
        let mut message = Vec::new();

        // Encode version
        let version_bytes = Self::encode_integer(version as i32);

        // Encode community
        let community_bytes = Self::encode_octet_string(community);

        // Build GetResponse PDU (tag 0xA2)
        let mut pdu = vec![0xA2]; // GetResponse tag (context-specific, constructed, tag 2)

        // Encode request ID
        let request_id_bytes = Self::encode_integer(request_id);

        // Encode error status
        let error_status_bytes = Self::encode_integer(error_status as i32);

        // Encode error index
        let error_index_bytes = Self::encode_integer(error_index as i32);

        // Encode variable bindings list
        let mut var_binds_list = vec![0x30]; // SEQUENCE tag
        let var_binds_total_len: usize = var_binds.iter().map(|v| v.len()).sum();

        if var_binds_total_len < 128 {
            var_binds_list.push(var_binds_total_len as u8);
        } else {
            var_binds_list.push(0x81);
            var_binds_list.push(var_binds_total_len as u8);
        }

        for var_bind in var_binds {
            var_binds_list.extend_from_slice(&var_bind);
        }

        // Calculate PDU length
        let pdu_len = request_id_bytes.len() +
                     error_status_bytes.len() +
                     error_index_bytes.len() +
                     var_binds_list.len();

        if pdu_len < 128 {
            pdu.push(pdu_len as u8);
        } else {
            pdu.push(0x81);
            pdu.push(pdu_len as u8);
        }

        pdu.extend_from_slice(&request_id_bytes);
        pdu.extend_from_slice(&error_status_bytes);
        pdu.extend_from_slice(&error_index_bytes);
        pdu.extend_from_slice(&var_binds_list);

        // Build complete message
        message.push(0x30); // SEQUENCE tag
        let message_len = version_bytes.len() + community_bytes.len() + pdu.len();

        if message_len < 128 {
            message.push(message_len as u8);
        } else {
            message.push(0x81);
            message.push(message_len as u8);
        }

        message.extend_from_slice(&version_bytes);
        message.extend_from_slice(&community_bytes);
        message.extend_from_slice(&pdu);

        Ok(message)
    }

    /// Build a generic error response
    fn build_error_response(version: u8, request_id: i32, community: &[u8]) -> Result<Vec<u8>> {
        // Build response with genErr (5) and no variable bindings
        Self::build_response_message(version, request_id, community, 5, 0, vec![])
    }
}