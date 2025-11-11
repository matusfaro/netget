//! JSON-RPC 2.0 client implementation
pub mod actions;

pub use actions::JsonRpcClientProtocol;

use anyhow::{Context, Result};
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
use crate::client::jsonrpc::actions::JSONRPC_CLIENT_RESPONSE_RECEIVED_EVENT;

/// JSON-RPC 2.0 client that makes RPC calls to remote servers
pub struct JsonRpcClient;

impl JsonRpcClient {
    /// Connect to a JSON-RPC server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // JSON-RPC is HTTP-based, so "connection" is logical
        // We'll create an HTTP client and store it in protocol_data

        info!("JSON-RPC client {} initialized for {}", client_id, remote_addr);

        // Build reqwest client
        let _http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to build HTTP client for JSON-RPC")?;

        // Store client in protocol_data
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "jsonrpc_client".to_string(),
                serde_json::json!("initialized"),
            );
            client.set_protocol_field(
                "endpoint".to_string(),
                serde_json::json!(remote_addr.clone()),
            );
            client.set_protocol_field(
                "next_id".to_string(),
                serde_json::json!(1),
            );
        }).await;

        // Update status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] JSON-RPC client {} ready for {}", client_id, remote_addr));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Call LLM with initial connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::jsonrpc::actions::JsonRpcClientProtocol::new());
            let event = Event::new(
                &crate::client::jsonrpc::actions::JSONRPC_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "endpoint": remote_addr,
                }),
            );

            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

            // Spawn task to process initial actions
            let app_state_clone = app_state.clone();
            let llm_client_clone = llm_client.clone();
            let status_tx_clone = status_tx.clone();
            tokio::spawn(async move {
                match call_llm_for_client(
                    &llm_client_clone,
                    &app_state_clone,
                    client_id.to_string(),
                    &instruction,
                    &memory,
                    Some(&event),
                    protocol.as_ref(),
                    &status_tx_clone,
                ).await {
                    Ok(ClientLlmResult { actions, memory_updates }) => {
                        // Update memory
                        if let Some(mem) = memory_updates {
                            app_state_clone.set_memory_for_client(client_id, mem).await;
                        }

                        // Execute actions
                        for action in actions {
                            match protocol.execute_action(action) {
                                Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data }) if name == "jsonrpc_request" => {
                                    let method = data.get("method").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    let params = data.get("params").cloned();
                                    let id = data.get("id").cloned();

                                    if let Err(e) = JsonRpcClient::make_request(
                                        client_id,
                                        method,
                                        params,
                                        id,
                                        app_state_clone.clone(),
                                        llm_client_clone.clone(),
                                        status_tx_clone.clone(),
                                    ).await {
                                        error!("JSON-RPC request failed: {}", e);
                                    }
                                }
                                Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data }) if name == "jsonrpc_batch" => {
                                    let requests = data.get("requests").and_then(|v| v.as_array()).cloned().unwrap_or_default();

                                    if let Err(e) = JsonRpcClient::make_batch_request(
                                        client_id,
                                        requests,
                                        app_state_clone.clone(),
                                        llm_client_clone.clone(),
                                        status_tx_clone.clone(),
                                    ).await {
                                        error!("JSON-RPC batch request failed: {}", e);
                                    }
                                }
                                Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                                    info!("JSON-RPC client {} disconnecting", client_id);
                                    app_state_clone.update_client_status(client_id, ClientStatus::Disconnected).await;
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(e) => {
                        error!("LLM error for JSON-RPC client {}: {}", client_id, e);
                    }
                }
            });
        }

        // Spawn a background task that monitors for client disconnection
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("JSON-RPC client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return a dummy local address (JSON-RPC is connectionless over HTTP)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Make a JSON-RPC request
    pub async fn make_request(
        client_id: ClientId,
        method: String,
        params: Option<serde_json::Value>,
        id: Option<serde_json::Value>,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get endpoint from client
        let endpoint = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("endpoint")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No endpoint found")?;

        info!("JSON-RPC client {} calling method: {}", client_id, method);

        // Build JSON-RPC request
        let mut request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
        });

        if let Some(p) = params {
            request["params"] = p;
        }

        if let Some(request_id) = id {
            request["id"] = request_id.clone();
        }

        // Build HTTP request
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        let response = http_client
            .post(&endpoint)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        let status_code = response.status().as_u16();
        let body_text = response.text().await.unwrap_or_default();

        info!("JSON-RPC client {} received response: {}", client_id, status_code);

        // Parse JSON-RPC response
        if let Ok(response_json) = serde_json::from_str::<serde_json::Value>(&body_text) {
            // Call LLM with response
            if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                let protocol = Arc::new(crate::client::jsonrpc::actions::JsonRpcClientProtocol::new());
                let event = Event::new(
                    &JSONRPC_CLIENT_RESPONSE_RECEIVED_EVENT,
                    response_json.clone(),
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
                    Ok(ClientLlmResult { actions: _, memory_updates }) => {
                        // Update memory
                        if let Some(mem) = memory_updates {
                            app_state.set_memory_for_client(client_id, mem).await;
                        }
                    }
                    Err(e) => {
                        error!("LLM error for JSON-RPC client {}: {}", client_id, e);
                    }
                }
            }
        } else {
            error!("JSON-RPC client {} received invalid JSON response", client_id);
        }

        Ok(())
    }

    /// Make a batch JSON-RPC request
    pub async fn make_batch_request(
        client_id: ClientId,
        requests: Vec<serde_json::Value>,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get endpoint from client
        let endpoint = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("endpoint")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No endpoint found")?;

        info!("JSON-RPC client {} sending batch with {} requests", client_id, requests.len());

        // Build batch request (array of JSON-RPC requests)
        let mut batch = Vec::new();
        for req in requests {
            let mut request = serde_json::json!({
                "jsonrpc": "2.0",
            });

            // Merge the request fields
            if let Some(method) = req.get("method") {
                request["method"] = method.clone();
            }
            if let Some(params) = req.get("params") {
                request["params"] = params.clone();
            }
            if let Some(id) = req.get("id") {
                request["id"] = id.clone();
            }

            batch.push(request);
        }

        // Build HTTP request
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        let response = http_client
            .post(&endpoint)
            .header("Content-Type", "application/json")
            .json(&batch)
            .send()
            .await?;

        let status_code = response.status().as_u16();
        let body_text = response.text().await.unwrap_or_default();

        info!("JSON-RPC client {} received batch response: {}", client_id, status_code);

        // Parse JSON-RPC batch response
        if let Ok(response_json) = serde_json::from_str::<serde_json::Value>(&body_text) {
            // Call LLM with response
            if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                let protocol = Arc::new(crate::client::jsonrpc::actions::JsonRpcClientProtocol::new());
                let event = Event::new(
                    &JSONRPC_CLIENT_RESPONSE_RECEIVED_EVENT,
                    serde_json::json!({
                        "batch": true,
                        "responses": response_json,
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
                    Ok(ClientLlmResult { actions: _, memory_updates }) => {
                        // Update memory
                        if let Some(mem) = memory_updates {
                            app_state.set_memory_for_client(client_id, mem).await;
                        }
                    }
                    Err(e) => {
                        error!("LLM error for JSON-RPC client {}: {}", client_id, e);
                    }
                }
            }
        } else {
            error!("JSON-RPC client {} received invalid JSON batch response", client_id);
        }

        Ok(())
    }
}
