//! WebDAV client implementation
pub mod actions;

pub use actions::WebdavClientProtocol;

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
use crate::client::webdav::actions::WEBDAV_CLIENT_RESPONSE_RECEIVED_EVENT;

/// WebDAV client that makes requests to remote WebDAV servers
pub struct WebdavClient;

impl WebdavClient {
    /// Connect to a WebDAV server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        _llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // For WebDAV, "connection" is logical, not a persistent TCP connection
        // We'll create an HTTP client and store it in protocol_data

        info!("WebDAV client {} initialized for {}", client_id, remote_addr);

        // Build reqwest client with basic auth support if credentials provided
        let _http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to build HTTP client")?;

        // Store client in protocol_data
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "http_client".to_string(),
                serde_json::json!("initialized"),
            );
            client.set_protocol_field(
                "base_url".to_string(),
                serde_json::json!(remote_addr),
            );
        }).await;

        // Update status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] WebDAV client {} ready for {}", client_id, remote_addr));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Send initial connected event to LLM
        tokio::spawn(async move {
            if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                let protocol = Arc::new(crate::client::webdav::actions::WebdavClientProtocol::new());
                let event = Event::new(
                    &crate::client::webdav::actions::WEBDAV_CLIENT_CONNECTED_EVENT,
                    serde_json::json!({
                        "base_url": app_state.with_client_mut(client_id, |c|
                            c.get_protocol_field("base_url").and_then(|v| v.as_str().map(|s| s.to_string()))
                        ).await.flatten().unwrap_or_default(),
                    }),
                );

                let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

                match call_llm_for_client(
                    &_llm_client,
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

                        // Execute actions
                        for action in actions {
                            if let Err(e) = Self::execute_webdav_action(
                                action,
                                &protocol,
                                &_llm_client,
                                &app_state,
                                &status_tx,
                                client_id,
                                &instruction,
                            ).await {
                                error!("Failed to execute WebDAV action: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("LLM error for WebDAV client {}: {}", client_id, e);
                    }
                }
            }
        });

        // Return a dummy local address (WebDAV is connectionless)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Execute a WebDAV action from the LLM
    async fn execute_webdav_action(
        action: serde_json::Value,
        protocol: &Arc<WebdavClientProtocol>,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        client_id: ClientId,
        _instruction: &str,
    ) -> Result<()> {
        use crate::llm::actions::client_trait::{Client, ClientActionResult};

        let result = protocol.as_ref().execute_action(action)?;

        match result {
            ClientActionResult::Custom { name, data } if name == "webdav_request" => {
                let method = data.get("method")
                    .and_then(|v| v.as_str())
                    .context("Missing 'method' in webdav_request")?
                    .to_string();

                let path = data.get("path")
                    .and_then(|v| v.as_str())
                    .context("Missing 'path' in webdav_request")?
                    .to_string();

                // Build headers from action data
                let mut headers = Vec::new();

                // Add Depth header for PROPFIND/COPY
                if let Some(depth) = data.get("depth").and_then(|v| v.as_str()) {
                    headers.push(("Depth".to_string(), depth.to_string()));
                }

                // Add Destination header for COPY/MOVE
                if let Some(destination) = data.get("destination").and_then(|v| v.as_str()) {
                    // Get base URL and construct full destination URL
                    let base_url = app_state.with_client_mut(client_id, |c|
                        c.get_protocol_field("base_url").and_then(|v| v.as_str().map(|s| s.to_string()))
                    ).await.flatten().unwrap_or_default();

                    let dest_url = if destination.starts_with("http") {
                        destination.to_string()
                    } else {
                        format!("{}{}", base_url, destination)
                    };
                    headers.push(("Destination".to_string(), dest_url));
                }

                // Add Overwrite header for COPY/MOVE
                if let Some(overwrite) = data.get("overwrite").and_then(|v| v.as_bool()) {
                    headers.push(("Overwrite".to_string(), if overwrite { "T" } else { "F" }.to_string()));
                }

                // Add Content-Type for PUT
                if let Some(content_type) = data.get("content_type").and_then(|v| v.as_str()) {
                    headers.push(("Content-Type".to_string(), content_type.to_string()));
                } else if method == "PUT" {
                    headers.push(("Content-Type".to_string(), "application/octet-stream".to_string()));
                } else if method == "PROPFIND" {
                    headers.push(("Content-Type".to_string(), "application/xml".to_string()));
                }

                // Build request body
                let body = if method == "PROPFIND" {
                    // Build PROPFIND XML body
                    let properties = data.get("properties")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect::<Vec<String>>()
                        });
                    Some(Self::build_propfind_body(properties))
                } else if method == "PUT" {
                    // Use content from action
                    data.get("content").and_then(|v| v.as_str()).map(|s| s.to_string())
                } else {
                    None
                };

                // Make the WebDAV request
                Self::make_request(
                    client_id,
                    method,
                    path,
                    Some(headers),
                    body,
                    Arc::clone(app_state),
                    llm_client.clone(),
                    status_tx.clone(),
                ).await?;
            }
            ClientActionResult::Disconnect => {
                info!("WebDAV client {} disconnecting", client_id);
                app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                let _ = status_tx.send(format!("[CLIENT] WebDAV client {} disconnected", client_id));
                let _ = status_tx.send("__UPDATE_UI__".to_string());
            }
            _ => {
                return Err(anyhow::anyhow!("Unexpected action result: {:?}", result));
            }
        }

        Ok(())
    }

    /// Make a WebDAV request
    pub async fn make_request(
        client_id: ClientId,
        method: String,
        path: String,
        headers: Option<Vec<(String, String)>>,
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

        info!("WebDAV client {} making request: {} {}", client_id, method, url);

        // Build request with custom method support for WebDAV
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        let method_upper = method.to_uppercase();
        let request_method = match method_upper.as_str() {
            "GET" => reqwest::Method::GET,
            "PUT" => reqwest::Method::PUT,
            "POST" => reqwest::Method::POST,
            "DELETE" => reqwest::Method::DELETE,
            "HEAD" => reqwest::Method::HEAD,
            "OPTIONS" => reqwest::Method::OPTIONS,
            "PROPFIND" => reqwest::Method::from_bytes(b"PROPFIND")?,
            "PROPPATCH" => reqwest::Method::from_bytes(b"PROPPATCH")?,
            "MKCOL" => reqwest::Method::from_bytes(b"MKCOL")?,
            "COPY" => reqwest::Method::from_bytes(b"COPY")?,
            "MOVE" => reqwest::Method::from_bytes(b"MOVE")?,
            "LOCK" => reqwest::Method::from_bytes(b"LOCK")?,
            "UNLOCK" => reqwest::Method::from_bytes(b"UNLOCK")?,
            _ => return Err(anyhow::anyhow!("Unsupported WebDAV method: {}", method)),
        };

        let mut request = http_client.request(request_method, &url);

        // Add headers
        if let Some(hdrs) = headers {
            for (key, value) in hdrs {
                request = request.header(&key, value);
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

                info!("WebDAV client {} received response: {} ({})", client_id, status_code, status);

                // Call LLM with response
                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                    let protocol = Arc::new(crate::client::webdav::actions::WebdavClientProtocol::new());
                    let event = Event::new(
                        &WEBDAV_CLIENT_RESPONSE_RECEIVED_EVENT,
                        serde_json::json!({
                            "method": method,
                            "status_code": status_code,
                            "status_text": status.to_string(),
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
                            error!("LLM error for WebDAV client {}: {}", client_id, e);
                        }
                    }
                }

                Ok(())
            }
            Err(e) => {
                error!("WebDAV client {} request failed: {}", client_id, e);
                let _ = status_tx.send(format!("[ERROR] WebDAV request failed: {}", e));
                Err(e.into())
            }
        }
    }

    /// Build XML body for PROPFIND request
    pub fn build_propfind_body(properties: Option<Vec<String>>) -> String {
        match properties {
            Some(props) => {
                let mut prop_elements = String::new();
                for prop in props {
                    prop_elements.push_str(&format!("<D:{}/>\n", prop));
                }
                format!(
                    r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:">
  <D:prop>
{}
  </D:prop>
</D:propfind>"#,
                    prop_elements
                )
            }
            None => {
                // Request all properties
                r#"<?xml version="1.0" encoding="utf-8"?>
<D:propfind xmlns:D="DAV:">
  <D:allprop/>
</D:propfind>"#
                    .to_string()
            }
        }
    }
}
