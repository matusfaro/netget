//! OpenAPI client implementation - spec-driven HTTP requests
pub mod actions;

pub use actions::OpenApiClientProtocol;

use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::client::openapi::actions::{
    OPENAPI_CLIENT_CONNECTED_EVENT, OPENAPI_OPERATION_RESPONSE_EVENT,
};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

#[cfg(feature = "openapi")]
use openapi_rs::model::parse::OpenAPI;

/// OpenAPI client that makes spec-driven requests to HTTP servers
pub struct OpenApiClient;

impl OpenApiClient {
    /// Connect to an OpenAPI server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: serde_json::Value,
    ) -> Result<SocketAddr> {
        info!(
            "OpenAPI client {} initializing for {}",
            client_id, remote_addr
        );

        // Parse startup parameters to get spec
        let spec_yaml = if let Some(spec_str) = startup_params.get("spec").and_then(|v| v.as_str())
        {
            spec_str.to_string()
        } else if let Some(spec_file) = startup_params.get("spec_file").and_then(|v| v.as_str()) {
            // Load spec from file
            std::fs::read_to_string(spec_file)
                .with_context(|| format!("Failed to read spec file: {}", spec_file))?
        } else {
            return Err(anyhow!(
                "OpenAPI client requires 'spec' or 'spec_file' parameter"
            ));
        };

        // Parse OpenAPI spec
        #[cfg(feature = "openapi")]
        let parsed_spec: OpenAPI = serde_yaml::from_str(&spec_yaml)
            .context("Failed to parse OpenAPI spec (YAML)")?;

        #[cfg(not(feature = "openapi"))]
        let parsed_spec = ();

        // Determine base URL (from spec or override)
        #[cfg(feature = "openapi")]
        let base_url = if let Some(override_url) =
            startup_params.get("base_url").and_then(|v| v.as_str())
        {
            override_url.to_string()
        } else if let Some(server) = parsed_spec.servers.first() {
            server.url.clone()
        } else {
            // Default: use remote_addr with http://
            if remote_addr.starts_with("http://") || remote_addr.starts_with("https://") {
                remote_addr.clone()
            } else {
                format!("http://{}", remote_addr)
            }
        };

        #[cfg(not(feature = "openapi"))]
        let base_url = format!("http://{}", remote_addr);

        info!(
            "OpenAPI client {} using base URL: {}",
            client_id, base_url
        );

        // Build reqwest client
        let _http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .use_rustls_tls()
            .build()
            .context("Failed to build HTTP client")?;

        // Store spec and base URL in protocol_data
        app_state
            .with_client_mut(client_id, |client| {
                client.set_protocol_field("spec".to_string(), serde_json::json!(spec_yaml));
                client.set_protocol_field("base_url".to_string(), serde_json::json!(base_url));
                client
                    .set_protocol_field("http_client".to_string(), serde_json::json!("initialized"));
            })
            .await;

        // Update status
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!(
            "[CLIENT] OpenAPI client {} ready for {}",
            client_id, remote_addr
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Extract operation list for LLM
        #[cfg(feature = "openapi")]
        let operations = Self::extract_operations(&parsed_spec);

        #[cfg(not(feature = "openapi"))]
        let operations: Vec<serde_json::Value> = vec![];

        // Call LLM with openapi_client_connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            #[cfg(feature = "openapi")]
            let spec_title = parsed_spec.info.title.clone();
            #[cfg(feature = "openapi")]
            let spec_version = parsed_spec.info.version.clone();

            #[cfg(not(feature = "openapi"))]
            let spec_title = String::new();
            #[cfg(not(feature = "openapi"))]
            let spec_version = String::new();

            let event = Event::new(
                &OPENAPI_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "base_url": base_url.clone(),
                    "spec_title": spec_title,
                    "spec_version": spec_version,
                    "operation_count": operations.len(),
                    "operations": operations,
                }),
            );

            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &String::new(), // No memory yet
                Some(&event),
                &crate::client::openapi::actions::OpenApiClientProtocol,
                &status_tx,
            )
            .await
            {
                Ok(result) => {
                    // Execute actions from LLM response
                    Self::execute_llm_actions(
                        client_id,
                        result,
                        app_state.clone(),
                        llm_client.clone(),
                        status_tx.clone(),
                    )
                    .await;
                }
                Err(e) => {
                    error!("LLM error on openapi_client_connected event: {}", e);
                }
            }
        }

        // Spawn background monitoring task
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("OpenAPI client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return dummy address (HTTP is connectionless)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Extract operation list from OpenAPI spec for LLM
    #[cfg(feature = "openapi")]
    fn extract_operations(spec: &OpenAPI) -> Vec<serde_json::Value> {
        let mut operations = Vec::new();

        for (path, path_item) in &spec.paths {
            // Iterate over HTTP methods in path_item.operations
            for (method_str, operation) in &path_item.operations {
                let operation_id = operation
                    .operation_id
                    .as_ref()
                    .cloned()
                    .unwrap_or_else(|| format!("{}_{}", method_str, path.replace('/', "_")));

                operations.push(serde_json::json!({
                    "operation_id": operation_id,
                    "method": method_str.to_uppercase(),
                    "path": path,
                    "summary": operation.summary.as_ref().unwrap_or(&String::new()),
                    "description": operation.description.as_ref().unwrap_or(&String::new()),
                }));
            }
        }

        operations
    }

    /// Execute actions returned by LLM
    async fn execute_llm_actions(
        client_id: ClientId,
        result: ClientLlmResult,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) {
        use crate::llm::actions::client_trait::{Client, ClientActionResult};
        let protocol = crate::client::openapi::actions::OpenApiClientProtocol;

        for action in result.actions {
            match protocol.execute_action(action.clone()) {
                Ok(ClientActionResult::Custom { name, data }) => {
                    if name == "openapi_operation" {
                        // Execute OpenAPI operation
                        let operation_id = data["operation_id"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string();
                        let path_params = data["path_params"]
                            .as_object()
                            .map(|obj| {
                                obj.iter()
                                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let query_params = data["query_params"]
                            .as_object()
                            .map(|obj| {
                                obj.iter()
                                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let headers = data["headers"].as_object().cloned();
                        let body = data["body"].clone();

                        let llm_clone = llm_client.clone();
                        let state_clone = app_state.clone();
                        let status_clone = status_tx.clone();

                        tokio::spawn(async move {
                            if let Err(e) = Self::execute_operation(
                                client_id,
                                operation_id,
                                path_params,
                                query_params,
                                headers,
                                body,
                                state_clone,
                                llm_clone,
                                status_clone,
                            )
                            .await
                            {
                                error!("OpenAPI operation execution failed: {}", e);
                            }
                        });
                    }
                }
                Ok(ClientActionResult::Disconnect) => {
                    info!("LLM requested disconnect for OpenAPI client {}", client_id);
                    // Client will be cleaned up by monitoring task
                }
                Ok(ClientActionResult::NoAction) => {
                    debug!("LLM returned no action");
                }
                Ok(_) => {
                    debug!("Unhandled action result for OpenAPI client");
                }
                Err(e) => {
                    error!("Action execution error: {}", e);
                }
            }
        }
    }

    /// Execute an OpenAPI operation
    #[allow(clippy::too_many_arguments)]
    async fn execute_operation(
        client_id: ClientId,
        operation_id: String,
        path_params: HashMap<String, String>,
        query_params: HashMap<String, String>,
        header_overrides: Option<serde_json::Map<String, serde_json::Value>>,
        body: serde_json::Value,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get spec and base URL from client
        let (spec_yaml, base_url) = app_state
            .with_client_mut(client_id, |client| {
                let spec = client
                    .get_protocol_field("spec")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let url = client
                    .get_protocol_field("base_url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                (spec, url)
            })
            .await
            .unwrap_or((None, None));

        let spec_yaml = spec_yaml.context("No spec found")?;
        let base_url = base_url.context("No base URL found")?;

        // Parse spec
        #[cfg(feature = "openapi")]
        let parsed_spec: OpenAPI =
            serde_yaml::from_str(&spec_yaml).context("Failed to parse OpenAPI spec")?;

        // Find operation in spec
        #[cfg(feature = "openapi")]
        let (path_template, method) = Self::find_operation(&parsed_spec, &operation_id)?;

        #[cfg(not(feature = "openapi"))]
        let (path_template, method) = (String::from("/"), String::from("GET"));

        // Substitute path parameters
        let path = Self::substitute_path_params(&path_template, &path_params)?;

        // Build full URL
        let url = format!("{}{}", base_url, path);

        info!(
            "OpenAPI client {} executing operation '{}': {} {}",
            client_id, operation_id, method, url
        );

        // Build HTTP client
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .use_rustls_tls()
            .build()?;

        // Build request
        let mut request = match method.to_uppercase().as_str() {
            "GET" => http_client.get(&url),
            "POST" => http_client.post(&url),
            "PUT" => http_client.put(&url),
            "DELETE" => http_client.delete(&url),
            "HEAD" => http_client.head(&url),
            "PATCH" => http_client.patch(&url),
            _ => return Err(anyhow!("Unsupported HTTP method: {}", method)),
        };

        // Add query parameters
        if !query_params.is_empty() {
            request = request.query(&query_params);
        }

        // Add headers
        if let Some(hdrs) = header_overrides {
            for (key, value) in hdrs {
                if let Some(val_str) = value.as_str() {
                    request = request.header(&key, val_str);
                }
            }
        }

        // Add body if not null
        if !body.is_null() {
            request = request.json(&body);
        }

        // Execute request
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
                    "OpenAPI client {} received response for '{}': {} ({})",
                    client_id, operation_id, status_code, status
                );

                // Call LLM with response
                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                    let protocol =
                        Arc::new(crate::client::openapi::actions::OpenApiClientProtocol::new());
                    let event = Event::new(
                        &OPENAPI_OPERATION_RESPONSE_EVENT,
                        serde_json::json!({
                            "operation_id": operation_id,
                            "method": method,
                            "path": path,
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
                            // Note: We don't execute follow-up actions here to avoid recursive async function issues.
                            // The LLM can handle follow-up operations by including them in the original response
                            // or via user commands.
                        }
                        Err(e) => {
                            error!("LLM error for OpenAPI client {}: {}", client_id, e);
                        }
                    }
                }

                Ok(())
            }
            Err(e) => {
                error!(
                    "OpenAPI client {} request failed for '{}': {}",
                    client_id, operation_id, e
                );
                let _ = status_tx.send(format!("[ERROR] OpenAPI request failed: {}", e));
                Err(e.into())
            }
        }
    }

    /// Find operation in spec by operation_id
    #[cfg(feature = "openapi")]
    fn find_operation(
        spec: &OpenAPI,
        operation_id: &str,
    ) -> Result<(String, String)> {
        for (path, path_item) in &spec.paths {
            for (method, operation) in &path_item.operations {
                let op_id = operation
                    .operation_id
                    .as_ref()
                    .map(|s| s.as_str())
                    .unwrap_or("");

                if op_id == operation_id {
                    return Ok((path.clone(), method.clone()));
                }
            }
        }

        Err(anyhow!(
            "Operation '{}' not found in OpenAPI spec",
            operation_id
        ))
    }

    /// Substitute path parameters in template
    fn substitute_path_params(
        template: &str,
        params: &HashMap<String, String>,
    ) -> Result<String> {
        let mut path = template.to_string();

        // Replace {param} with values
        for (key, value) in params {
            let pattern = format!("{{{}}}", key);
            path = path.replace(&pattern, value);
        }

        // Check for unsubstituted parameters
        if path.contains('{') {
            return Err(anyhow!(
                "Missing required path parameters in '{}': {}",
                template,
                path
            ));
        }

        Ok(path)
    }
}
