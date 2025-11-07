//! SNMP agent implementation using rasn-snmp library
pub mod actions;

use crate::server::connection::ConnectionId;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

// SNMP protocol support
use rasn_snmp::{v1, v2c, v2};
use rasn::{ber, types::Integer};

use crate::llm::action_helper::call_llm;
use crate::llm::ollama_client::OllamaClient;
use actions::SNMP_REQUEST_EVENT;
use crate::server::SnmpProtocol;
use crate::protocol::Event;
use crate::state::app_state::AppState;

/// Get LLM context and output format instructions for SNMP stack
pub fn get_llm_protocol_prompt() -> (&'static str, &'static str) {
    let context = r#"You are handling SNMP requests. Return appropriate SNMP responses with OID values.
Common OIDs:
- 1.3.6.1.2.1.1.1.0: System description
- 1.3.6.1.2.1.1.5.0: System name
- 1.3.6.1.2.1.1.3.0: System uptime"#;

    let output_format = r#"IMPORTANT: Respond with a JSON object containing SNMP variable bindings:
{
  "variables": [
    {"oid": "1.3.6.1.2.1.1.1.0", "type": "string", "value": "System Description"},
    {"oid": "1.3.6.1.2.1.1.5.0", "type": "string", "value": "hostname"}
  ],
  "error": false,
  "error_message": null
}

Supported value types: "string", "integer", "counter", "gauge", "timeticks", "null"

For errors, respond with:
{
  "error": true,
  "error_message": "Error description"
}
"#;

    (context, output_format)
}

/// Parsed SNMP message information
#[derive(Debug)]
pub struct ParsedSnmpInfo {
    pub description: String,
    pub request_type: String,
    pub version: u8,
    pub request_id: i32,
    pub community: Vec<u8>,
    pub requested_oids: Vec<String>,
}

/// SNMP server that forwards requests to LLM
pub struct SnmpServer;

impl SnmpServer {
    /// Spawn SNMP agent with integrated LLM handling
    pub async fn spawn_with_llm(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        info!("SNMP agent listening on {}", local_addr);

        let protocol = Arc::new(SnmpProtocol::new());

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 65535];

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let _connection_id = ConnectionId::new();

                        // DEBUG: Log summary
                        debug!("SNMP received {} bytes from {}", n, peer_addr);
                        let _ = status_tx.send(format!("[DEBUG] SNMP received {} bytes from {}", n, peer_addr));

                        // TRACE: Log full payload
                        let hex_str = hex::encode(&data);
                        trace!("SNMP data (hex): {}", hex_str);
                        let _ = status_tx.send(format!("[TRACE] SNMP data (hex): {}", hex_str));

                        // Parse the SNMP message
                        let parsed = match Self::parse_snmp_message(&data) {
                            Ok(p) => p,
                            Err(e) => {
                                error!("Failed to parse SNMP message: {}", e);
                                continue;
                            }
                        };

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();
                        let protocol_clone = protocol.clone();
                        let version = parsed.version;
                        let request_id = parsed.request_id;
                        let community = parsed.community.clone();
                        let requested_oids = parsed.requested_oids.clone();

                        // Spawn task to handle request with LLM
                        tokio::spawn(async move {
                            // Create Event with SNMP request data
                            let event = Event::new(&SNMP_REQUEST_EVENT, serde_json::json!({
                                "request_type": parsed.request_type,
                                "oids": parsed.requested_oids,
                                "community": String::from_utf8_lossy(&parsed.community).to_string()
                            }));

                            debug!("SNMP calling LLM for request from {}", peer_addr);
                            let _ = status_clone.send(format!("[DEBUG] SNMP calling LLM for request from {}", peer_addr));

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

                                    debug!("SNMP got {} protocol results", execution_result.protocol_results.len());
                                    let _ = status_clone.send(format!("[DEBUG] SNMP got {} protocol results", execution_result.protocol_results.len()));

                                    // For legacy function, extract first raw action as JSON response
                                    if let Some(first_action) = execution_result.raw_actions.first() {
                                        let llm_output = serde_json::to_string(first_action).unwrap_or_default();
                                        debug!("SNMP LLM response: {}", llm_output);

                                        // Build SNMP response from LLM output
                                        match Self::build_snmp_response(&llm_output, version, request_id, &community, &requested_oids) {
                                            Ok(snmp_response) => {
                                                if let Err(e) = socket_clone.send_to(&snmp_response, peer_addr).await {
                                                    error!("Failed to send SNMP response: {}", e);
                                                } else {
                                                    // DEBUG: Log summary
                                                    debug!("SNMP sent {} bytes to {}", snmp_response.len(), peer_addr);
                                                    let _ = status_clone.send(format!("[DEBUG] SNMP sent {} bytes to {}", snmp_response.len(), peer_addr));

                                                    // TRACE: Log full payload
                                                    let hex_dump: String = snmp_response.iter()
                                                        .map(|b| format!("{:02X}", b))
                                                        .collect::<Vec<_>>()
                                                        .join(" ");
                                                    trace!("SNMP sent (hex): {}", hex_dump);
                                                    let _ = status_clone.send(format!("[TRACE] SNMP sent (hex): {}", hex_dump));

                                                    let _ = status_clone.send(format!(
                                                        "→ SNMP response to {} ({} bytes)",
                                                        peer_addr, snmp_response.len()
                                                    ));
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to build SNMP response: {}", e);
                                                // Send error response
                                                if let Ok(error_response) = Self::build_error_response(version, request_id, &community) {
                                                    let _ = socket_clone.send_to(&error_response, peer_addr).await;
                                                }
                                            }
                                        }
                                    } else {
                                        debug!("SNMP no raw actions from LLM");
                                        let _ = status_clone.send("[DEBUG] SNMP no raw actions from LLM".to_string());
                                    }
                                }
                                Err(e) => {
                                    error!("SNMP LLM call failed: {}", e);
                                    let _ = status_clone.send(format!("✗ SNMP LLM error: {}", e));
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("SNMP receive error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Spawn SNMP agent with integrated LLM actions
    pub async fn spawn_with_llm_actions(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: crate::state::ServerId,
    ) -> Result<SocketAddr> {
        let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
        let local_addr = socket.local_addr()?;
        info!("SNMP agent (action-based) listening on {}", local_addr);

        let protocol = Arc::new(SnmpProtocol::new());

        tokio::spawn(async move {
            let mut buffer = vec![0u8; 65535];

            loop {
                match socket.recv_from(&mut buffer).await {
                    Ok((n, peer_addr)) => {
                        let data = buffer[..n].to_vec();
                        let connection_id = ConnectionId::new();

                        // Add connection to ServerInstance (SNMP "connection" = recent peer)
                        use crate::state::server::{ConnectionState as ServerConnectionState, ProtocolConnectionInfo, ConnectionStatus};
                        let now = std::time::Instant::now();
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
                            protocol_info: ProtocolConnectionInfo::empty(),
                        };
                        app_state.add_connection_to_server(server_id, conn_state).await;
                        let _ = status_tx.send("__UPDATE_UI__".to_string());

                        // DEBUG: Log summary
                        debug!("SNMP received {} bytes from {}", n, peer_addr);
                        let _ = status_tx.send(format!("[DEBUG] SNMP received {} bytes from {}", n, peer_addr));

                        // TRACE: Log full payload
                        let hex_str = hex::encode(&data);
                        trace!("SNMP data (hex): {}", hex_str);
                        let _ = status_tx.send(format!("[TRACE] SNMP data (hex): {}", hex_str));

                        // Parse the SNMP message
                        let parsed = match Self::parse_snmp_message(&data) {
                            Ok(p) => p,
                            Err(e) => {
                                error!("Failed to parse SNMP message: {}", e);
                                continue;
                            }
                        };

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();
                        let socket_clone = socket.clone();
                        let protocol_clone = protocol.clone();
                        let version = parsed.version;
                        let request_id = parsed.request_id;
                        let community = parsed.community.clone();
                        let requested_oids = parsed.requested_oids.clone();

                        // Spawn task to handle request with LLM
                        tokio::spawn(async move {
                            // Clone for BER encoding later
                            let requested_oids_clone = requested_oids.clone();
                            let community_clone = community.clone();

                            // Create SNMP request event
                            let event = Event::new(&SNMP_REQUEST_EVENT, serde_json::json!({
                                "request_type": parsed.request_type,
                                "oids": parsed.requested_oids,
                                "community": String::from_utf8_lossy(&parsed.community).to_string()
                            }));

                            debug!("SNMP calling LLM for request from {}", peer_addr);
                            let _ = status_clone.send(format!("[DEBUG] SNMP calling LLM for request from {}", peer_addr));

                            // Call LLM
                            match call_llm(
                                &llm_clone,
                                &state_clone,
                                server_id,
                                None,
                                &event,
                                protocol_clone.as_ref(),
                            ).await {
                                Ok(execution_result) => {
                                    // Display messages from LLM
                                    for message in &execution_result.messages {
                                        info!("{}", message);
                                        let _ = status_clone.send(format!("[INFO] {}", message));
                                    }

                                    debug!("SNMP got {} protocol results", execution_result.protocol_results.len());
                                    let _ = status_clone.send(format!("[DEBUG] SNMP got {} protocol results", execution_result.protocol_results.len()));

                                    // Handle protocol results (send SNMP response)
                                    for protocol_result in execution_result.protocol_results {
                                                        if let Some(output_data) = protocol_result.get_all_output().first() {
                                                            // Parse JSON response and convert to SNMP BER format
                                                            let json_str = String::from_utf8_lossy(output_data);
                                                            match Self::build_snmp_response(&json_str, version, request_id, &community_clone, &requested_oids_clone) {
                                                                Ok(snmp_response) => {
                                                                    if let Err(e) = socket_clone.send_to(&snmp_response, peer_addr).await {
                                                                        error!("Failed to send SNMP response: {}", e);
                                                                    } else {
                                                                        // DEBUG: Log summary
                                                                        debug!("SNMP sent {} bytes to {}", snmp_response.len(), peer_addr);
                                                                        let _ = status_clone.send(format!("[DEBUG] SNMP sent {} bytes to {}", snmp_response.len(), peer_addr));

                                                                        // TRACE: Log full payload
                                                                        let hex_dump: String = snmp_response.iter()
                                                                            .map(|b| format!("{:02X}", b))
                                                                            .collect::<Vec<_>>()
                                                                            .join(" ");
                                                                        trace!("SNMP sent (hex): {}", hex_dump);
                                                                        let _ = status_clone.send(format!("[TRACE] SNMP sent (hex): {}", hex_dump));

                                                                        let _ = status_clone.send(format!(
                                                                            "→ SNMP response to {} ({} bytes)",
                                                                            peer_addr, snmp_response.len()
                                                                        ));
                                                                    }
                                                                }
                                                                Err(e) => {
                                                                    error!("Failed to build SNMP BER response: {}", e);
                                                                    let _ = status_clone.send(format!("✗ SNMP encoding error: {}", e));
                                                                }
                                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("SNMP LLM call failed: {}", e);
                                    let _ = status_clone.send(format!("✗ SNMP LLM error: {}", e));
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("SNMP receive error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(local_addr)
    }

    /// Parse SNMP message and extract relevant information
    pub fn parse_snmp_message(data: &[u8]) -> Result<ParsedSnmpInfo> {
        // Try to decode as SNMPv2c first (most common)
        if let Ok(msg) = ber::decode::<v2c::Message<v2::Pdus>>(data) {
            let request_type = Self::get_v2_pdu_type(&msg.data);
            let request_id = Self::get_v2_request_id(&msg.data);
            let requested_oids = Self::get_v2_requested_oids(&msg.data);
            return Ok(ParsedSnmpInfo {
                description: Self::format_v2c_message(&msg),
                request_type,
                version: 1, // v2c uses version 1 in the packet
                request_id,
                community: msg.community.to_vec(),
                requested_oids,
            });
        }

        // Try SNMPv1
        if let Ok(msg) = ber::decode::<v1::Message<v1::Pdus>>(data) {
            let request_type = Self::get_v1_pdu_type(&msg.data);
            let request_id = Self::get_v1_request_id(&msg.data);
            let requested_oids = Self::get_v1_requested_oids(&msg.data);
            return Ok(ParsedSnmpInfo {
                description: Self::format_v1_message(&msg),
                request_type,
                version: 0,
                request_id,
                community: msg.community.to_vec(),
                requested_oids,
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

    /// Get requested OIDs for v2
    fn get_v2_requested_oids(pdu: &v2::Pdus) -> Vec<String> {
        let bindings = match pdu {
            v2::Pdus::GetRequest(p) => &p.0.variable_bindings,
            v2::Pdus::GetNextRequest(p) => &p.0.variable_bindings,
            v2::Pdus::GetBulkRequest(p) => &p.0.variable_bindings,
            v2::Pdus::SetRequest(p) => &p.0.variable_bindings,
            _ => return vec![],
        };

        bindings.iter().map(|vb| vb.name.to_string()).collect()
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

    /// Get requested OIDs for v1
    fn get_v1_requested_oids(pdu: &v1::Pdus) -> Vec<String> {
        let bindings = match pdu {
            v1::Pdus::GetRequest(p) => &p.0.variable_bindings,
            v1::Pdus::GetNextRequest(p) => &p.0.variable_bindings,
            v1::Pdus::SetRequest(p) => &p.0.variable_bindings,
            _ => return vec![],
        };

        bindings.iter().map(|vb| vb.name.to_string()).collect()
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
        let mut info = "SNMPv2c Message:\n".to_string();
        info.push_str(&format!("  Community: {}\n", String::from_utf8_lossy(&msg.community)));

        match &msg.data {
            v2::Pdus::GetRequest(pdu) => {
                info.push_str("  Type: GetRequest\n");
                info.push_str(&format!("  Request ID: {}\n", pdu.0.request_id));
                info.push_str(&Self::format_v2_var_binds(&pdu.0.variable_bindings));
            },
            v2::Pdus::GetNextRequest(pdu) => {
                info.push_str("  Type: GetNextRequest\n");
                info.push_str(&format!("  Request ID: {}\n", pdu.0.request_id));
                info.push_str(&Self::format_v2_var_binds(&pdu.0.variable_bindings));
            },
            v2::Pdus::GetBulkRequest(pdu) => {
                info.push_str("  Type: GetBulkRequest\n");
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
        let mut info = "SNMPv1 Message:\n".to_string();
        info.push_str(&format!("  Community: {}\n", String::from_utf8_lossy(&msg.community)));

        match &msg.data {
            v1::Pdus::GetRequest(pdu) => {
                info.push_str("  Type: GetRequest\n");
                info.push_str(&format!("  Request ID: {}\n", pdu.0.request_id));
                info.push_str(&Self::format_v1_var_binds(&pdu.0.variable_bindings));
            },
            v1::Pdus::GetNextRequest(pdu) => {
                info.push_str("  Type: GetNextRequest\n");
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
        requested_oids: &[String],
    ) -> Result<Vec<u8>> {
        let trimmed = llm_response.trim();

        // Try to parse as JSON first
        if let Ok(response_data) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if !response_data.is_object() {
                // Not an object, fall through to plain text handling
            } else if response_data.get("variables").is_some() || response_data.get("error").is_some() {
                // SNMP-specific format with variables
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

                        // DEBUG: Log the actual value being returned
                        let value_str = match value_type {
                            "string" => format!("\"{}\"", value.as_str().unwrap_or("")),
                            "integer" => format!("{}", value.as_i64().unwrap_or(0)),
                            "counter" | "gauge" | "timeticks" => format!("{}", value.as_u64().unwrap_or(0)),
                            "null" | _ => "null".to_string(),
                        };
                        debug!("SNMP response: {} = {} ({})", oid_str, value_str, value_type);

                        // Encode each variable binding
                        let var_bind = Self::encode_var_bind(oid_str, value_type, value)?;
                        var_binds.push(var_bind);
                    }
                }

                // Build the complete SNMP response message
                return Self::build_response_message(version, request_id, community, 0, 0, var_binds);
            } else if let Some(output) = response_data.get("output") {
                // Standard LlmResponse format - extract output field and process as text
                if let Some(output_str) = output.as_str() {
                    debug!("LLM returned LlmResponse format, using 'output' field: {}", output_str);

                    // Use the first requested OID
                    let oid = requested_oids.first()
                        .map(|s| s.as_str())
                        .unwrap_or("1.3.6.1.2.1.1.1.0");

                    // Try to parse as integer first
                    let var_bind = if let Ok(num) = output_str.trim().parse::<i32>() {
                        debug!("SNMP response: {} = {} (integer)", oid, num);
                        Self::encode_var_bind(oid, "integer", &serde_json::Value::from(num))?
                    } else {
                        // Treat as string
                        debug!("SNMP response: {} = \"{}\" (string)", oid, output_str);
                        Self::encode_var_bind(oid, "string", &serde_json::Value::from(output_str))?
                    };

                    return Self::build_response_message(version, request_id, community, 0, 0, vec![var_bind]);
                }
                // If output is null or not a string, fall through to plain text handling
            }
            // If JSON parsed but not recognized format, fall through to plain text handling
        }

        // Fallback: treat as plain text value
        // Use the first requested OID if available, otherwise use a default
        let oid = requested_oids.first()
            .map(|s| s.as_str())
            .unwrap_or("1.3.6.1.2.1.1.1.0");

        debug!("LLM returned plain text (not JSON), treating as simple value for OID {}: {}", oid, trimmed);

        // Try to parse as integer first
        let var_bind = if let Ok(num) = trimmed.parse::<i32>() {
            debug!("SNMP response: {} = {} (integer)", oid, num);
            Self::encode_var_bind(oid, "integer", &serde_json::Value::from(num))?
        } else {
            // Treat as string
            debug!("SNMP response: {} = \"{}\" (string)", oid, trimmed);
            Self::encode_var_bind(oid, "string", &serde_json::Value::from(trimmed))?
        };

        Self::build_response_message(version, request_id, community, 0, 0, vec![var_bind])
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
