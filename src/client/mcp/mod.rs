//! MCP (Model Context Protocol) client implementation
//!
//! Implements JSON-RPC 2.0 client over HTTP for the Model Context Protocol.

pub mod actions;

pub use actions::McpClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::client::mcp::actions::{MCP_CLIENT_CONNECTED_EVENT, MCP_CLIENT_RESPONSE_RECEIVED_EVENT};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::{Client, ClientActionResult};
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::console_error;
use serde_json::{json, Value};

/// JSON-RPC 2.0 request message
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
    id: i64,
}

/// JSON-RPC 2.0 response message
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
    id: Option<i64>,
}

/// JSON-RPC 2.0 notification message
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct JsonRpcNotification {
    jsonrpc: String,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

/// JSON-RPC 2.0 error object
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

/// MCP client that connects to MCP servers
pub struct McpClient;

impl McpClient {
    /// Connect to an MCP server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        info!("MCP client {} connecting to {}", client_id, remote_addr);

        // Build HTTP client
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to build HTTP client")?;

        // Store client data
        let base_url = if remote_addr.starts_with("http://") || remote_addr.starts_with("https://")
        {
            remote_addr.clone()
        } else {
            format!("http://{}", remote_addr)
        };

        app_state
            .with_client_mut(client_id, |client| {
                client.set_protocol_field("base_url".to_string(), serde_json::json!(base_url));
                client.set_protocol_field("request_id".to_string(), serde_json::json!(1));
                client.set_protocol_field("initialized".to_string(), serde_json::json!(false));
            })
            .await;

        // Phase 1: Send initialize request
        let init_response = Self::send_initialize_request(
            &http_client,
            &base_url,
            client_id,
            &app_state,
            &status_tx,
        )
        .await?;

        // Parse server info from response
        let server_info = init_response
            .get("serverInfo")
            .and_then(|v| v.as_object())
            .context("Missing serverInfo in initialize response")?;

        let server_name = server_info
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let server_version = server_info
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let capabilities = init_response
            .get("capabilities")
            .cloned()
            .unwrap_or(json!({}));

        // Store server capabilities
        app_state
            .with_client_mut(client_id, |client| {
                client.set_protocol_field("server_info".to_string(), json!(server_info));
                client.set_protocol_field("capabilities".to_string(), capabilities.clone());
            })
            .await;

        // Phase 2: Send initialized notification
        Self::send_initialized_notification(&http_client, &base_url, &status_tx).await?;

        // Mark as initialized
        app_state
            .with_client_mut(client_id, |client| {
                client.set_protocol_field("initialized".to_string(), serde_json::json!(true));
            })
            .await;

        // Update status to connected
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!(
            "[CLIENT] MCP client {} initialized with server {}",
            client_id, server_name
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        info!("MCP client {} initialization complete", client_id);

        // Create connected event and call LLM
        let event = Event::new(
            &MCP_CLIENT_CONNECTED_EVENT,
            json!({
                "server_name": server_name,
                "server_version": server_version,
                "capabilities": capabilities,
            }),
        );

        // Spawn task to handle LLM interactions
        let http_client_clone = http_client.clone();
        tokio::spawn(async move {
            let protocol = Arc::new(McpClientProtocol::new());

            // Get instruction and memory
            let instruction = app_state
                .get_instruction_for_client(client_id)
                .await
                .unwrap_or_default();
            let memory = app_state
                .get_memory_for_client(client_id)
                .await
                .unwrap_or_default();

            // Initial LLM call with connected event
            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                protocol.as_ref(),
                &status_tx,
            )
            .await
            {
                Ok(result) => {
                    // Update memory
                    if let Some(mem) = result.memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }

                    // Execute actions from LLM
                    if let Err(e) = Self::execute_llm_actions(
                        client_id,
                        result.actions,
                        &http_client_clone,
                        &llm_client,
                        &app_state,
                        &status_tx,
                        protocol.clone(),
                    )
                    .await
                    {
                        error!("Failed to execute LLM actions: {}", e);
                        let _ = status_tx.send(format!("[ERROR] Failed to execute actions: {}", e));
                    }
                }
                Err(e) => {
                    console_error!(status_tx, "Failed to call LLM: {}", e);
                }
            }

            // Monitor for disconnection
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                if app_state.get_client(client_id).await.is_none() {
                    info!("MCP client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return dummy local address (MCP is HTTP-based)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Send initialize request to MCP server
    async fn send_initialize_request(
        http_client: &reqwest::Client,
        base_url: &str,
        client_id: ClientId,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<Value> {
        let request_id: i64 = app_state
            .with_client_mut(client_id, |client| {
                let id = client
                    .get_protocol_field("request_id")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(1);
                client.set_protocol_field("request_id".to_string(), json!(id + 1));
                id
            })
            .await
            .context("Client not found")?;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "initialize".to_string(),
            params: Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "roots": {
                        "listChanged": false
                    }
                },
                "clientInfo": {
                    "name": "netget-mcp-client",
                    "version": "0.1.0"
                }
            })),
            id: request_id,
        };

        debug!("Sending initialize request: {:?}", request);
        let _ = status_tx.send("[CLIENT] Sending MCP initialize request".to_string());

        let response = http_client
            .post(base_url)
            .json(&request)
            .send()
            .await
            .context("Failed to send initialize request")?;

        let status = response.status();
        let response_text = response.text().await?;

        debug!(
            "Initialize response status: {}, body: {}",
            status, response_text
        );

        if !status.is_success() {
            return Err(anyhow::anyhow!("HTTP error {}: {}", status, response_text));
        }

        let json_response: JsonRpcResponse =
            serde_json::from_str(&response_text).context("Failed to parse initialize response")?;

        if let Some(error) = json_response.error {
            return Err(anyhow::anyhow!(
                "JSON-RPC error {}: {}",
                error.code,
                error.message
            ));
        }

        json_response
            .result
            .context("Missing result in initialize response")
    }

    /// Send initialized notification to MCP server
    async fn send_initialized_notification(
        http_client: &reqwest::Client,
        base_url: &str,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let notification = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: "initialized".to_string(),
            params: Some(json!({})),
        };

        debug!("Sending initialized notification: {:?}", notification);
        let _ = status_tx.send("[CLIENT] Sending MCP initialized notification".to_string());

        http_client
            .post(base_url)
            .json(&notification)
            .send()
            .await
            .context("Failed to send initialized notification")?;

        Ok(())
    }

    /// Execute actions returned by LLM
    fn execute_llm_actions<'a>(
        client_id: ClientId,
        actions: Vec<Value>,
        http_client: &'a reqwest::Client,
        llm_client: &'a OllamaClient,
        app_state: &'a Arc<AppState>,
        status_tx: &'a mpsc::UnboundedSender<String>,
        protocol: Arc<McpClientProtocol>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            for action in actions {
                debug!("Executing action: {:?}", action);

                // Parse action
                let action_result = protocol.as_ref().execute_action(action.clone())?;

                match action_result {
                    ClientActionResult::Disconnect => {
                        info!("Disconnecting MCP client {}", client_id);
                        app_state
                            .update_client_status(client_id, ClientStatus::Disconnected)
                            .await;
                        let _ = status_tx
                            .send(format!("[CLIENT] MCP client {} disconnected", client_id));
                        break;
                    }
                    ClientActionResult::Custom { name, data } => {
                        // Execute MCP-specific action
                        match Self::execute_mcp_action(
                            client_id,
                            &name,
                            &data,
                            http_client,
                            app_state,
                            status_tx,
                        )
                        .await
                        {
                            Ok(response) => {
                                // Create response event
                                let event = Event::new(
                                    &MCP_CLIENT_RESPONSE_RECEIVED_EVENT,
                                    json!({
                                        "method": name,
                                        "result": response,
                                    }),
                                );

                                // Get instruction and memory
                                let instruction = app_state
                                    .get_instruction_for_client(client_id)
                                    .await
                                    .unwrap_or_default();
                                let memory = app_state
                                    .get_memory_for_client(client_id)
                                    .await
                                    .unwrap_or_default();

                                // Call LLM with response
                                match call_llm_for_client(
                                    llm_client,
                                    app_state,
                                    client_id.to_string(),
                                    &instruction,
                                    &memory,
                                    Some(&event),
                                    protocol.as_ref(),
                                    status_tx,
                                )
                                .await
                                {
                                    Ok(result) => {
                                        // Update memory
                                        if let Some(mem) = result.memory_updates {
                                            app_state.set_memory_for_client(client_id, mem).await;
                                        }

                                        // Recursively execute more actions
                                        if let Err(e) = Self::execute_llm_actions(
                                            client_id,
                                            result.actions,
                                            http_client,
                                            llm_client,
                                            app_state,
                                            status_tx,
                                            protocol.clone(),
                                        )
                                        .await
                                        {
                                            error!("Failed to execute nested actions: {}", e);
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to call LLM: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Failed to execute MCP action {}: {}", name, e);
                                let _ = status_tx
                                    .send(format!("[ERROR] Failed to execute {}: {}", name, e));
                            }
                        }
                    }
                    _ => {
                        debug!("Ignoring non-custom action result");
                    }
                }
            }

            Ok(())
        })
    }

    /// Execute a specific MCP action
    async fn execute_mcp_action(
        client_id: ClientId,
        action_name: &str,
        data: &Value,
        http_client: &reqwest::Client,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<Value> {
        let (base_url, request_id): (String, i64) = app_state
            .with_client_mut(client_id, |client| {
                let url = client
                    .get_protocol_field("base_url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .expect("Missing base_url");

                let id = client
                    .get_protocol_field("request_id")
                    .and_then(|v| v.as_i64())
                    .expect("Missing request_id");

                client.set_protocol_field("request_id".to_string(), json!(id + 1));

                (url, id)
            })
            .await
            .context("Client not found")?;

        let (method, params) = match action_name {
            "mcp_list_resources" => ("resources/list".to_string(), None),
            "mcp_read_resource" => {
                let uri = data
                    .get("uri")
                    .and_then(|v| v.as_str())
                    .context("Missing uri in read_resource")?;
                ("resources/read".to_string(), Some(json!({"uri": uri})))
            }
            "mcp_list_tools" => ("tools/list".to_string(), None),
            "mcp_call_tool" => {
                let name = data
                    .get("name")
                    .and_then(|v| v.as_str())
                    .context("Missing name in call_tool")?;
                let arguments = data.get("arguments").cloned().unwrap_or(json!({}));
                (
                    "tools/call".to_string(),
                    Some(json!({
                        "name": name,
                        "arguments": arguments
                    })),
                )
            }
            "mcp_list_prompts" => ("prompts/list".to_string(), None),
            "mcp_get_prompt" => {
                let name = data
                    .get("name")
                    .and_then(|v| v.as_str())
                    .context("Missing name in get_prompt")?;
                let arguments = data.get("arguments").cloned();
                (
                    "prompts/get".to_string(),
                    Some(json!({
                        "name": name,
                        "arguments": arguments
                    })),
                )
            }
            _ => return Err(anyhow::anyhow!("Unknown MCP action: {}", action_name)),
        };

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: method.clone(),
            params,
            id: request_id,
        };

        debug!("Sending MCP request: {:?}", request);
        let _ = status_tx.send(format!("[CLIENT] Sending MCP request: {}", method));

        let response = http_client
            .post(&base_url)
            .json(&request)
            .send()
            .await
            .context("Failed to send MCP request")?;

        let status = response.status();
        let response_text = response.text().await?;

        debug!("MCP response status: {}, body: {}", status, response_text);

        if !status.is_success() {
            return Err(anyhow::anyhow!("HTTP error {}: {}", status, response_text));
        }

        let json_response: JsonRpcResponse =
            serde_json::from_str(&response_text).context("Failed to parse MCP response")?;

        if let Some(error) = json_response.error {
            return Err(anyhow::anyhow!(
                "JSON-RPC error {}: {}",
                error.code,
                error.message
            ));
        }

        json_response
            .result
            .context("Missing result in MCP response")
    }
}
