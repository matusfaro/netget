//! XML-RPC client implementation
pub mod actions;

pub use actions::XmlRpcClientProtocol;

use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::Client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::xmlrpc::actions::{XMLRPC_CLIENT_CONNECTED_EVENT, XMLRPC_CLIENT_RESPONSE_RECEIVED_EVENT};
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// XML-RPC client that calls methods on remote servers
pub struct XmlRpcClient;

impl XmlRpcClient {
    /// Connect to an XML-RPC server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        _llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // For XML-RPC, "connection" is logical, not a persistent connection
        // The server URL is stored and used for each method call

        info!("XML-RPC client {} initialized for {}", client_id, remote_addr);

        // Ensure URL is properly formatted
        let server_url = if remote_addr.starts_with("http://") || remote_addr.starts_with("https://") {
            remote_addr.clone()
        } else {
            format!("http://{}", remote_addr)
        };

        // Store server URL in protocol_data
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "server_url".to_string(),
                serde_json::json!(server_url),
            );
        }).await;

        // Update status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        console_info!(status_tx, "[CLIENT] XML-RPC client {} ready for {}", client_id, server_url);
        console_info!(status_tx, "__UPDATE_UI__");

        // Call LLM with initial connected event to trigger first action
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::xmlrpc::actions::XmlRpcClientProtocol::new());
            let event = Event::new(
                &XMLRPC_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "server_url": server_url,
                }),
            );

            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

            if let Ok(ClientLlmResult { actions, memory_updates }) = call_llm_for_client(
                &_llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                protocol.as_ref(),
                &status_tx,
            ).await {
                // Update memory
                if let Some(mem) = memory_updates {
                    app_state.set_memory_for_client(client_id, mem).await;
                }

                // Execute initial actions
                for action in actions {
                    match protocol.execute_action(action) {
                        Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data })
                            if name == "xmlrpc_call" => {
                            if let (Some(method), Some(params)) = (
                                data.get("method_name").and_then(|v| v.as_str()),
                                data.get("params").and_then(|v| v.as_array())
                            ) {
                                // Spawn initial method call
                                let app_state_clone = app_state.clone();
                                let llm_client_clone = _llm_client.clone();
                                let status_tx_clone = status_tx.clone();
                                let method_clone = method.to_string();
                                let params_clone = params.clone();

                                tokio::spawn(async move {
                                    let _ = Self::call_method(
                                        client_id,
                                        method_clone,
                                        params_clone,
                                        app_state_clone,
                                        llm_client_clone,
                                        status_tx_clone,
                                    ).await;
                                });
                            }
                        }
                        Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                            info!("XML-RPC client {} disconnecting on initial action", client_id);
                            return Ok("0.0.0.0:0".parse().unwrap());
                        }
                        _ => {}
                    }
                }
            }
        }

        // Spawn background task that monitors for client disconnection
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("XML-RPC client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return a dummy local address (XML-RPC is connectionless)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Call an XML-RPC method
    pub fn call_method(
        client_id: ClientId,
        method_name: String,
        params: Vec<serde_json::Value>,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>> {
        Box::pin(async move {
            Self::call_method_impl(client_id, method_name, params, app_state, llm_client, status_tx).await
        })
    }

    /// Implementation of call_method
    async fn call_method_impl(
        client_id: ClientId,
        method_name: String,
        params: Vec<serde_json::Value>,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get server URL from client
        let server_url = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("server_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No server URL found")?;

        info!("XML-RPC client {} calling method: {} with {} params",
            client_id, method_name, params.len());

        // Convert JSON values to xmlrpc::Value
        let mut xmlrpc_params = Vec::new();
        for param in params {
            let xmlrpc_value = Self::json_to_xmlrpc_value(param)?;
            xmlrpc_params.push(xmlrpc_value);
        }

        // Clone method_name for the closure
        let method_name_for_log = method_name.clone();

        // Make the call - build request in blocking task
        match tokio::task::spawn_blocking(move || {
            let mut request = xmlrpc::Request::new(&method_name);
            for param in xmlrpc_params {
                request = request.arg(param);
            }
            request.call_url(&server_url)
        }).await {
            Ok(Ok(response)) => {
                info!("XML-RPC client {} received response for {}", client_id, method_name_for_log);

                // Convert response to JSON
                let result_json = Self::xmlrpc_value_to_json(&response);

                // Call LLM with response
                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                    let protocol = Arc::new(crate::client::xmlrpc::actions::XmlRpcClientProtocol::new());
                    let event = Event::new(
                        &XMLRPC_CLIENT_RESPONSE_RECEIVED_EVENT,
                        serde_json::json!({
                            "method_name": method_name_for_log,
                            "result": result_json,
                        }),
                    );

                    let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

                    match call_llm_for_client(
                        &llm_client,
                        &app_state,
                        client_id.to_string(),
                        &instruction,
                        &memory,
                        Some(&event),
                        protocol.as_ref(),
                        &status_tx,
                    ).await {
                        Ok(ClientLlmResult { actions, memory_updates }) => {
                            // Update memory
                            if let Some(mem) = memory_updates {
                                app_state.set_memory_for_client(client_id, mem).await;
                            }

                            // Execute actions returned by LLM
                            for action in actions {
                                match protocol.execute_action(action) {
                                    Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data })
                                        if name == "xmlrpc_call" => {
                                        // Extract method call parameters
                                        if let (Some(method), Some(params)) = (
                                            data.get("method_name").and_then(|v| v.as_str()),
                                            data.get("params").and_then(|v| v.as_array())
                                        ) {
                                            // Recursive call
                                            let _ = Self::call_method(
                                                client_id,
                                                method.to_string(),
                                                params.clone(),
                                                app_state.clone(),
                                                llm_client.clone(),
                                                status_tx.clone(),
                                            ).await;
                                        }
                                    }
                                    Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                                        info!("XML-RPC client {} disconnecting", client_id);
                                        return Ok(());
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Err(e) => {
                            error!("LLM error for XML-RPC client {}: {}", client_id, e);
                        }
                    }
                }

                Ok(())
            }
            Ok(Err(fault)) => {
                let fault_msg = fault.to_string();
                error!("XML-RPC client {} error for {}: {}", client_id, method_name_for_log, fault_msg);

                // Call LLM with fault
                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                    let protocol = Arc::new(crate::client::xmlrpc::actions::XmlRpcClientProtocol::new());
                    let event = Event::new(
                        &XMLRPC_CLIENT_RESPONSE_RECEIVED_EVENT,
                        serde_json::json!({
                            "method_name": method_name_for_log,
                            "fault": {
                                "error": fault_msg.clone(),
                            },
                        }),
                    );

                    let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

                    if let Ok(ClientLlmResult { actions, memory_updates }) = call_llm_for_client(
                        &llm_client,
                        &app_state,
                        client_id.to_string(),
                        &instruction,
                        &memory,
                        Some(&event),
                        protocol.as_ref(),
                        &status_tx,
                    ).await {
                        // Update memory
                        if let Some(mem) = memory_updates {
                            app_state.set_memory_for_client(client_id, mem).await;
                        }

                        // Execute actions (e.g., retry with different params, log error, etc.)
                        for action in actions {
                            match protocol.execute_action(action) {
                                Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data })
                                    if name == "xmlrpc_call" => {
                                    if let (Some(method), Some(params)) = (
                                        data.get("method_name").and_then(|v| v.as_str()),
                                        data.get("params").and_then(|v| v.as_array())
                                    ) {
                                        let _ = Self::call_method(
                                            client_id,
                                            method.to_string(),
                                            params.clone(),
                                            app_state.clone(),
                                            llm_client.clone(),
                                            status_tx.clone(),
                                        ).await;
                                    }
                                }
                                Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                                    // LLM decided to disconnect after fault
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                }

                Err(anyhow::anyhow!("XML-RPC error: {}", fault_msg))
            }
            Err(e) => {
                console_error!(status_tx, "[ERROR] XML-RPC call failed: {}", e);
                Err(e.into())
            }
        }
    }

    /// Convert JSON value to xmlrpc::Value
    fn json_to_xmlrpc_value(json: serde_json::Value) -> Result<xmlrpc::Value> {
        match json {
            serde_json::Value::Null => Ok(xmlrpc::Value::String("".to_string())),
            serde_json::Value::Bool(b) => Ok(xmlrpc::Value::Bool(b)),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    if i >= i32::MIN as i64 && i <= i32::MAX as i64 {
                        Ok(xmlrpc::Value::Int(i as i32))
                    } else {
                        Ok(xmlrpc::Value::Int64(i))
                    }
                } else if let Some(f) = n.as_f64() {
                    Ok(xmlrpc::Value::Double(f))
                } else {
                    Err(anyhow::anyhow!("Invalid number"))
                }
            }
            serde_json::Value::String(s) => Ok(xmlrpc::Value::String(s)),
            serde_json::Value::Array(arr) => {
                let mut xmlrpc_arr = Vec::new();
                for item in arr {
                    xmlrpc_arr.push(Self::json_to_xmlrpc_value(item)?);
                }
                Ok(xmlrpc::Value::Array(xmlrpc_arr))
            }
            serde_json::Value::Object(obj) => {
                let mut xmlrpc_struct = BTreeMap::new();
                for (key, value) in obj {
                    xmlrpc_struct.insert(key, Self::json_to_xmlrpc_value(value)?);
                }
                Ok(xmlrpc::Value::Struct(xmlrpc_struct))
            }
        }
    }

    /// Convert xmlrpc::Value to JSON
    fn xmlrpc_value_to_json(value: &xmlrpc::Value) -> serde_json::Value {
        match value {
            xmlrpc::Value::Int(i) => serde_json::json!(i),
            xmlrpc::Value::Int64(i) => serde_json::json!(i),
            xmlrpc::Value::Bool(b) => serde_json::json!(b),
            xmlrpc::Value::String(s) => serde_json::json!(s),
            xmlrpc::Value::Double(f) => serde_json::json!(f),
            xmlrpc::Value::DateTime(dt) => serde_json::json!(dt.to_string()),
            xmlrpc::Value::Base64(b) => {
                // Convert to base64 string for JSON
                serde_json::json!(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b))
            }
            xmlrpc::Value::Array(arr) => {
                let json_arr: Vec<_> = arr.iter().map(Self::xmlrpc_value_to_json).collect();
                serde_json::json!(json_arr)
            }
            xmlrpc::Value::Struct(s) => {
                let mut obj = serde_json::Map::new();
                for (key, value) in s {
                    obj.insert(key.clone(), Self::xmlrpc_value_to_json(value));
                }
                serde_json::json!(obj)
            }
            xmlrpc::Value::Nil => serde_json::Value::Null,
        }
    }
}
