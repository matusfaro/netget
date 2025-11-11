//! Maven client implementation
pub mod actions;

pub use actions::MavenClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::maven::actions::MAVEN_CLIENT_CONNECTED_EVENT;

/// Maven client that interacts with Maven repositories
pub struct MavenClient;

impl MavenClient {
    /// Execute a Maven custom action
    fn execute_maven_action(
        client_id: ClientId,
        name: String,
        data: serde_json::Value,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) {
        match name.as_str() {
            "maven_download_artifact" => {
                let group_id = data["group_id"].as_str().unwrap_or_default().to_string();
                let artifact_id = data["artifact_id"].as_str().unwrap_or_default().to_string();
                let version = data["version"].as_str().unwrap_or_default().to_string();
                let packaging = data["packaging"].as_str().map(|s| s.to_string());

                tokio::spawn(async move {
                    if let Err(e) = Self::download_artifact(
                        client_id,
                        group_id,
                        artifact_id,
                        version,
                        packaging,
                        app_state,
                        llm_client,
                        status_tx,
                    ).await {
                        error!("Maven artifact download failed: {}", e);
                    }
                });
            }
            "maven_download_pom" => {
                let group_id = data["group_id"].as_str().unwrap_or_default().to_string();
                let artifact_id = data["artifact_id"].as_str().unwrap_or_default().to_string();
                let version = data["version"].as_str().unwrap_or_default().to_string();

                tokio::spawn(async move {
                    if let Err(e) = Self::download_pom(
                        client_id,
                        group_id,
                        artifact_id,
                        version,
                        app_state,
                        llm_client,
                        status_tx,
                    ).await {
                        error!("Maven POM download failed: {}", e);
                    }
                });
            }
            "maven_search_versions" => {
                let group_id = data["group_id"].as_str().unwrap_or_default().to_string();
                let artifact_id = data["artifact_id"].as_str().unwrap_or_default().to_string();

                tokio::spawn(async move {
                    if let Err(e) = Self::search_versions(
                        client_id,
                        group_id,
                        artifact_id,
                        app_state,
                        llm_client,
                        status_tx,
                    ).await {
                        error!("Maven version search failed: {}", e);
                    }
                });
            }
            _ => {
                warn!("Unknown Maven custom action: {}", name);
            }
        }
    }

    /// Connect to a Maven repository with integrated LLM actions
    pub async fn connect_with_llm_actions(
        repository_url: String,
        _llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // For Maven, "connection" is logical - we interact with HTTP-based repositories
        // Default to Maven Central if no specific URL provided
        let repo_url = if repository_url.is_empty() || repository_url == "maven" || repository_url == "maven-central" {
            "https://repo.maven.apache.org/maven2".to_string()
        } else {
            repository_url
        };

        info!("Maven client {} initialized for repository: {}", client_id, repo_url);

        // Store client in protocol_data
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "http_client".to_string(),
                serde_json::json!("initialized"),
            );
            client.set_protocol_field(
                "repository_url".to_string(),
                serde_json::json!(repo_url.clone()),
            );
        }).await;

        // Update status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] Maven client {} connected to repository: {}", client_id, repo_url));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::maven::actions::MavenClientProtocol::new());
            let event = Event::new(
                &MAVEN_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "repository_url": repo_url,
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
                        use crate::llm::actions::client_trait::Client;
                        match protocol.as_ref().execute_action(action) {
                            Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data }) => {
                                match name.as_str() {
                                    "maven_download_artifact" => {
                                        let group_id = data["group_id"].as_str().unwrap_or_default().to_string();
                                        let artifact_id = data["artifact_id"].as_str().unwrap_or_default().to_string();
                                        let version = data["version"].as_str().unwrap_or_default().to_string();
                                        let packaging = data["packaging"].as_str().map(|s| s.to_string());

                                        let app_state_clone = app_state.clone();
                                        let llm_client_clone = _llm_client.clone();
                                        let status_tx_clone = status_tx.clone();

                                        tokio::spawn(async move {
                                            if let Err(e) = Self::download_artifact(
                                                client_id,
                                                group_id,
                                                artifact_id,
                                                version,
                                                packaging,
                                                app_state_clone,
                                                llm_client_clone,
                                                status_tx_clone,
                                            ).await {
                                                error!("Maven artifact download failed: {}", e);
                                            }
                                        });
                                    }
                                    "maven_download_pom" => {
                                        let group_id = data["group_id"].as_str().unwrap_or_default().to_string();
                                        let artifact_id = data["artifact_id"].as_str().unwrap_or_default().to_string();
                                        let version = data["version"].as_str().unwrap_or_default().to_string();

                                        let app_state_clone = app_state.clone();
                                        let llm_client_clone = _llm_client.clone();
                                        let status_tx_clone = status_tx.clone();

                                        tokio::spawn(async move {
                                            if let Err(e) = Self::download_pom(
                                                client_id,
                                                group_id,
                                                artifact_id,
                                                version,
                                                app_state_clone,
                                                llm_client_clone,
                                                status_tx_clone,
                                            ).await {
                                                error!("Maven POM download failed: {}", e);
                                            }
                                        });
                                    }
                                    "maven_search_versions" => {
                                        let group_id = data["group_id"].as_str().unwrap_or_default().to_string();
                                        let artifact_id = data["artifact_id"].as_str().unwrap_or_default().to_string();

                                        let app_state_clone = app_state.clone();
                                        let llm_client_clone = _llm_client.clone();
                                        let status_tx_clone = status_tx.clone();

                                        tokio::spawn(async move {
                                            if let Err(e) = Self::search_versions(
                                                client_id,
                                                group_id,
                                                artifact_id,
                                                app_state_clone,
                                                llm_client_clone,
                                                status_tx_clone,
                                            ).await {
                                                error!("Maven version search failed: {}", e);
                                            }
                                        });
                                    }
                                    _ => {
                                        warn!("Unknown Maven custom action: {}", name);
                                    }
                                }
                            }
                            Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                                info!("Maven client {} disconnecting", client_id);
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error for Maven client {}: {}", client_id, e);
                }
            }
        }

        // Spawn background task to monitor client status
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("Maven client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return a dummy local address (Maven is HTTP-based)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Construct Maven artifact URL
    ///
    /// Converts Maven coordinates (groupId:artifactId:version) into repository URL
    /// Example: org.apache.commons:commons-lang3:3.12.0 ->
    ///   https://repo.maven.apache.org/maven2/org/apache/commons/commons-lang3/3.12.0/commons-lang3-3.12.0.jar
    pub fn artifact_url(
        repository_url: &str,
        group_id: &str,
        artifact_id: &str,
        version: &str,
        packaging: &str,
    ) -> String {
        let group_path = group_id.replace('.', "/");
        format!(
            "{}/{}/{}/{}/{}-{}.{}",
            repository_url.trim_end_matches('/'),
            group_path,
            artifact_id,
            version,
            artifact_id,
            version,
            packaging
        )
    }

    /// Construct POM URL
    pub fn pom_url(
        repository_url: &str,
        group_id: &str,
        artifact_id: &str,
        version: &str,
    ) -> String {
        Self::artifact_url(repository_url, group_id, artifact_id, version, "pom")
    }

    /// Construct metadata URL
    pub fn metadata_url(
        repository_url: &str,
        group_id: &str,
        artifact_id: &str,
    ) -> String {
        let group_path = group_id.replace('.', "/");
        format!(
            "{}/{}/{}/maven-metadata.xml",
            repository_url.trim_end_matches('/'),
            group_path,
            artifact_id
        )
    }

    /// Download artifact from Maven repository
    pub async fn download_artifact(
        client_id: ClientId,
        group_id: String,
        artifact_id: String,
        version: String,
        packaging: Option<String>,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let packaging = packaging.unwrap_or_else(|| "jar".to_string());

        // Get repository URL from client
        let repository_url = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("repository_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No repository URL found")?;

        let artifact_url = Self::artifact_url(
            &repository_url,
            &group_id,
            &artifact_id,
            &version,
            &packaging,
        );

        info!(
            "Maven client {} downloading artifact: {}:{}:{}",
            client_id, group_id, artifact_id, version
        );
        let _ = status_tx.send(format!(
            "[CLIENT] Downloading artifact from: {}",
            artifact_url
        ));

        // Build HTTP client
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("NetGet-Maven/1.0")
            .build()?;

        // Download artifact
        match http_client.get(&artifact_url).send().await {
            Ok(response) => {
                let status = response.status();
                let status_code = status.as_u16();

                if status.is_success() {
                    let content_length = response.content_length().unwrap_or(0);
                    let body_bytes = response.bytes().await.unwrap_or_default();

                    info!(
                        "Maven client {} artifact downloaded: {} bytes",
                        client_id,
                        body_bytes.len()
                    );

                    // Call LLM with artifact download result
                    if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                        let protocol = Arc::new(crate::client::maven::actions::MavenClientProtocol::new());
                        let event = Event::new(
                            &crate::client::maven::actions::MAVEN_CLIENT_ARTIFACT_DOWNLOADED_EVENT,
                            serde_json::json!({
                                "group_id": group_id,
                                "artifact_id": artifact_id,
                                "version": version,
                                "packaging": packaging,
                                "url": artifact_url,
                                "size_bytes": body_bytes.len(),
                                "content_length": content_length,
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
                            Ok(ClientLlmResult { actions, memory_updates }) => {
                                if let Some(mem) = memory_updates {
                                    app_state.set_memory_for_client(client_id, mem).await;
                                }

                                // Execute actions from LLM response
                                for action in actions {
                                    use crate::llm::actions::client_trait::Client;
                                    match protocol.as_ref().execute_action(action) {
                                        Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data }) => {
                                            Self::execute_maven_action(
                                                client_id,
                                                name,
                                                data,
                                                app_state.clone(),
                                                llm_client.clone(),
                                                status_tx.clone(),
                                            );
                                        }
                                        Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                                            info!("Maven client {} disconnecting", client_id);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            Err(e) => {
                                error!("LLM error for Maven client {}: {}", client_id, e);
                            }
                        }
                    }

                    Ok(())
                } else {
                    let error_msg = format!("Artifact not found: HTTP {}", status_code);
                    error!("Maven client {} error: {}", client_id, error_msg);
                    let _ = status_tx.send(format!("[ERROR] {}", error_msg));
                    Err(anyhow::anyhow!(error_msg))
                }
            }
            Err(e) => {
                error!("Maven client {} download failed: {}", client_id, e);
                let _ = status_tx.send(format!("[ERROR] Download failed: {}", e));
                Err(e.into())
            }
        }
    }

    /// Download and parse POM file
    pub async fn download_pom(
        client_id: ClientId,
        group_id: String,
        artifact_id: String,
        version: String,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get repository URL from client
        let repository_url = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("repository_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No repository URL found")?;

        let pom_url = Self::pom_url(
            &repository_url,
            &group_id,
            &artifact_id,
            &version,
        );

        info!(
            "Maven client {} downloading POM: {}:{}:{}",
            client_id, group_id, artifact_id, version
        );
        let _ = status_tx.send(format!(
            "[CLIENT] Downloading POM from: {}",
            pom_url
        ));

        // Build HTTP client
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("NetGet-Maven/1.0")
            .build()?;

        // Download POM
        match http_client.get(&pom_url).send().await {
            Ok(response) => {
                let status = response.status();
                let status_code = status.as_u16();

                if status.is_success() {
                    let pom_content = response.text().await.unwrap_or_default();

                    info!(
                        "Maven client {} POM downloaded: {} bytes",
                        client_id,
                        pom_content.len()
                    );

                    // Call LLM with POM content
                    if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                        let protocol = Arc::new(crate::client::maven::actions::MavenClientProtocol::new());
                        let event = Event::new(
                            &crate::client::maven::actions::MAVEN_CLIENT_POM_RECEIVED_EVENT,
                            serde_json::json!({
                                "group_id": group_id,
                                "artifact_id": artifact_id,
                                "version": version,
                                "url": pom_url,
                                "pom_content": pom_content,
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
                            Ok(ClientLlmResult { actions, memory_updates }) => {
                                if let Some(mem) = memory_updates {
                                    app_state.set_memory_for_client(client_id, mem).await;
                                }

                                // Execute actions from LLM response
                                for action in actions {
                                    use crate::llm::actions::client_trait::Client;
                                    match protocol.as_ref().execute_action(action) {
                                        Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data }) => {
                                            Self::execute_maven_action(
                                                client_id,
                                                name,
                                                data,
                                                app_state.clone(),
                                                llm_client.clone(),
                                                status_tx.clone(),
                                            );
                                        }
                                        Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                                            info!("Maven client {} disconnecting", client_id);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            Err(e) => {
                                error!("LLM error for Maven client {}: {}", client_id, e);
                            }
                        }
                    }

                    Ok(())
                } else {
                    let error_msg = format!("POM not found: HTTP {}", status_code);
                    error!("Maven client {} error: {}", client_id, error_msg);
                    let _ = status_tx.send(format!("[ERROR] {}", error_msg));
                    Err(anyhow::anyhow!(error_msg))
                }
            }
            Err(e) => {
                error!("Maven client {} POM download failed: {}", client_id, e);
                let _ = status_tx.send(format!("[ERROR] POM download failed: {}", e));
                Err(e.into())
            }
        }
    }

    /// Search for artifact versions (via maven-metadata.xml)
    pub async fn search_versions(
        client_id: ClientId,
        group_id: String,
        artifact_id: String,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get repository URL from client
        let repository_url = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("repository_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No repository URL found")?;

        let metadata_url = Self::metadata_url(
            &repository_url,
            &group_id,
            &artifact_id,
        );

        info!(
            "Maven client {} searching versions: {}:{}",
            client_id, group_id, artifact_id
        );
        let _ = status_tx.send(format!(
            "[CLIENT] Fetching metadata from: {}",
            metadata_url
        ));

        // Build HTTP client
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("NetGet-Maven/1.0")
            .build()?;

        // Download metadata
        match http_client.get(&metadata_url).send().await {
            Ok(response) => {
                let status = response.status();
                let status_code = status.as_u16();

                if status.is_success() {
                    let metadata_content = response.text().await.unwrap_or_default();

                    info!(
                        "Maven client {} metadata received: {} bytes",
                        client_id,
                        metadata_content.len()
                    );

                    // Call LLM with metadata content
                    if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                        let protocol = Arc::new(crate::client::maven::actions::MavenClientProtocol::new());
                        let event = Event::new(
                            &crate::client::maven::actions::MAVEN_CLIENT_METADATA_RECEIVED_EVENT,
                            serde_json::json!({
                                "group_id": group_id,
                                "artifact_id": artifact_id,
                                "url": metadata_url,
                                "metadata_content": metadata_content,
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
                            Ok(ClientLlmResult { actions, memory_updates }) => {
                                if let Some(mem) = memory_updates {
                                    app_state.set_memory_for_client(client_id, mem).await;
                                }

                                // Execute actions from LLM response
                                for action in actions {
                                    use crate::llm::actions::client_trait::Client;
                                    match protocol.as_ref().execute_action(action) {
                                        Ok(crate::llm::actions::client_trait::ClientActionResult::Custom { name, data }) => {
                                            Self::execute_maven_action(
                                                client_id,
                                                name,
                                                data,
                                                app_state.clone(),
                                                llm_client.clone(),
                                                status_tx.clone(),
                                            );
                                        }
                                        Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                                            info!("Maven client {} disconnecting", client_id);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            Err(e) => {
                                error!("LLM error for Maven client {}: {}", client_id, e);
                            }
                        }
                    }

                    Ok(())
                } else {
                    let error_msg = format!("Metadata not found: HTTP {}", status_code);
                    error!("Maven client {} error: {}", client_id, error_msg);
                    let _ = status_tx.send(format!("[ERROR] {}", error_msg));
                    Err(anyhow::anyhow!(error_msg))
                }
            }
            Err(e) => {
                error!("Maven client {} metadata fetch failed: {}", client_id, e);
                let _ = status_tx.send(format!("[ERROR] Metadata fetch failed: {}", e));
                Err(e.into())
            }
        }
    }
}
