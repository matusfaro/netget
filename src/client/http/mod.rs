//! HTTP client implementation
pub mod actions;

pub use actions::HttpClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::client::http::actions::{
    HTTP_CLIENT_CONNECTED_EVENT, HTTP_CLIENT_RESPONSE_RECEIVED_EVENT,
};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

/// HTTP client that makes requests to remote HTTP servers
pub struct HttpClient;

impl HttpClient {
    /// Connect to an HTTP server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // For HTTP, "connection" is logical, not a persistent TCP connection
        // We'll create an HTTP client and store it in protocol_data

        info!("HTTP client {} initialized for {}", client_id, remote_addr);

        // Build reqwest client with HTTPS and HTTP/2 support
        // Protocol versions are automatically negotiated via ALPN during TLS handshake
        let _http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .use_rustls_tls() // Use rustls for HTTPS (HTTP/1.1 and HTTP/2)
            .build()
            .context("Failed to build HTTP client")?;

        // Store client in protocol_data
        // Ensure base_url has http:// scheme
        let base_url = if remote_addr.starts_with("http://") || remote_addr.starts_with("https://") {
            remote_addr.clone()
        } else {
            format!("http://{}", remote_addr)
        };

        app_state
            .with_client_mut(client_id, |client| {
                client.set_protocol_field(
                    "http_client".to_string(),
                    serde_json::json!("initialized"),
                );
                client.set_protocol_field("base_url".to_string(), serde_json::json!(base_url));
            })
            .await;

        // Update status
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!(
            "[CLIENT] HTTP client {} ready for {}",
            client_id, remote_addr
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Call LLM with http_connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let event = Event::new(
                &HTTP_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "base_url": base_url.clone(),
                }),
            );

            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &String::new(), // No memory yet for initial connection
                Some(&event),
                &crate::client::http::actions::HttpClientProtocol,
                &status_tx,
            )
            .await
            {
                Ok(result) => {
                    // Execute actions from LLM response
                    use crate::llm::actions::client_trait::{Client, ClientActionResult};
                    let protocol = crate::client::http::actions::HttpClientProtocol;

                    for action in result.actions {
                        match protocol.execute_action(action.clone()) {
                            Ok(ClientActionResult::Custom { name, data }) => {
                                if name == "http_request" {
                                    // Extract request parameters
                                    let method = data["method"].as_str().unwrap_or("GET").to_string();
                                    let path = data["path"].as_str().unwrap_or("/").to_string();
                                    let headers = data["headers"].as_object().cloned();
                                    let body = data["body"].as_str().map(|s| s.to_string());

                                    // Actually make the HTTP request
                                    let llm_clone = llm_client.clone();
                                    let state_clone = app_state.clone();
                                    let status_clone = status_tx.clone();

                                    tokio::spawn(async move {
                                        if let Err(e) = HttpClient::make_request(
                                            client_id,
                                            method,
                                            path,
                                            headers,
                                            body,
                                            state_clone,
                                            llm_clone,
                                            status_clone,
                                        ).await {
                                            error!("HTTP request failed: {}", e);
                                        }
                                    });
                                }
                            }
                            Ok(ClientActionResult::Disconnect) => {
                                info!("LLM requested disconnect after connect");
                                return Ok("0.0.0.0:0".parse().unwrap());
                            }
                            Ok(ClientActionResult::WaitForMore) => {
                                // No action needed
                            }
                            Ok(ClientActionResult::SendData(_)) | Ok(ClientActionResult::NoAction) | Ok(ClientActionResult::Multiple(_)) => {
                                // These are not applicable for HTTP client in this context
                            }
                            Err(e) => {
                                error!("Action execution error: {}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error on http_connected event: {}", e);
                }
            }
        }

        // For HTTP client, we'll spawn a background task that processes LLM-requested actions
        // The actual requests are made on-demand via actions, not in a read loop
        tokio::spawn(async move {
            // This task monitors for client disconnection requests
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("HTTP client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return a dummy local address (HTTP is connectionless)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Make an HTTP request
    #[allow(clippy::too_many_arguments)]
    pub async fn make_request(
        client_id: ClientId,
        method: String,
        path: String,
        headers: Option<serde_json::Map<String, serde_json::Value>>,
        body: Option<String>,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get base URL from client
        let base_url = app_state
            .with_client_mut(client_id, |client| {
                client
                    .get_protocol_field("base_url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .await
            .flatten()
            .context("No base URL found")?;

        let url = if path.starts_with("http://") || path.starts_with("https://") {
            path.clone()
        } else {
            format!("{}{}", base_url, path)
        };

        info!(
            "HTTP client {} making request: {} {}",
            client_id, method, url
        );

        // Build request with HTTPS and HTTP/2 support
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .use_rustls_tls() // Use rustls for HTTPS (HTTP/1.1 and HTTP/2)
            .build()?;

        let mut request = match method.to_uppercase().as_str() {
            "GET" => http_client.get(&url),
            "POST" => http_client.post(&url),
            "PUT" => http_client.put(&url),
            "DELETE" => http_client.delete(&url),
            "HEAD" => http_client.head(&url),
            "PATCH" => http_client.patch(&url),
            _ => return Err(anyhow::anyhow!("Unsupported HTTP method: {}", method)),
        };

        // Add headers
        if let Some(hdrs) = headers {
            for (key, value) in hdrs {
                if let Some(val_str) = value.as_str() {
                    request = request.header(&key, val_str);
                }
            }
        }

        // Add body
        if let Some(body_str) = body {
            request = request.body(body_str);
        }

        // Make request
        match request.send().await {
            Ok(response) => {
                let status = response.status();
                let status_code = status.as_u16();

                // Get headers
                let mut resp_headers = serde_json::Map::new();
                for (name, value) in response.headers() {
                    if let Ok(val_str) = value.to_str() {
                        resp_headers.insert(name.to_string(), serde_json::json!(val_str));
                    }
                }

                // Get body
                let body_text = response.text().await.unwrap_or_default();

                info!(
                    "HTTP client {} received response: {} ({})",
                    client_id, status_code, status
                );

                // Call LLM with response
                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                    let protocol =
                        Arc::new(crate::client::http::actions::HttpClientProtocol::new());
                    let event = Event::new(
                        &HTTP_CLIENT_RESPONSE_RECEIVED_EVENT,
                        serde_json::json!({
                            "status_code": status_code,
                            "status_text": status.to_string(),
                            "headers": resp_headers,
                            "body": body_text,
                        }),
                    );

                    let memory = app_state
                        .get_memory_for_client(client_id)
                        .await
                        .unwrap_or_default();

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
                        Ok(ClientLlmResult {
                            actions: _,
                            memory_updates,
                        }) => {
                            // Update memory
                            if let Some(mem) = memory_updates {
                                app_state.set_memory_for_client(client_id, mem).await;
                            }
                        }
                        Err(e) => {
                            error!("LLM error for HTTP client {}: {}", client_id, e);
                        }
                    }
                }

                Ok(())
            }
            Err(e) => {
                error!("HTTP client {} request failed: {}", client_id, e);
                let _ = status_tx.send(format!("[ERROR] HTTP request failed: {}", e));
                Err(e.into())
            }
        }
    }
}
