//! Bitcoin RPC client implementation
pub mod actions;

pub use actions::BitcoinClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::bitcoin::actions::BITCOIN_CLIENT_RESPONSE_RECEIVED_EVENT;

/// Bitcoin RPC client that connects to Bitcoin Core node
pub struct BitcoinClient;

impl BitcoinClient {
    /// Connect to a Bitcoin Core RPC server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        _llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // For Bitcoin RPC, "connection" is logical (HTTP-based JSON-RPC)
        // We don't maintain a persistent connection, but verify connectivity

        info!("Bitcoin RPC client {} initialized for {}", client_id, remote_addr);

        // Parse remote_addr to extract RPC URL
        // Expected format: "http://user:pass@host:port" or "host:port"
        let rpc_url = if remote_addr.starts_with("http://") || remote_addr.starts_with("https://") {
            remote_addr.clone()
        } else {
            // Default to http://
            format!("http://{}", remote_addr)
        };

        // Store RPC URL and auth in protocol_data
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "rpc_url".to_string(),
                serde_json::json!(rpc_url),
            );
            client.set_protocol_field(
                "initialized".to_string(),
                serde_json::json!(true),
            );
        }).await;

        // Update status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] Bitcoin RPC client {} ready for {}", client_id, remote_addr));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Spawn background task that monitors for client disconnection
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("Bitcoin RPC client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return a dummy local address (Bitcoin RPC is connectionless HTTP)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Execute a Bitcoin RPC command
    pub async fn execute_rpc_command(
        client_id: ClientId,
        method: String,
        params: Vec<serde_json::Value>,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get RPC URL from client
        let rpc_url = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("rpc_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No RPC URL found")?;

        info!("Bitcoin RPC client {} executing: {} {:?}", client_id, method, params);

        // Build JSON-RPC request
        let request_body = serde_json::json!({
            "jsonrpc": "1.0",
            "id": "netget",
            "method": method,
            "params": params,
        });

        // Make HTTP POST request to Bitcoin RPC
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()?;

        match http_client.post(&rpc_url)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await {
            Ok(response) => {
                let status = response.status();

                // Get response body
                let response_text = response.text().await.unwrap_or_default();

                info!("Bitcoin RPC client {} received response: {}", client_id, status);

                // Parse JSON-RPC response
                let response_json: serde_json::Value = serde_json::from_str(&response_text)
                    .unwrap_or(serde_json::json!({
                        "error": "Failed to parse response",
                        "raw": response_text
                    }));

                // Extract result or error
                let result = response_json.get("result").cloned();
                let error = response_json.get("error").cloned();

                // Call LLM with response
                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                    let protocol = Arc::new(crate::client::bitcoin::actions::BitcoinClientProtocol::new());
                    let event = Event::new(
                        &BITCOIN_CLIENT_RESPONSE_RECEIVED_EVENT,
                        serde_json::json!({
                            "method": method,
                            "result": result,
                            "error": error,
                            "status_code": status.as_u16(),
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
                            error!("LLM error for Bitcoin RPC client {}: {}", client_id, e);
                        }
                    }
                }

                Ok(())
            }
            Err(e) => {
                error!("Bitcoin RPC client {} request failed: {}", client_id, e);
                let _ = status_tx.send(format!("[ERROR] Bitcoin RPC request failed: {}", e));
                Err(e.into())
            }
        }
    }
}
