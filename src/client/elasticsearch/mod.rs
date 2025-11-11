//! Elasticsearch client implementation
pub mod actions;

pub use actions::ElasticsearchClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::client::elasticsearch::actions::{
    ELASTICSEARCH_CLIENT_CONNECTED_EVENT, ELASTICSEARCH_CLIENT_RESPONSE_RECEIVED_EVENT,
};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

/// Elasticsearch client that interacts with Elasticsearch clusters
pub struct ElasticsearchClient;

impl ElasticsearchClient {
    /// Connect to an Elasticsearch cluster with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // For Elasticsearch, "connection" is logical (HTTP-based)
        // We'll create a reqwest client configured for Elasticsearch

        info!(
            "Elasticsearch client {} initialized for {}",
            client_id, remote_addr
        );

        // Ensure URL has scheme
        let cluster_url =
            if remote_addr.starts_with("http://") || remote_addr.starts_with("https://") {
                remote_addr.clone()
            } else {
                format!("http://{}", remote_addr)
            };

        // Build HTTP client for Elasticsearch
        let _http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to build HTTP client for Elasticsearch")?;

        // Store client configuration in protocol_data
        app_state
            .with_client_mut(client_id, |client| {
                client
                    .set_protocol_field("es_client".to_string(), serde_json::json!("initialized"));
                client.set_protocol_field(
                    "cluster_url".to_string(),
                    serde_json::json!(cluster_url.clone()),
                );
            })
            .await;

        // Update status
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!(
            "[CLIENT] Elasticsearch client {} ready for {}",
            client_id, cluster_url
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Call LLM with connected event to get initial instructions
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(ElasticsearchClientProtocol::new());
            let event = Event::new(
                &ELASTICSEARCH_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "cluster_url": cluster_url.clone(),
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
                    actions,
                    memory_updates,
                }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }

                    // Execute initial actions
                    use crate::llm::actions::client_trait::{Client, ClientActionResult};
                    for action in actions {
                        match protocol.execute_action(action) {
                            Ok(ClientActionResult::Custom { name, data }) => match name.as_str() {
                                "index_document" => {
                                    let index = data
                                        .get("index")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown")
                                        .to_string();
                                    let id = data
                                        .get("id")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string());
                                    let document = data
                                        .get("document")
                                        .cloned()
                                        .unwrap_or(serde_json::json!({}));

                                    tokio::spawn(Self::index_document(
                                        client_id,
                                        index,
                                        id,
                                        document,
                                        app_state.clone(),
                                        llm_client.clone(),
                                        status_tx.clone(),
                                    ));
                                }
                                "search" => {
                                    let index = data
                                        .get("index")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown")
                                        .to_string();
                                    let query =
                                        data.get("query").cloned().unwrap_or(serde_json::json!({}));

                                    tokio::spawn(Self::search(
                                        client_id,
                                        index,
                                        query,
                                        app_state.clone(),
                                        llm_client.clone(),
                                        status_tx.clone(),
                                    ));
                                }
                                _ => {
                                    info!("Ignoring action {} during initial connection", name);
                                }
                            },
                            Ok(ClientActionResult::NoAction) => {
                                // LLM chose to take no action
                            }
                            Ok(ClientActionResult::Multiple(_)) => {
                                // Multiple actions not supported for initial connection
                                warn!("Multiple actions not supported during initial connection");
                            }
                            Ok(ClientActionResult::Disconnect) => {
                                // Ignore disconnect during initial connection
                            }
                            Ok(ClientActionResult::WaitForMore) => {
                                // Ignore wait for more during initial connection
                            }
                            Ok(ClientActionResult::SendData(_)) => {
                                // Not applicable for Elasticsearch (HTTP-based)
                            }
                            Err(e) => {
                                error!("Failed to execute initial action: {}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(
                        "Initial LLM call failed for Elasticsearch client {}: {}",
                        client_id, e
                    );
                }
            }
        }

        // Spawn background monitoring task
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("Elasticsearch client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return dummy address (Elasticsearch is HTTP-based)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Index a document into Elasticsearch
    pub async fn index_document(
        client_id: ClientId,
        index: String,
        id: Option<String>,
        document: serde_json::Value,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let cluster_url = Self::get_cluster_url(&app_state, client_id).await?;

        let url = if let Some(doc_id) = &id {
            format!("{}/{}/_doc/{}", cluster_url, index, doc_id)
        } else {
            format!("{}/{}/_doc", cluster_url, index)
        };

        info!(
            "Elasticsearch client {} indexing document into {}",
            client_id, index
        );

        let http_client = reqwest::Client::new();
        let response = http_client
            .post(&url)
            .json(&document)
            .send()
            .await
            .context("Failed to send index request")?;

        let status_code = response.status().as_u16();
        let response_body: serde_json::Value = response
            .json()
            .await
            .unwrap_or(serde_json::json!({"error": "Failed to parse response"}));

        info!(
            "Elasticsearch client {} index response: {}",
            client_id, status_code
        );

        Self::call_llm_with_response(
            client_id,
            "index_document".to_string(),
            status_code,
            response_body,
            app_state,
            llm_client,
            status_tx,
        )
        .await
    }

    /// Search documents in Elasticsearch
    pub async fn search(
        client_id: ClientId,
        index: String,
        query: serde_json::Value,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let cluster_url = Self::get_cluster_url(&app_state, client_id).await?;
        let url = format!("{}/{}/_search", cluster_url, index);

        info!(
            "Elasticsearch client {} searching index {}",
            client_id, index
        );

        let search_body = serde_json::json!({
            "query": query
        });

        let http_client = reqwest::Client::new();
        let response = http_client
            .post(&url)
            .json(&search_body)
            .send()
            .await
            .context("Failed to send search request")?;

        let status_code = response.status().as_u16();
        let response_body: serde_json::Value = response
            .json()
            .await
            .unwrap_or(serde_json::json!({"error": "Failed to parse response"}));

        info!(
            "Elasticsearch client {} search response: {} hits",
            client_id,
            response_body
                .get("hits")
                .and_then(|h| h.get("total"))
                .and_then(|t| t.get("value"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
        );

        Self::call_llm_with_response(
            client_id,
            "search".to_string(),
            status_code,
            response_body,
            app_state,
            llm_client,
            status_tx,
        )
        .await
    }

    /// Get a document by ID
    pub async fn get_document(
        client_id: ClientId,
        index: String,
        id: String,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let cluster_url = Self::get_cluster_url(&app_state, client_id).await?;
        let url = format!("{}/{}/_doc/{}", cluster_url, index, id);

        info!(
            "Elasticsearch client {} getting document {} from {}",
            client_id, id, index
        );

        let http_client = reqwest::Client::new();
        let response = http_client
            .get(&url)
            .send()
            .await
            .context("Failed to send get request")?;

        let status_code = response.status().as_u16();
        let response_body: serde_json::Value = response
            .json()
            .await
            .unwrap_or(serde_json::json!({"error": "Failed to parse response"}));

        Self::call_llm_with_response(
            client_id,
            "get_document".to_string(),
            status_code,
            response_body,
            app_state,
            llm_client,
            status_tx,
        )
        .await
    }

    /// Delete a document by ID
    pub async fn delete_document(
        client_id: ClientId,
        index: String,
        id: String,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let cluster_url = Self::get_cluster_url(&app_state, client_id).await?;
        let url = format!("{}/{}/_doc/{}", cluster_url, index, id);

        info!(
            "Elasticsearch client {} deleting document {} from {}",
            client_id, id, index
        );

        let http_client = reqwest::Client::new();
        let response = http_client
            .delete(&url)
            .send()
            .await
            .context("Failed to send delete request")?;

        let status_code = response.status().as_u16();
        let response_body: serde_json::Value = response
            .json()
            .await
            .unwrap_or(serde_json::json!({"error": "Failed to parse response"}));

        Self::call_llm_with_response(
            client_id,
            "delete_document".to_string(),
            status_code,
            response_body,
            app_state,
            llm_client,
            status_tx,
        )
        .await
    }

    /// Execute bulk operations
    pub async fn bulk_operation(
        client_id: ClientId,
        operations: Vec<serde_json::Value>,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let cluster_url = Self::get_cluster_url(&app_state, client_id).await?;
        let url = format!("{}/_bulk", cluster_url);

        info!(
            "Elasticsearch client {} executing {} bulk operations",
            client_id,
            operations.len()
        );

        // Build NDJSON bulk request body
        let mut bulk_body = String::new();
        for op in operations {
            let action = op
                .get("action")
                .and_then(|a| a.as_str())
                .context("Missing 'action' field in bulk operation")?;
            let index = op
                .get("index")
                .and_then(|i| i.as_str())
                .context("Missing 'index' field in bulk operation")?;
            let id = op.get("id").and_then(|i| i.as_str());

            match action {
                "index" => {
                    let mut meta = serde_json::json!({
                        "index": { "_index": index }
                    });
                    if let Some(doc_id) = id {
                        meta["index"]["_id"] = serde_json::json!(doc_id);
                    }
                    bulk_body.push_str(&serde_json::to_string(&meta)?);
                    bulk_body.push('\n');

                    let document = op
                        .get("document")
                        .context("Missing 'document' field for index action")?;
                    bulk_body.push_str(&serde_json::to_string(&document)?);
                    bulk_body.push('\n');
                }
                "delete" => {
                    let doc_id = id.context("Missing 'id' field for delete action")?;
                    let meta = serde_json::json!({
                        "delete": {
                            "_index": index,
                            "_id": doc_id
                        }
                    });
                    bulk_body.push_str(&serde_json::to_string(&meta)?);
                    bulk_body.push('\n');
                }
                "update" => {
                    let doc_id = id.context("Missing 'id' field for update action")?;
                    let meta = serde_json::json!({
                        "update": {
                            "_index": index,
                            "_id": doc_id
                        }
                    });
                    bulk_body.push_str(&serde_json::to_string(&meta)?);
                    bulk_body.push('\n');

                    let document = op
                        .get("document")
                        .context("Missing 'document' field for update action")?;
                    let update_doc = serde_json::json!({ "doc": document });
                    bulk_body.push_str(&serde_json::to_string(&update_doc)?);
                    bulk_body.push('\n');
                }
                _ => return Err(anyhow::anyhow!("Unknown bulk action: {}", action)),
            }
        }

        let http_client = reqwest::Client::new();
        let response = http_client
            .post(&url)
            .header("Content-Type", "application/x-ndjson")
            .body(bulk_body)
            .send()
            .await
            .context("Failed to send bulk request")?;

        let status_code = response.status().as_u16();
        let response_body: serde_json::Value = response
            .json()
            .await
            .unwrap_or(serde_json::json!({"error": "Failed to parse response"}));

        Self::call_llm_with_response(
            client_id,
            "bulk_operation".to_string(),
            status_code,
            response_body,
            app_state,
            llm_client,
            status_tx,
        )
        .await
    }

    /// Helper: Get cluster URL from client state
    async fn get_cluster_url(app_state: &Arc<AppState>, client_id: ClientId) -> Result<String> {
        app_state
            .with_client_mut(client_id, |client| {
                client
                    .get_protocol_field("cluster_url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .await
            .flatten()
            .context("No cluster URL found")
    }

    /// Helper: Call LLM with response
    async fn call_llm_with_response(
        client_id: ClientId,
        operation: String,
        status_code: u16,
        response: serde_json::Value,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(ElasticsearchClientProtocol::new());
            let event = Event::new(
                &ELASTICSEARCH_CLIENT_RESPONSE_RECEIVED_EVENT,
                serde_json::json!({
                    "operation": operation,
                    "status_code": status_code,
                    "response": response,
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
                    // Note: Actions are intentionally not executed here to avoid recursion.
                    // For HTTP-based clients like Elasticsearch, responses don't trigger new operations.
                    // New operations are only triggered by the initial connection or explicit user actions.
                }
                Err(e) => {
                    error!("LLM error for Elasticsearch client {}: {}", client_id, e);
                }
            }
        }

        Ok(())
    }
}
