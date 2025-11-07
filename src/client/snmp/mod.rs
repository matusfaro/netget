//! SNMP client implementation
pub mod actions;

pub use actions::SnmpClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use tokio::time::timeout;
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::Client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::snmp::actions::{SNMP_CLIENT_CONNECTED_EVENT, SNMP_CLIENT_RESPONSE_RECEIVED_EVENT};

// SNMP protocol support
use rasn_snmp::{v1, v2, v2c};
use rasn::{ber, types::Integer};
use serde_json::Value;
use rand;

/// SNMP client configuration
#[derive(Debug, Clone)]
struct SnmpConfig {
    community: String,
    version: SnmpVersion,
    timeout_ms: u64,
    retries: u32,
}

impl Default for SnmpConfig {
    fn default() -> Self {
        Self {
            community: "public".to_string(),
            version: SnmpVersion::V2c,
            timeout_ms: 5000,
            retries: 3,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum SnmpVersion {
    V1,
    V2c,
}

/// Parse startup parameters
fn parse_startup_params(params: Option<Value>) -> SnmpConfig {
    let mut config = SnmpConfig::default();

    if let Some(params) = params {
        if let Some(community) = params.get("community").and_then(|v| v.as_str()) {
            config.community = community.to_string();
        }
        if let Some(version) = params.get("version").and_then(|v| v.as_str()) {
            config.version = match version.to_lowercase().as_str() {
                "v1" | "1" => SnmpVersion::V1,
                "v2c" | "v2" | "2c" | "2" => SnmpVersion::V2c,
                _ => SnmpVersion::V2c,
            };
        }
        if let Some(timeout) = params.get("timeout_ms").and_then(|v| v.as_u64()) {
            config.timeout_ms = timeout;
        }
        if let Some(retries) = params.get("retries").and_then(|v| v.as_u64()) {
            config.retries = retries as u32;
        }
    }

    config
}

/// SNMP client that connects to an SNMP agent
pub struct SnmpClient;

impl SnmpClient {
    /// Connect to an SNMP agent with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<Value>,
    ) -> Result<SocketAddr> {
        // Parse configuration
        let config = parse_startup_params(startup_params);
        debug!("SNMP client config: community={}, version={:?}, timeout={}ms, retries={}",
            config.community, config.version, config.timeout_ms, config.retries);

        // Bind UDP socket to any local port
        let socket = UdpSocket::bind("0.0.0.0:0").await
            .context("Failed to bind UDP socket")?;

        let local_addr = socket.local_addr()?;

        // Connect to remote agent (sets default destination for send/recv)
        socket.connect(&remote_addr).await
            .context(format!("Failed to connect to SNMP agent at {}", remote_addr))?;

        let remote_sock_addr: SocketAddr = remote_addr.parse()
            .context("Invalid remote address")?;

        info!("SNMP client {} connected to {} (local: {})", client_id, remote_sock_addr, local_addr);

        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] SNMP client {} connected", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(SnmpClientProtocol::new());
            let event = Event::new(
                &SNMP_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "remote_addr": remote_sock_addr.to_string(),
                }),
            );

            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

            let llm_clone = llm_client.clone();
            let state_clone = app_state.clone();
            let status_clone = status_tx.clone();
            let protocol_clone = protocol.clone();
            let socket = Arc::new(socket);
            let socket_clone = socket.clone();
            let config = Arc::new(config);

            // Spawn initial LLM call
            tokio::spawn(async move {
                match call_llm_for_client(
                    &llm_clone,
                    &state_clone,
                    client_id.to_string(),
                    &instruction,
                    &memory,
                    Some(&event),
                    protocol_clone.as_ref(),
                    &status_clone,
                ).await {
                    Ok(ClientLlmResult { actions, memory_updates }) => {
                        // Update memory
                        if let Some(mem) = memory_updates {
                            state_clone.set_memory_for_client(client_id, mem).await;
                        }

                        // Execute initial actions
                        Self::execute_actions(
                            actions,
                            &protocol_clone,
                            &socket_clone,
                            client_id,
                            &config,
                            &llm_clone,
                            &state_clone,
                            &status_clone,
                        ).await;
                    }
                    Err(e) => {
                        error!("Initial LLM call failed for SNMP client {}: {}", client_id, e);
                    }
                }
            });
        }

        Ok(local_addr)
    }

    /// Execute SNMP actions from LLM
    async fn execute_actions(
        actions: Vec<Value>,
        protocol: &Arc<SnmpClientProtocol>,
        socket: &Arc<UdpSocket>,
        client_id: ClientId,
        config: &Arc<SnmpConfig>,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
    ) {
        use crate::llm::actions::client_trait::ClientActionResult;

        for action in actions {
            match protocol.execute_action(action) {
                Ok(ClientActionResult::Custom { name, data }) => {
                    match name.as_str() {
                        "snmp_get" => {
                            if let Some(oids) = data.get("oids").and_then(|v| v.as_array()) {
                                let oid_strings: Vec<String> = oids.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect();

                                if let Err(e) = Self::send_get_request(
                                    socket, &oid_strings, config, client_id,
                                    llm_client, app_state, status_tx, protocol
                                ).await {
                                    error!("Failed to send SNMP GET: {}", e);
                                }
                            }
                        }
                        "snmp_getnext" => {
                            if let Some(oids) = data.get("oids").and_then(|v| v.as_array()) {
                                let oid_strings: Vec<String> = oids.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect();

                                if let Err(e) = Self::send_getnext_request(
                                    socket, &oid_strings, config, client_id,
                                    llm_client, app_state, status_tx, protocol
                                ).await {
                                    error!("Failed to send SNMP GETNEXT: {}", e);
                                }
                            }
                        }
                        "snmp_getbulk" => {
                            if let Some(oids) = data.get("oids").and_then(|v| v.as_array()) {
                                let oid_strings: Vec<String> = oids.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect();
                                let non_repeaters = data.get("non_repeaters")
                                    .and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                                let max_repetitions = data.get("max_repetitions")
                                    .and_then(|v| v.as_i64()).unwrap_or(10) as i32;

                                if let Err(e) = Self::send_getbulk_request(
                                    socket, &oid_strings, non_repeaters, max_repetitions,
                                    config, client_id, llm_client, app_state, status_tx, protocol
                                ).await {
                                    error!("Failed to send SNMP GETBULK: {}", e);
                                }
                            }
                        }
                        "snmp_set" => {
                            if let Some(variables) = data.get("variables").and_then(|v| v.as_array()) {
                                if let Err(e) = Self::send_set_request(
                                    socket, variables, config, client_id,
                                    llm_client, app_state, status_tx, protocol
                                ).await {
                                    error!("Failed to send SNMP SET: {}", e);
                                }
                            }
                        }
                        _ => {
                            debug!("Unknown SNMP action: {}", name);
                        }
                    }
                }
                Ok(ClientActionResult::Disconnect) => {
                    info!("SNMP client {} disconnecting", client_id);
                    app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                    let _ = status_tx.send("__UPDATE_UI__".to_string());
                }
                Ok(ClientActionResult::WaitForMore) => {
                    // No action needed
                }
                Ok(_) => {}
                Err(e) => {
                    error!("Failed to execute SNMP action: {}", e);
                }
            }
        }
    }

    /// Send SNMP GET request
    async fn send_get_request(
        socket: &Arc<UdpSocket>,
        oids: &[String],
        config: &Arc<SnmpConfig>,
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: &Arc<SnmpClientProtocol>,
    ) -> Result<()> {
        let request_id = rand::random::<i32>();

        // Build GET request based on version
        let request_bytes = match config.version {
            SnmpVersion::V1 => Self::build_v1_get_request(oids, &config.community, request_id)?,
            SnmpVersion::V2c => Self::build_v2c_get_request(oids, &config.community, request_id)?,
        };

        debug!("SNMP client {} sending GET for {} OIDs", client_id, oids.len());
        trace!("SNMP GET request (hex): {}", hex::encode(&request_bytes));

        // Send request and wait for response
        Self::send_request_and_handle_response(
            socket, &request_bytes, "GetRequest", config, client_id,
            llm_client, app_state, status_tx, protocol
        ).await
    }

    /// Send SNMP GETNEXT request
    async fn send_getnext_request(
        socket: &Arc<UdpSocket>,
        oids: &[String],
        config: &Arc<SnmpConfig>,
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: &Arc<SnmpClientProtocol>,
    ) -> Result<()> {
        let request_id = rand::random::<i32>();

        let request_bytes = match config.version {
            SnmpVersion::V1 => Self::build_v1_getnext_request(oids, &config.community, request_id)?,
            SnmpVersion::V2c => Self::build_v2c_getnext_request(oids, &config.community, request_id)?,
        };

        debug!("SNMP client {} sending GETNEXT for {} OIDs", client_id, oids.len());

        Self::send_request_and_handle_response(
            socket, &request_bytes, "GetNextRequest", config, client_id,
            llm_client, app_state, status_tx, protocol
        ).await
    }

    /// Send SNMP GETBULK request (v2c only)
    async fn send_getbulk_request(
        socket: &Arc<UdpSocket>,
        oids: &[String],
        non_repeaters: i32,
        max_repetitions: i32,
        config: &Arc<SnmpConfig>,
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: &Arc<SnmpClientProtocol>,
    ) -> Result<()> {
        if matches!(config.version, SnmpVersion::V1) {
            return Err(anyhow::anyhow!("GETBULK is only supported in SNMPv2c"));
        }

        let request_id = rand::random::<i32>();
        let request_bytes = Self::build_v2c_getbulk_request(
            oids, &config.community, request_id, non_repeaters, max_repetitions
        )?;

        debug!("SNMP client {} sending GETBULK (non_repeaters={}, max_repetitions={})",
            client_id, non_repeaters, max_repetitions);

        Self::send_request_and_handle_response(
            socket, &request_bytes, "GetBulkRequest", config, client_id,
            llm_client, app_state, status_tx, protocol
        ).await
    }

    /// Send SNMP SET request
    async fn send_set_request(
        socket: &Arc<UdpSocket>,
        variables: &[Value],
        config: &Arc<SnmpConfig>,
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: &Arc<SnmpClientProtocol>,
    ) -> Result<()> {
        let request_id = rand::random::<i32>();

        let request_bytes = match config.version {
            SnmpVersion::V1 => Self::build_v1_set_request(variables, &config.community, request_id)?,
            SnmpVersion::V2c => Self::build_v2c_set_request(variables, &config.community, request_id)?,
        };

        debug!("SNMP client {} sending SET for {} variables", client_id, variables.len());

        Self::send_request_and_handle_response(
            socket, &request_bytes, "SetRequest", config, client_id,
            llm_client, app_state, status_tx, protocol
        ).await
    }

    /// Send request and handle response
    async fn send_request_and_handle_response(
        socket: &Arc<UdpSocket>,
        request_bytes: &[u8],
        request_type: &str,
        config: &Arc<SnmpConfig>,
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: &Arc<SnmpClientProtocol>,
    ) -> Result<()> {
        let timeout_duration = Duration::from_millis(config.timeout_ms);
        let mut retries = config.retries;

        loop {
            // Send request
            socket.send(request_bytes).await?;

            // Wait for response with timeout
            let mut buffer = vec![0u8; 65535];
            match timeout(timeout_duration, socket.recv(&mut buffer)).await {
                Ok(Ok(n)) => {
                    let response_data = &buffer[..n];
                    trace!("SNMP response (hex): {}", hex::encode(response_data));

                    // Parse response and call LLM
                    Self::handle_response(
                        response_data, request_type, client_id, llm_client,
                        app_state, status_tx, protocol, socket, config
                    ).await?;

                    return Ok(());
                }
                Ok(Err(e)) => {
                    return Err(e.into());
                }
                Err(_) => {
                    // Timeout
                    if retries > 0 {
                        retries -= 1;
                        debug!("SNMP client {} request timeout, retrying ({} left)", client_id, retries);
                        continue;
                    } else {
                        return Err(anyhow::anyhow!("SNMP request timeout after {} retries", config.retries));
                    }
                }
            }
        }
    }

    /// Handle SNMP response
    async fn handle_response(
        response_data: &[u8],
        request_type: &str,
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: &Arc<SnmpClientProtocol>,
        socket: &Arc<UdpSocket>,
        config: &Arc<SnmpConfig>,
    ) -> Result<()> {
        // Try parsing as v2c first
        let (variables, error_status) = if let Ok(msg) = ber::decode::<v2c::Message<v2::Pdus>>(response_data) {
            Self::extract_v2c_response(&msg)?
        } else if let Ok(msg) = ber::decode::<v1::Message<v1::Pdus>>(response_data) {
            Self::extract_v1_response(&msg)?
        } else {
            return Err(anyhow::anyhow!("Failed to parse SNMP response"));
        };

        debug!("SNMP client {} received response with {} variables, error_status={}",
            client_id, variables.len(), error_status);

        // Call LLM with response
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let event = Event::new(
                &SNMP_CLIENT_RESPONSE_RECEIVED_EVENT,
                serde_json::json!({
                    "request_type": request_type,
                    "variables": variables,
                    "error_status": error_status,
                }),
            );

            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

            match call_llm_for_client(
                llm_client,
                app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                protocol.as_ref(),
                status_tx,
            ).await {
                Ok(ClientLlmResult { actions, memory_updates }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }

                    // Execute follow-up actions (boxed to avoid infinite recursion)
                    Box::pin(Self::execute_actions(
                        actions, protocol, socket, client_id, config,
                        llm_client, app_state, status_tx
                    )).await;
                }
                Err(e) => {
                    error!("LLM error for SNMP client {}: {}", client_id, e);
                }
            }
        }

        Ok(())
    }

    /// Extract variables from v2c response
    fn extract_v2c_response(msg: &v2c::Message<v2::Pdus>) -> Result<(Vec<Value>, i32)> {
        let (var_binds, error_status) = match &msg.data {
            v2::Pdus::Response(resp) => (&resp.0.variable_bindings, resp.0.error_status),
            _ => return Err(anyhow::anyhow!("Unexpected PDU type in response")),
        };

        let variables: Vec<Value> = var_binds.iter().map(|vb| {
            serde_json::json!({
                "oid": vb.name.to_string(),
                "value": Self::format_v2_value(&vb.data),
            })
        }).collect();

        Ok((variables, error_status))
    }

    /// Extract variables from v1 response
    fn extract_v1_response(msg: &v1::Message<v1::Pdus>) -> Result<(Vec<Value>, i32)> {
        let (var_binds, error_status) = match &msg.data {
            v1::Pdus::GetResponse(resp) => {
                let err_status = match &resp.0.error_status {
                    Integer::Primitive(v) => *v as i32,
                    Integer::Variable(big) => big.to_string().parse().unwrap_or(0),
                };
                (&resp.0.variable_bindings, err_status)
            }
            _ => return Err(anyhow::anyhow!("Unexpected PDU type in response")),
        };

        let variables: Vec<Value> = var_binds.iter().map(|vb| {
            serde_json::json!({
                "oid": vb.name.to_string(),
                "value": Self::format_v1_value(&vb.data),
            })
        }).collect();

        Ok((variables, error_status))
    }

    /// Format v2 value for JSON
    fn format_v2_value(value: &v2::ObjectValue) -> Value {
        use v2::ObjectValue::*;
        match value {
            Value(val) => match val {
                v2::Value::Number(n) => serde_json::json!(n),
                v2::Value::String(s) => serde_json::json!(String::from_utf8_lossy(s)),
                v2::Value::Object(_) => serde_json::json!("(object)"),
                v2::Value::Empty => serde_json::json!(null),
            },
            Unspecified => serde_json::json!(null),
            NoSuchObject => serde_json::json!("(no such object)"),
            NoSuchInstance => serde_json::json!("(no such instance)"),
            EndOfMibView => serde_json::json!("(end of MIB view)"),
        }
    }

    /// Format v1 value for JSON
    fn format_v1_value(value: &v1::ObjectValue) -> Value {
        use v1::ObjectValue::*;
        match value {
            Value(val) => match val {
                v1::Value::Number(n) => serde_json::json!(n),
                v1::Value::String(s) => serde_json::json!(String::from_utf8_lossy(s)),
                v1::Value::Object(_) => serde_json::json!("(object)"),
                v1::Value::Empty => serde_json::json!(null),
            },
        }
    }

    // Request builders (simplified - using rasn-snmp encoding)

    fn build_v2c_get_request(oids: &[String], community: &str, request_id: i32) -> Result<Vec<u8>> {
        let var_binds: Vec<v2::VarBind> = oids.iter().map(|oid| {
            v2::VarBind {
                name: oid.parse().unwrap_or_default(),
                data: v2::ObjectValue::Unspecified,
            }
        }).collect();

        let pdu = v2::Pdus::GetRequest(Box::new(v2::Pdu {
            request_id,
            error_status: 0,
            error_index: 0,
            variable_bindings: var_binds,
        }));

        let message = v2c::Message {
            version: 1,  // v2c uses version 1
            community: community.as_bytes().to_vec().into(),
            data: pdu,
        };

        ber::encode(&message).context("Failed to encode SNMP v2c GET request")
    }

    fn build_v2c_getnext_request(oids: &[String], community: &str, request_id: i32) -> Result<Vec<u8>> {
        let var_binds: Vec<v2::VarBind> = oids.iter().map(|oid| {
            v2::VarBind {
                name: oid.parse().unwrap_or_default(),
                data: v2::ObjectValue::Unspecified,
            }
        }).collect();

        let pdu = v2::Pdus::GetNextRequest(Box::new(v2::Pdu {
            request_id,
            error_status: 0,
            error_index: 0,
            variable_bindings: var_binds,
        }));

        let message = v2c::Message {
            version: 1,
            community: community.as_bytes().to_vec().into(),
            data: pdu,
        };

        ber::encode(&message).context("Failed to encode SNMP v2c request")
    }

    fn build_v2c_getbulk_request(
        oids: &[String],
        community: &str,
        request_id: i32,
        non_repeaters: i32,
        max_repetitions: i32,
    ) -> Result<Vec<u8>> {
        let var_binds: Vec<v2::VarBind> = oids.iter().map(|oid| {
            v2::VarBind {
                name: oid.parse().unwrap_or_default(),
                data: v2::ObjectValue::Unspecified,
            }
        }).collect();

        let pdu = v2::Pdus::GetBulkRequest(Box::new(v2::BulkPdu {
            request_id,
            non_repeaters,
            max_repetitions,
            variable_bindings: var_binds,
        }));

        let message = v2c::Message {
            version: 1,
            community: community.as_bytes().to_vec().into(),
            data: pdu,
        };

        ber::encode(&message).context("Failed to encode SNMP v2c request")
    }

    fn build_v2c_set_request(variables: &[Value], community: &str, request_id: i32) -> Result<Vec<u8>> {
        let var_binds: Vec<v2::VarBind> = variables.iter().map(|var| {
            let oid = var.get("oid").and_then(|v| v.as_str()).unwrap_or("1.3.6.1.2.1.1.1.0");
            let value_type = var.get("type").and_then(|v| v.as_str()).unwrap_or("string");
            let value = var.get("value").unwrap_or(&serde_json::json!(null));

            let data = match value_type {
                "integer" => {
                    let n = value.as_i64().unwrap_or(0);
                    v2::ObjectValue::Value(v2::Value::Number(Integer::Primitive(n as isize)))
                }
                "string" => {
                    let s = value.as_str().unwrap_or("");
                    v2::ObjectValue::Value(v2::Value::String(s.as_bytes().to_vec()))
                }
                _ => v2::ObjectValue::Unspecified,
            };

            v2::VarBind {
                name: oid.parse().unwrap_or_default(),
                data,
            }
        }).collect();

        let pdu = v2::Pdus::SetRequest(Box::new(v2::Pdu {
            request_id,
            error_status: 0,
            error_index: 0,
            variable_bindings: var_binds,
        }));

        let message = v2c::Message {
            version: 1,
            community: community.as_bytes().to_vec().into(),
            data: pdu,
        };

        ber::encode(&message).context("Failed to encode SNMP v2c request")
    }

    fn build_v1_get_request(oids: &[String], community: &str, request_id: i32) -> Result<Vec<u8>> {
        let var_binds: Vec<v1::VarBind> = oids.iter().map(|oid| {
            // Create OID from string - if parsing fails, use a simple default
            let oid_obj = oid.split('.').fold(
                rasn::types::ObjectIdentifier::new(vec![0, 0]),
                |mut acc, part| {
                    if let Ok(num) = part.parse::<u32>() {
                        acc.push(num);
                    }
                    acc
                }
            );

            v1::VarBind {
                name: oid_obj,
                value: v1::ObjectValue::Value(v1::Value::Empty),
            }
        }).collect();

        let pdu = v1::Pdus::GetRequest(v1::GetRequest {
            request_id: Integer::Primitive(request_id as isize),
            error_status: Integer::Primitive(0),
            error_index: Integer::Primitive(0),
            variable_bindings: var_binds,
        });

        let message = v1::Message {
            version: Integer::Primitive(0),  // v1 uses version 0
            community: community.as_bytes().to_vec().into(),
            pdu,
        };

        ber::encode(&message).context("Failed to encode SNMP v1 GET request")
    }

    fn build_v1_getnext_request(oids: &[String], community: &str, request_id: i32) -> Result<Vec<u8>> {
        let var_binds: Vec<v1::VarBind> = oids.iter().map(|oid| {
            // Create OID from string
            let oid_obj = oid.split('.').fold(
                rasn::types::ObjectIdentifier::new(vec![0, 0]),
                |mut acc, part| {
                    if let Ok(num) = part.parse::<u32>() {
                        acc.push(num);
                    }
                    acc
                }
            );

            v1::VarBind {
                name: oid_obj,
                value: v1::ObjectValue::Value(v1::Value::Empty),
            }
        }).collect();

        let pdu = v1::Pdus::GetNextRequest(v1::GetNextRequest {
            request_id: Integer::Primitive(request_id as isize),
            error_status: Integer::Primitive(0),
            error_index: Integer::Primitive(0),
            variable_bindings: var_binds,
        });

        let message = v1::Message {
            version: Integer::Primitive(0),
            community: community.as_bytes().to_vec().into(),
            pdu,
        };

        ber::encode(&message).context("Failed to encode SNMP v1 request")
    }

    fn build_v1_set_request(variables: &[Value], community: &str, request_id: i32) -> Result<Vec<u8>> {
        let var_binds: Vec<v1::VarBind> = variables.iter().map(|var| {
            let oid = var.get("oid").and_then(|v| v.as_str()).unwrap_or("1.3.6.1.2.1.1.1.0");
            let value_type = var.get("type").and_then(|v| v.as_str()).unwrap_or("string");
            let value = var.get("value").unwrap_or(&serde_json::json!(null));

            // Create OID from string
            let oid_obj = oid.split('.').fold(
                rasn::types::ObjectIdentifier::new(vec![0, 0]),
                |mut acc, part| {
                    if let Ok(num) = part.parse::<u32>() {
                        acc.push(num);
                    }
                    acc
                }
            );

            let value_obj = match value_type {
                "integer" => {
                    let n = value.as_i64().unwrap_or(0);
                    v1::ObjectValue::Value(v1::Value::Number(Integer::Primitive(n as isize)))
                }
                "string" => {
                    let s = value.as_str().unwrap_or("");
                    v1::ObjectValue::Value(v1::Value::String(s.as_bytes().to_vec()))
                }
                _ => v1::ObjectValue::Value(v1::Value::Empty),
            };

            v1::VarBind {
                name: oid_obj,
                value: value_obj,
            }
        }).collect();

        let pdu = v1::Pdus::SetRequest(v1::SetRequest {
            request_id: Integer::Primitive(request_id as isize),
            error_status: Integer::Primitive(0),
            error_index: Integer::Primitive(0),
            variable_bindings: var_binds,
        });

        let message = v1::Message {
            version: Integer::Primitive(0),
            community: community.as_bytes().to_vec().into(),
            pdu,
        };

        ber::encode(&message).context("Failed to encode SNMP v1 request")
    }
}
