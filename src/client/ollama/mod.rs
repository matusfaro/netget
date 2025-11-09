//! Ollama client implementation
pub mod actions;

pub use actions::OllamaClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::{Event, StartupParams};
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::ollama::actions::OLLAMA_CLIENT_RESPONSE_RECEIVED_EVENT;

/// Ollama client that connects to the Ollama API
pub struct OllamaClientImpl;

impl OllamaClientImpl {
    /// Connect to Ollama API with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        _llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        _startup_params: Option<StartupParams>,
    ) -> Result<SocketAddr> {
        info!("Ollama client {} initializing with API endpoint: {}", client_id, remote_addr);

        // Store only endpoint in protocol_data (no model storage - LLM must provide model on every call)
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "api_endpoint".to_string(),
                serde_json::json!(remote_addr),
            );
        }).await;

        // Update status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] Ollama client {} ready (endpoint: {})", client_id, remote_addr));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // For Ollama client, spawn a background task that monitors for client removal
        // The actual API requests are made on-demand via actions
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("Ollama client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return a dummy local address (Ollama is a remote API, not a local connection)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Make a generate request (model is required)
    pub async fn make_generate_request(
        client_id: ClientId,
        prompt: String,
        model: String,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get API endpoint from client (model must be provided by LLM on every call)
        let api_endpoint = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("api_endpoint")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No API endpoint found")?;

        info!("Ollama client {} making generate request with model: {}", client_id, model);

        // Build Ollama client with custom endpoint
        let client = reqwest::Client::new();
        let url = format!("{}/api/generate", api_endpoint);

        let request_body = serde_json::json!({
            "model": model,
            "prompt": prompt,
            "stream": false
        });

        // Make request
        match client.post(&url)
            .json(&request_body)
            .send()
            .await
        {
            Ok(response) => {
                let status_code = response.status();
                let response_json: serde_json::Value = response.json().await?;

                if status_code.is_success() {
                    let response_text = response_json.get("response")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    info!("Ollama client {} received generate response", client_id);

                    // Call LLM with response
                    if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                        let protocol = Arc::new(crate::client::ollama::actions::OllamaClientProtocol::new());
                        let event = Event::new(
                            &OLLAMA_CLIENT_RESPONSE_RECEIVED_EVENT,
                            serde_json::json!({
                                "response_type": "generate",
                                "content": response_text,
                                "model": model,
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
                                error!("LLM error for Ollama client {}: {}", client_id, e);
                            }
                        }
                    }

                    Ok(())
                } else {
                    let error_msg = response_json.get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown error")
                        .to_string();

                    Err(anyhow::anyhow!("Ollama API error: {}", error_msg))
                }
            }
            Err(e) => {
                error!("Ollama client {} request failed: {}", client_id, e);
                let _ = status_tx.send(format!("[ERROR] Ollama request failed: {}", e));
                Err(e.into())
            }
        }
    }

    /// Make a chat completion request (model is required)
    pub async fn make_chat_request(
        client_id: ClientId,
        messages: serde_json::Value,
        model: String,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get API endpoint from client (model must be provided by LLM on every call)
        let api_endpoint = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("api_endpoint")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No API endpoint found")?;

        info!("Ollama client {} making chat request with model: {}", client_id, model);

        // Build Ollama client with custom endpoint
        let client = reqwest::Client::new();
        let url = format!("{}/api/chat", api_endpoint);

        let request_body = serde_json::json!({
            "model": model,
            "messages": messages,
            "stream": false
        });

        // Make request
        match client.post(&url)
            .json(&request_body)
            .send()
            .await
        {
            Ok(response) => {
                let status_code = response.status();
                let response_json: serde_json::Value = response.json().await?;

                if status_code.is_success() {
                    let message_content = response_json.get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    info!("Ollama client {} received chat response", client_id);

                    // Call LLM with response
                    if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                        let protocol = Arc::new(crate::client::ollama::actions::OllamaClientProtocol::new());
                        let event = Event::new(
                            &OLLAMA_CLIENT_RESPONSE_RECEIVED_EVENT,
                            serde_json::json!({
                                "response_type": "chat",
                                "content": message_content,
                                "model": model,
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
                                error!("LLM error for Ollama client {}: {}", client_id, e);
                            }
                        }
                    }

                    Ok(())
                } else {
                    let error_msg = response_json.get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown error")
                        .to_string();

                    Err(anyhow::anyhow!("Ollama API error: {}", error_msg))
                }
            }
            Err(e) => {
                error!("Ollama client {} request failed: {}", client_id, e);
                let _ = status_tx.send(format!("[ERROR] Ollama request failed: {}", e));
                Err(e.into())
            }
        }
    }

    /// List available models
    pub async fn list_models(
        client_id: ClientId,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get API configuration from client
        let api_endpoint = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("api_endpoint")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No API endpoint found")?;

        info!("Ollama client {} listing models", client_id);

        // Build Ollama client with custom endpoint
        let client = reqwest::Client::new();
        let url = format!("{}/api/tags", api_endpoint);

        // Make request
        match client.get(&url).send().await {
            Ok(response) => {
                let status_code = response.status();
                let response_json: serde_json::Value = response.json().await?;

                if status_code.is_success() {
                    let models = response_json.get("models")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|m| m.get("name").and_then(|n| n.as_str()))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();

                    info!("Ollama client {} found {} models", client_id, models.len());

                    // Call LLM with response
                    if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                        let protocol = Arc::new(crate::client::ollama::actions::OllamaClientProtocol::new());
                        let event = Event::new(
                            &OLLAMA_CLIENT_RESPONSE_RECEIVED_EVENT,
                            serde_json::json!({
                                "response_type": "models",
                                "content": format!("Found {} models", models.len()),
                                "models": models,
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
                                error!("LLM error for Ollama client {}: {}", client_id, e);
                            }
                        }
                    }

                    Ok(())
                } else {
                    let error_msg = response_json.get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown error")
                        .to_string();

                    Err(anyhow::anyhow!("Ollama API error: {}", error_msg))
                }
            }
            Err(e) => {
                error!("Ollama client {} request failed: {}", client_id, e);
                let _ = status_tx.send(format!("[ERROR] Ollama request failed: {}", e));
                Err(e.into())
            }
        }
    }

    /// Generate embeddings (model is required)
    pub async fn make_embeddings_request(
        client_id: ClientId,
        prompt: String,
        model: String,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get API endpoint from client (model must be provided by LLM on every call)
        let api_endpoint = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("api_endpoint")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No API endpoint found")?;

        info!("Ollama client {} making embeddings request with model: {}", client_id, model);

        // Build Ollama client with custom endpoint
        let client = reqwest::Client::new();
        let url = format!("{}/api/embeddings", api_endpoint);

        let request_body = serde_json::json!({
            "model": model,
            "prompt": prompt
        });

        // Make request
        match client.post(&url)
            .json(&request_body)
            .send()
            .await
        {
            Ok(response) => {
                let status_code = response.status();
                let response_json: serde_json::Value = response.json().await?;

                if status_code.is_success() {
                    let embedding = response_json.get("embedding")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.len())
                        .unwrap_or(0);

                    info!("Ollama client {} received embeddings ({} dimensions)", client_id, embedding);

                    // Call LLM with response
                    if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                        let protocol = Arc::new(crate::client::ollama::actions::OllamaClientProtocol::new());
                        let event = Event::new(
                            &OLLAMA_CLIENT_RESPONSE_RECEIVED_EVENT,
                            serde_json::json!({
                                "response_type": "embeddings",
                                "content": format!("Generated embeddings with {} dimensions", embedding),
                                "model": model,
                                "dimensions": embedding,
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
                                error!("LLM error for Ollama client {}: {}", client_id, e);
                            }
                        }
                    }

                    Ok(())
                } else {
                    let error_msg = response_json.get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown error")
                        .to_string();

                    Err(anyhow::anyhow!("Ollama API error: {}", error_msg))
                }
            }
            Err(e) => {
                error!("Ollama client {} embeddings request failed: {}", client_id, e);
                let _ = status_tx.send(format!("[ERROR] Ollama embeddings request failed: {}", e));
                Err(e.into())
            }
        }
    }
}
