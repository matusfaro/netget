//! HTTP/2 client implementation
pub mod actions;

pub use actions::Http2ClientProtocol;

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
use crate::client::http2::actions::HTTP2_CLIENT_RESPONSE_RECEIVED_EVENT;

/// HTTP/2 client that makes requests to remote HTTP/2 servers
pub struct Http2Client;

impl Http2Client {
    /// Connect to an HTTP/2 server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        _llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // For HTTP/2, "connection" is logical, with persistent multiplexed streams
        // We'll create an HTTP/2 client and store it in protocol_data

        info!("HTTP/2 client {} initialized for {}", client_id, remote_addr);

        // Build reqwest client with HTTP/2 enabled
        let _http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .http2_prior_knowledge()  // Force HTTP/2 (without ALPN negotiation)
            .build()
            .context("Failed to build HTTP/2 client")?;

        // Store client in protocol_data
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "http2_client".to_string(),
                serde_json::json!("initialized"),
            );
            client.set_protocol_field(
                "base_url".to_string(),
                serde_json::json!(remote_addr),
            );
        }).await;

        // Update status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] HTTP/2 client {} ready for {}", client_id, remote_addr));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // For HTTP/2 client, we'll spawn a background task that processes LLM-requested actions
        // The actual requests are made on-demand via actions, not in a read loop
        tokio::spawn(async move {
            // This task monitors for client disconnection requests
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("HTTP/2 client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return a dummy local address (HTTP/2 is connectionless)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Make an HTTP/2 request
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
        let base_url = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("base_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No base URL found")?;

        let url = if path.starts_with("http://") || path.starts_with("https://") {
            path.clone()
        } else {
            format!("{}{}", base_url, path)
        };

        info!("HTTP/2 client {} making request: {} {}", client_id, method, url);

        // Build request with HTTP/2 enabled
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .http2_prior_knowledge()  // Force HTTP/2
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
                let version = response.version();

                // Get headers
                let mut resp_headers = serde_json::Map::new();
                for (name, value) in response.headers() {
                    if let Ok(val_str) = value.to_str() {
                        resp_headers.insert(name.to_string(), serde_json::json!(val_str));
                    }
                }

                // Get body
                let body_text = response.text().await.unwrap_or_default();

                info!("HTTP/2 client {} received response: {} ({}) version: {:?}",
                    client_id, status_code, status, version);

                // Call LLM with response
                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                    let protocol = Arc::new(crate::client::http2::actions::Http2ClientProtocol::new());
                    let event = Event::new(
                        &HTTP2_CLIENT_RESPONSE_RECEIVED_EVENT,
                        serde_json::json!({
                            "status_code": status_code,
                            "status_text": status.to_string(),
                            "http_version": format!("{:?}", version),
                            "headers": resp_headers,
                            "body": body_text,
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
                            error!("LLM error for HTTP/2 client {}: {}", client_id, e);
                        }
                    }
                }

                Ok(())
            }
            Err(e) => {
                error!("HTTP/2 client {} request failed: {}", client_id, e);
                let _ = status_tx.send(format!("[ERROR] HTTP/2 request failed: {}", e));
                Err(e.into())
            }
        }
    }
}
