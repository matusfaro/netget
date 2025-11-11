//! PyPI (Python Package Index) client implementation
pub mod actions;

pub use actions::PypiClientProtocol;

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
use crate::client::pypi::actions::{
use crate::{console_trace, console_debug, console_info, console_warn, console_error};
    PYPI_PACKAGE_INFO_EVENT, PYPI_SEARCH_RESULTS_EVENT, PYPI_FILE_DOWNLOADED_EVENT,
};

/// PyPI client that interacts with Python Package Index
pub struct PypiClient;

impl PypiClient {
    /// Connect to PyPI with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        _llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        info!("PyPI client {} initialized for {}", client_id, remote_addr);

        // Build reqwest client
        let _http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("NetGet-PyPI-Client/1.0")
            .build()
            .context("Failed to build HTTP client")?;

        // Parse index URL, default to pypi.org
        let index_url = if remote_addr.starts_with("http://") || remote_addr.starts_with("https://") {
            remote_addr.clone()
        } else {
            "https://pypi.org".to_string()
        };

        // Store client data
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "pypi_client".to_string(),
                serde_json::json!("initialized"),
            );
            client.set_protocol_field(
                "index_url".to_string(),
                serde_json::json!(index_url),
            );
        }).await;

        // Update status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        console_info!(status_tx, "[CLIENT] PyPI client {} ready for {}", client_id, index_url);
        console_info!(status_tx, "__UPDATE_UI__");

        // Spawn background task to monitor client lifecycle
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("PyPI client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return a dummy local address (PyPI is connectionless HTTP)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Get package information from PyPI
    pub async fn get_package_info(
        client_id: ClientId,
        package_name: String,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let index_url = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("index_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No index URL found")?;

        let url = format!("{}/pypi/{}/json", index_url, package_name);

        info!("PyPI client {} fetching package info: {}", client_id, package_name);

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("NetGet-PyPI-Client/1.0")
            .build()?;

        match http_client.get(&url).send().await {
            Ok(response) => {
                if !response.status().is_success() {
                    let status = response.status();
                    console_error!(status_tx, "[ERROR] Package not found or error: {}", status);
                    return Err(anyhow::anyhow!("Package not found: {}", status));
                }

                let json: serde_json::Value = response.json().await?;

                info!("PyPI client {} received package info for {}", client_id, package_name);

                // Call LLM with package info
                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                    let protocol = Arc::new(crate::client::pypi::actions::PypiClientProtocol::new());
                    let event = Event::new(
                        &PYPI_PACKAGE_INFO_EVENT,
                        serde_json::json!({
                            "package_name": package_name,
                            "info": json,
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
                            if let Some(mem) = memory_updates {
                                app_state.set_memory_for_client(client_id, mem).await;
                            }
                        }
                        Err(e) => {
                            error!("LLM error for PyPI client {}: {}", client_id, e);
                        }
                    }
                }

                Ok(())
            }
            Err(e) => {
                console_error!(status_tx, "[ERROR] PyPI request failed: {}", e);
                Err(e.into())
            }
        }
    }

    /// Search for packages on PyPI
    pub async fn search_packages(
        client_id: ClientId,
        query: String,
        _limit: u64,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Note: PyPI deprecated their XML-RPC search API
        // We'll use the warehouse JSON API endpoint (unofficial but commonly used)
        let url = format!("https://pypi.org/search/?q={}", urlencoding::encode(&query));


        console_info!(status_tx, "[INFO] Searching PyPI for: {}", query);

        // Since PyPI's search is HTML-based now, we'll return a simplified result
        // In production, you might want to use a proper search API or scrape the HTML
        let results = serde_json::json!({
            "message": "PyPI search API is deprecated. Use 'get_package_info' for specific packages.",
            "query": query,
            "search_url": url,
            "suggestion": "Try using package names directly with get_package_info action",
        });

        // Call LLM with search results
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::pypi::actions::PypiClientProtocol::new());
            let event = Event::new(
                &PYPI_SEARCH_RESULTS_EVENT,
                serde_json::json!({
                    "query": query,
                    "results": results,
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
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }
                }
                Err(e) => {
                    error!("LLM error for PyPI client {}: {}", client_id, e);
                }
            }
        }

        Ok(())
    }

    /// Download a package file from PyPI
    pub async fn download_package(
        client_id: ClientId,
        package_name: String,
        version: Option<String>,
        filename: Option<String>,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let index_url = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("index_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No index URL found")?;

        // First, get package info to find download URLs
        let info_url = format!("{}/pypi/{}/json", index_url, package_name);

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("NetGet-PyPI-Client/1.0")
            .build()?;

        let json: serde_json::Value = http_client.get(&info_url).send().await?.json().await?;

        // Get the appropriate version
        let target_version = version.unwrap_or_else(|| {
            json["info"]["version"].as_str().unwrap_or("").to_string()
        });

        // Get URLs for this version
        let urls = json["urls"].as_array().context("No URLs found")?;

        // Find the file to download
        let file_info = if let Some(fname) = filename {
            urls.iter().find(|u| u["filename"].as_str() == Some(&fname))
        } else {
            // Default to first wheel, or first sdist
            urls.iter().find(|u| u["packagetype"].as_str() == Some("bdist_wheel"))
                .or_else(|| urls.iter().find(|u| u["packagetype"].as_str() == Some("sdist")))
        }.context("No suitable file found")?;

        let download_url = file_info["url"].as_str().context("No download URL")?;
        let file_name = file_info["filename"].as_str().context("No filename")?;

        console_info!(status_tx, "[INFO] Downloading: {}", file_name);

        // Download the file
        let response = http_client.get(download_url).send().await?;
        let bytes = response.bytes().await?;

        info!("PyPI client {} downloaded {} ({} bytes)", client_id, file_name, bytes.len());

        // Call LLM with download result
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::pypi::actions::PypiClientProtocol::new());
            let event = Event::new(
                &PYPI_FILE_DOWNLOADED_EVENT,
                serde_json::json!({
                    "filename": file_name,
                    "size": bytes.len(),
                    "package": package_name,
                    "version": target_version,
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
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }
                }
                Err(e) => {
                    error!("LLM error for PyPI client {}: {}", client_id, e);
                }
            }
        }

        Ok(())
    }

    /// List available files for a package version
    pub async fn list_package_files(
        client_id: ClientId,
        package_name: String,
        version: Option<String>,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let index_url = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("index_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No index URL found")?;

        let info_url = format!("{}/pypi/{}/json", index_url, package_name);

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("NetGet-PyPI-Client/1.0")
            .build()?;

        let json: serde_json::Value = http_client.get(&info_url).send().await?.json().await?;

        let urls = json["urls"].as_array().context("No URLs found")?;

        let files: Vec<serde_json::Value> = urls.iter().map(|u| {
            serde_json::json!({
                "filename": u["filename"],
                "packagetype": u["packagetype"],
                "size": u["size"],
                "python_version": u["python_version"],
                "url": u["url"],
            })
        }).collect();

        info!("PyPI client {} listed {} files for {}", client_id, files.len(), package_name);

        // Call LLM with file list
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::pypi::actions::PypiClientProtocol::new());
            let event = Event::new(
                &PYPI_PACKAGE_INFO_EVENT,
                serde_json::json!({
                    "package_name": package_name,
                    "info": {
                        "files": files,
                        "version": version.unwrap_or_else(|| json["info"]["version"].as_str().unwrap_or("").to_string()),
                    },
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
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }
                }
                Err(e) => {
                    error!("LLM error for PyPI client {}: {}", client_id, e);
                }
            }
        }

        Ok(())
    }
}
