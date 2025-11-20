//! MongoDB client implementation
pub mod actions;

pub use actions::MongodbClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, trace};

#[cfg(feature = "mongodb")]
use mongodb::{
    bson::{doc, Document},
    options::{ClientOptions, FindOptions, UpdateOptions},
    Client as MongoClient, Database,
};

use crate::client::mongodb::actions::{
    MONGODB_CLIENT_CONNECTED_EVENT, MONGODB_CLIENT_RESULT_RECEIVED_EVENT,
};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::Client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

/// MongoDB client that connects to a MongoDB server
pub struct MongodbClient;

impl MongodbClient {
    /// Connect to a MongoDB server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        Self::connect_impl(
            remote_addr,
            llm_client,
            app_state,
            status_tx,
            client_id,
            startup_params,
        )
        .await
    }

    #[cfg(feature = "mongodb")]
    async fn connect_impl(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        // Parse startup parameters
        let database_name = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("database"))
            .unwrap_or_else(|| "admin".to_string());

        let username = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("username"));

        let password = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("password"));

        // Build MongoDB connection string
        let connection_string = if let (Some(user), Some(pass)) = (username, password) {
            format!("mongodb://{}:{}@{}", user, pass, remote_addr)
        } else {
            format!("mongodb://{}", remote_addr)
        };

        // Parse connection options
        let client_options = ClientOptions::parse(&connection_string)
            .await
            .context(format!(
                "Failed to parse MongoDB connection string for {}",
                remote_addr
            ))?;

        // Connect to MongoDB server
        let mongo_client = MongoClient::with_options(client_options)
            .context(format!("Failed to connect to MongoDB at {}", remote_addr))?;

        info!("MongoDB client {} connected to {}", client_id, remote_addr);

        // Get database
        let db = mongo_client.database(&database_name);

        // Parse socket address (MongoDB connection string to SocketAddr)
        let socket_addr: SocketAddr = if remote_addr.contains(':') {
            remote_addr.parse().context("Failed to parse socket address")?
        } else {
            format!("{}:27017", remote_addr)
                .parse()
                .context("Failed to parse socket address")?
        };

        // Update client state
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!(
            "[CLIENT] MongoDB client {} connected to {}",
            client_id, remote_addr
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Wrap database in Arc for shared access
        let db_arc = Arc::new(db);

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::mongodb::actions::MongodbClientProtocol::new());
            let event = Event::new(
                &MONGODB_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "remote_addr": remote_addr,
                    "database": database_name,
                }),
            );

            let memory = app_state
                .get_memory_for_client(client_id)
                .await
                .unwrap_or_default();

            let db_clone = db_arc.clone();
            let app_state_clone = app_state.clone();
            let status_tx_clone = status_tx.clone();

            tokio::spawn(async move {
                match call_llm_for_client(
                    &llm_client,
                    &app_state_clone,
                    client_id.to_string(),
                    &instruction,
                    &memory,
                    Some(&event),
                    protocol.as_ref(),
                    &status_tx_clone,
                )
                .await
                {
                    Ok(ClientLlmResult {
                        actions,
                        memory_updates,
                    }) => {
                        // Update memory
                        if let Some(mem) = memory_updates {
                            app_state_clone.set_memory_for_client(client_id, mem).await;
                        }

                        // Execute actions
                        for action in actions {
                            if let Err(e) = Self::execute_llm_action(
                                client_id,
                                action,
                                &protocol,
                                &db_clone,
                                &app_state_clone,
                                &llm_client,
                                &status_tx_clone,
                            )
                            .await
                            {
                                error!("Error executing MongoDB action: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("LLM error for MongoDB client {}: {}", client_id, e);
                    }
                }
            });
        }

        Ok(socket_addr)
    }

    #[cfg(not(feature = "mongodb"))]
    async fn connect_impl(
        _remote_addr: String,
        _llm_client: OllamaClient,
        _app_state: Arc<AppState>,
        _status_tx: mpsc::UnboundedSender<String>,
        _client_id: ClientId,
        _startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        Err(anyhow::anyhow!("MongoDB client feature not enabled"))
    }

    /// Execute an action from the LLM
    #[cfg(feature = "mongodb")]
    async fn execute_llm_action(
        client_id: ClientId,
        action: serde_json::Value,
        protocol: &Arc<MongodbClientProtocol>,
        db: &Arc<Database>,
        app_state: &Arc<AppState>,
        llm_client: &OllamaClient,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        use crate::llm::actions::client_trait::ClientActionResult;

        match protocol.execute_action(action)? {
            ClientActionResult::Custom { name, data } if name == "mongodb_find" => {
                let collection_name = data
                    .get("collection")
                    .and_then(|v| v.as_str())
                    .context("Missing collection")?;
                let filter_json = data.get("filter").cloned().unwrap_or(serde_json::json!({}));
                let projection_json = data.get("projection").cloned();
                let limit = data.get("limit").and_then(|v| v.as_i64());

                trace!(
                    "MongoDB client {} finding in collection: {}",
                    client_id,
                    collection_name
                );

                let collection = db.collection::<Document>(collection_name);

                // Convert JSON filter to BSON document
                let filter = bson::to_document(&filter_json)
                    .context("Failed to convert filter to BSON")?;

                // Build find options
                let mut find_options = FindOptions::default();
                if let Some(proj_json) = projection_json {
                    let projection = bson::to_document(&proj_json).ok();
                    find_options.projection = projection;
                }
                if let Some(lim) = limit {
                    find_options.limit = Some(lim);
                }

                // Execute find
                let cursor = collection
                    .find(filter)
                    .with_options(find_options)
                    .await
                    .context("Failed to execute find")?;

                // Collect results
                use futures::stream::StreamExt;
                let documents: Vec<Document> = cursor
                    .collect::<Vec<Result<Document, mongodb::error::Error>>>()
                    .await
                    .into_iter()
                    .filter_map(|r| r.ok())
                    .collect();

                let _ = status_tx.send(format!(
                    "[MongoDB] Found {} documents in {}",
                    documents.len(),
                    collection_name
                ));

                // Send result event to LLM
                Self::send_result_event(
                    client_id,
                    "find",
                    Some(documents.clone()),
                    None,
                    protocol,
                    app_state,
                    llm_client,
                    status_tx,
                )
                .await?;
            }
            ClientActionResult::Custom { name, data } if name == "mongodb_insert" => {
                let collection_name = data
                    .get("collection")
                    .and_then(|v| v.as_str())
                    .context("Missing collection")?;
                let doc_json = data.get("document").context("Missing document")?;

                trace!(
                    "MongoDB client {} inserting into collection: {}",
                    client_id,
                    collection_name
                );

                let collection = db.collection::<Document>(collection_name);

                // Convert JSON to BSON document
                let document = bson::to_document(doc_json)
                    .context("Failed to convert document to BSON")?;

                // Execute insert
                let result = collection
                    .insert_one(document)
                    .await
                    .context("Failed to insert document")?;

                let _ = status_tx.send(format!(
                    "[MongoDB] Inserted document into {} (id: {:?})",
                    collection_name, result.inserted_id
                ));

                // Send result event to LLM
                Self::send_result_event(
                    client_id,
                    "insert",
                    None,
                    Some(1),
                    protocol,
                    app_state,
                    llm_client,
                    status_tx,
                )
                .await?;
            }
            ClientActionResult::Custom { name, data } if name == "mongodb_update" => {
                let collection_name = data
                    .get("collection")
                    .and_then(|v| v.as_str())
                    .context("Missing collection")?;
                let filter_json = data.get("filter").context("Missing filter")?;
                let update_json = data.get("update").context("Missing update")?;

                trace!(
                    "MongoDB client {} updating collection: {}",
                    client_id,
                    collection_name
                );

                let collection = db.collection::<Document>(collection_name);

                // Convert JSON to BSON documents
                let filter = bson::to_document(filter_json)
                    .context("Failed to convert filter to BSON")?;
                let update = bson::to_document(update_json)
                    .context("Failed to convert update to BSON")?;

                // Execute update
                let result = collection
                    .update_many(filter, update)
                    .await
                    .context("Failed to update documents")?;

                let _ = status_tx.send(format!(
                    "[MongoDB] Updated {} documents in {}",
                    result.modified_count, collection_name
                ));

                // Send result event to LLM
                Self::send_result_event(
                    client_id,
                    "update",
                    None,
                    Some(result.modified_count as u64),
                    protocol,
                    app_state,
                    llm_client,
                    status_tx,
                )
                .await?;
            }
            ClientActionResult::Custom { name, data } if name == "mongodb_delete" => {
                let collection_name = data
                    .get("collection")
                    .and_then(|v| v.as_str())
                    .context("Missing collection")?;
                let filter_json = data.get("filter").context("Missing filter")?;

                trace!(
                    "MongoDB client {} deleting from collection: {}",
                    client_id,
                    collection_name
                );

                let collection = db.collection::<Document>(collection_name);

                // Convert JSON filter to BSON document
                let filter = bson::to_document(filter_json)
                    .context("Failed to convert filter to BSON")?;

                // Execute delete
                let result = collection
                    .delete_many(filter)
                    .await
                    .context("Failed to delete documents")?;

                let _ = status_tx.send(format!(
                    "[MongoDB] Deleted {} documents from {}",
                    result.deleted_count, collection_name
                ));

                // Send result event to LLM
                Self::send_result_event(
                    client_id,
                    "delete",
                    None,
                    Some(result.deleted_count),
                    protocol,
                    app_state,
                    llm_client,
                    status_tx,
                )
                .await?;
            }
            ClientActionResult::Disconnect => {
                info!("MongoDB client {} disconnecting", client_id);
                app_state
                    .update_client_status(client_id, ClientStatus::Disconnected)
                    .await;
            }
            ClientActionResult::WaitForMore => {
                trace!("MongoDB client {} waiting for more input", client_id);
            }
            _ => {
                trace!("MongoDB client {} unhandled action result", client_id);
            }
        }

        Ok(())
    }

    #[cfg(not(feature = "mongodb"))]
    async fn execute_llm_action(
        _client_id: ClientId,
        _action: serde_json::Value,
        _protocol: &Arc<MongodbClientProtocol>,
        _db: &Arc<()>,
        _app_state: &Arc<AppState>,
        _llm_client: &OllamaClient,
        _status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        Err(anyhow::anyhow!("MongoDB client feature not enabled"))
    }

    /// Send result event to LLM
    #[cfg(feature = "mongodb")]
    async fn send_result_event(
        client_id: ClientId,
        result_type: &str,
        documents: Option<Vec<Document>>,
        count: Option<u64>,
        protocol: &Arc<MongodbClientProtocol>,
        app_state: &Arc<AppState>,
        llm_client: &OllamaClient,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        use crate::llm::actions::client_trait::Client;

        let mut event_data = serde_json::json!({
            "result_type": result_type,
        });

        if let Some(docs) = documents {
            // Convert BSON documents to JSON
            let json_docs: Vec<serde_json::Value> = docs
                .iter()
                .filter_map(|doc| bson::to_bson(doc).ok())
                .filter_map(|bson| bson.into_canonical_extjson().as_document().cloned())
                .filter_map(|doc| bson::from_document(doc).ok())
                .collect();
            event_data["documents"] = serde_json::json!(json_docs);
        }

        if let Some(c) = count {
            event_data["count"] = serde_json::json!(c);
        }

        let event = Event::new(&MONGODB_CLIENT_RESULT_RECEIVED_EVENT, event_data);

        let memory = app_state
            .get_memory_for_client(client_id)
            .await
            .unwrap_or_default();
        let instruction = app_state
            .get_instruction_for_client(client_id)
            .await
            .unwrap_or_default();

        match call_llm_for_client(
            llm_client,
            app_state,
            client_id.to_string(),
            &instruction,
            &memory,
            Some(&event),
            protocol.as_ref(),
            status_tx,
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

                // Execute follow-up actions
                for action in actions {
                    // Note: We'd need to pass db_arc here in a real implementation
                    // For now, just log the actions
                    trace!("MongoDB client {} follow-up action: {:?}", client_id, action);
                }
            }
            Err(e) => {
                error!("LLM error for MongoDB result: {}", e);
            }
        }

        Ok(())
    }

    #[cfg(not(feature = "mongodb"))]
    async fn send_result_event(
        _client_id: ClientId,
        _result_type: &str,
        _documents: Option<Vec<()>>,
        _count: Option<u64>,
        _protocol: &Arc<MongodbClientProtocol>,
        _app_state: &Arc<AppState>,
        _llm_client: &OllamaClient,
        _status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        Err(anyhow::anyhow!("MongoDB client feature not enabled"))
    }
}
