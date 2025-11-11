//! NPM Registry client implementation
pub mod actions;

pub use actions::NpmClientProtocol;

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
use crate::client::npm::actions::{NPM_CLIENT_PACKAGE_INFO_RECEIVED_EVENT, NPM_CLIENT_SEARCH_RESULTS_RECEIVED_EVENT};

/// NPM Registry client that queries packages
pub struct NpmClient;

impl NpmClient {
    /// Connect to NPM registry with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        _llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // For NPM, "connection" is logical - we're accessing a REST API
        // Default to registry.npmjs.org if not specified
        let registry_url = if remote_addr.starts_with("http://") || remote_addr.starts_with("https://") {
            remote_addr
        } else {
            // Treat as package name or use default registry
            "https://registry.npmjs.org".to_string()
        };

        info!("NPM client {} initialized for {}", client_id, registry_url);

        // Build reqwest client
        let _http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("NetGet NPM Client/1.0")
            .build()
            .context("Failed to build HTTP client")?;

        // Store client in protocol_data
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "npm_client".to_string(),
                serde_json::json!("initialized"),
            );
            client.set_protocol_field(
                "registry_url".to_string(),
                serde_json::json!(registry_url),
            );
        }).await;

        // Update status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] NPM client {} ready for {}", client_id, registry_url));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Spawn background task to monitor for disconnection
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("NPM client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return a dummy local address (NPM is HTTP-based)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Get information about a package
    pub async fn get_package_info(
        client_id: ClientId,
        package_name: String,
        version: String,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get registry URL from client
        let registry_url = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("registry_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No registry URL found")?;

        // Encode package name for URL (handles scoped packages like @types/node)
        let encoded_name = package_name.replace("/", "%2f");

        let url = if version == "latest" {
            format!("{}/{}", registry_url, encoded_name)
        } else {
            format!("{}/{}/{}", registry_url, encoded_name, version)
        };

        info!("NPM client {} getting package info: {} ({})", client_id, package_name, version);

        // Build HTTP client
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("NetGet NPM Client/1.0")
            .build()?;

        // Make request
        match http_client.get(&url).send().await {
            Ok(response) => {
                let status = response.status();

                if !status.is_success() {
                    let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                    error!("NPM client {} failed to get package {}: {} - {}", client_id, package_name, status, error_text);
                    let _ = status_tx.send(format!("[ERROR] NPM request failed: {} - {}", status, error_text));
                    return Err(anyhow::anyhow!("NPM request failed: {}", status));
                }

                // Parse JSON response
                let package_data: serde_json::Value = response.json().await
                    .context("Failed to parse NPM response")?;

                info!("NPM client {} received package info for {}", client_id, package_name);

                // Extract relevant fields
                let description = package_data.get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let dist_tags = package_data.get("dist-tags");
                let latest_version = dist_tags
                    .and_then(|dt| dt.get("latest"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");

                let versions = package_data.get("versions")
                    .and_then(|v| v.as_object())
                    .map(|obj| obj.keys().cloned().collect::<Vec<_>>())
                    .unwrap_or_default();

                let dist = if version == "latest" {
                    package_data.get("dist-tags")
                        .and_then(|dt| dt.get("latest"))
                        .and_then(|lv| {
                            package_data.get("versions")
                                .and_then(|vs| vs.get(lv.as_str().unwrap_or("")))
                        })
                        .and_then(|v| v.get("dist"))
                        .cloned()
                } else {
                    package_data.get("dist").cloned()
                };

                // Call LLM with package info
                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                    let protocol = Arc::new(crate::client::npm::actions::NpmClientProtocol::new());
                    let event = Event::new(
                        &NPM_CLIENT_PACKAGE_INFO_RECEIVED_EVENT,
                        serde_json::json!({
                            "package_name": package_name,
                            "version": if version == "latest" { latest_version } else { version.as_str() },
                            "description": description,
                            "versions": versions,
                            "dist": dist,
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
                            error!("LLM error for NPM client {}: {}", client_id, e);
                        }
                    }
                }

                Ok(())
            }
            Err(e) => {
                error!("NPM client {} request failed: {}", client_id, e);
                let _ = status_tx.send(format!("[ERROR] NPM request failed: {}", e));
                Err(e.into())
            }
        }
    }

    /// Search for packages
    pub async fn search_packages(
        client_id: ClientId,
        query: String,
        limit: u64,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // NPM search API endpoint
        let search_url = "https://registry.npmjs.org/-/v1/search";

        info!("NPM client {} searching for: {} (limit: {})", client_id, query, limit);

        // Build HTTP client
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("NetGet NPM Client/1.0")
            .build()?;

        // Build query parameters
        let url = format!("{}?text={}&size={}", search_url, urlencoding::encode(&query), limit);

        // Make request
        match http_client.get(&url).send().await {
            Ok(response) => {
                let status = response.status();

                if !status.is_success() {
                    let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                    error!("NPM client {} search failed: {} - {}", client_id, status, error_text);
                    let _ = status_tx.send(format!("[ERROR] NPM search failed: {} - {}", status, error_text));
                    return Err(anyhow::anyhow!("NPM search failed: {}", status));
                }

                // Parse JSON response
                let search_data: serde_json::Value = response.json().await
                    .context("Failed to parse NPM search response")?;

                let results = search_data.get("objects")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter().map(|obj| {
                            let package = obj.get("package");
                            serde_json::json!({
                                "name": package.and_then(|p| p.get("name")).and_then(|v| v.as_str()).unwrap_or(""),
                                "version": package.and_then(|p| p.get("version")).and_then(|v| v.as_str()).unwrap_or(""),
                                "description": package.and_then(|p| p.get("description")).and_then(|v| v.as_str()).unwrap_or(""),
                            })
                        }).collect::<Vec<_>>()
                    })
                    .unwrap_or_default();

                let total = search_data.get("total")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(results.len() as u64);

                info!("NPM client {} received {} search results", client_id, results.len());

                // Call LLM with search results
                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                    let protocol = Arc::new(crate::client::npm::actions::NpmClientProtocol::new());
                    let event = Event::new(
                        &NPM_CLIENT_SEARCH_RESULTS_RECEIVED_EVENT,
                        serde_json::json!({
                            "query": query,
                            "results": results,
                            "total": total,
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
                            error!("LLM error for NPM client {}: {}", client_id, e);
                        }
                    }
                }

                Ok(())
            }
            Err(e) => {
                error!("NPM client {} search failed: {}", client_id, e);
                let _ = status_tx.send(format!("[ERROR] NPM search failed: {}", e));
                Err(e.into())
            }
        }
    }

    /// Download package tarball
    pub async fn download_tarball(
        client_id: ClientId,
        package_name: String,
        version: String,
        output_path: String,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // First get package info to find tarball URL
        let registry_url = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("registry_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No registry URL found")?;

        let encoded_name = package_name.replace("/", "%2f");
        let info_url = format!("{}/{}", registry_url, encoded_name);

        info!("NPM client {} downloading tarball for {} ({})", client_id, package_name, version);

        // Build HTTP client
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .user_agent("NetGet NPM Client/1.0")
            .build()?;

        // Get package info
        let package_data: serde_json::Value = http_client.get(&info_url).send().await?
            .json().await
            .context("Failed to get package info")?;

        // Find tarball URL
        let tarball_url = if version == "latest" {
            package_data.get("dist-tags")
                .and_then(|dt| dt.get("latest"))
                .and_then(|lv| {
                    package_data.get("versions")
                        .and_then(|vs| vs.get(lv.as_str().unwrap_or("")))
                })
                .and_then(|v| v.get("dist"))
                .and_then(|d| d.get("tarball"))
                .and_then(|t| t.as_str())
        } else {
            package_data.get("versions")
                .and_then(|vs| vs.get(&version))
                .and_then(|v| v.get("dist"))
                .and_then(|d| d.get("tarball"))
                .and_then(|t| t.as_str())
        }.context("Could not find tarball URL")?;

        info!("NPM client {} downloading from: {}", client_id, tarball_url);

        // Download tarball
        let response = http_client.get(tarball_url).send().await?;
        let bytes = response.bytes().await?;

        // Write to file
        tokio::fs::write(&output_path, bytes).await
            .context("Failed to write tarball")?;

        info!("NPM client {} downloaded tarball to: {}", client_id, output_path);
        let _ = status_tx.send(format!("[CLIENT] NPM tarball downloaded to: {}", output_path));

        Ok(())
    }
}
