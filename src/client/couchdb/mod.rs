//! CouchDB client implementation
pub mod actions;

pub use actions::CouchDbClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::client::couchdb::actions::{
    COUCHDB_CLIENT_CONNECTED_EVENT, COUCHDB_CLIENT_RESPONSE_RECEIVED_EVENT,
};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::{console_error, console_info};

/// CouchDB client that connects to a CouchDB server
pub struct CouchDbClient;

impl CouchDbClient {
    /// Connect to a CouchDB server with integrated LLM actions
    #[allow(clippy::too_many_arguments)]
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        username: Option<String>,
        password: Option<String>,
    ) -> Result<SocketAddr> {
        // Build CouchDB URL (add http:// if not present)
        let url = if remote_addr.starts_with("http://") || remote_addr.starts_with("https://") {
            remote_addr.clone()
        } else {
            format!("http://{}", remote_addr)
        };

        console_info!(status_tx, "Connecting to CouchDB at {}", url);

        // Create CouchDB client using couch_rs
        let client = if let (Some(user), Some(pass)) = (username.clone(), password.clone()) {
            console_info!(status_tx, "Using basic auth (username: {})", user);
            couch_rs::Client::new(&url, &user, &pass)
                .context(format!("Failed to create CouchDB client for {}", url))?
        } else {
            couch_rs::Client::new_no_auth(&url)
                .context(format!("Failed to create CouchDB client for {}", url))?
        };

        // Try to connect and get server info
        let server_info = match client.check_status().await {
            Ok(status) => {
                console_info!(
                    status_tx,
                    "Connected to CouchDB (version: {})",
                    &status.version
                );
                Some(serde_json::json!({
                    "couchdb": "Welcome",
                    "version": status.version,
                    "vendor": status.vendor
                }))
            }
            Err(e) => {
                console_error!(status_tx, "Failed to get CouchDB server info: {}", e);
                None
            }
        };

        // Parse local address from URL
        // For HTTP clients, we don't have a real local socket address
        // Use a dummy address
        let local_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();

        info!(
            "CouchDB client {} connected to {} (local: {})",
            client_id, url, local_addr
        );

        // Update client state
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!("[CLIENT] CouchDB client {} connected", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Clone for async task
        let client_arc = Arc::new(tokio::sync::Mutex::new(client));
        let client_for_connected = client_arc.clone();

        // Call LLM with couchdb_connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let event = Event::new(
                &COUCHDB_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "remote_addr": url,
                    "server_info": server_info,
                }),
            );

            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &String::new(), // No memory yet for initial connection
                Some(&event),
                &crate::client::couchdb::actions::CouchDbClientProtocol::new(),
                &status_tx,
            )
            .await
            {
                Ok(result) => {
                    // Execute actions from LLM response
                    for action in result.actions {
                        if let Err(e) = execute_couchdb_action(
                            &action,
                            client_id,
                            &client_for_connected,
                            &app_state,
                            &llm_client,
                            &status_tx,
                        )
                        .await
                        {
                            console_error!(status_tx, "Error executing action after connect: {}", e);
                        }
                    }
                }
                Err(e) => {
                    console_error!(status_tx, "LLM error on couchdb_connected event: {}", e);
                }
            }
        }

        // For CouchDB, we don't have a persistent read loop like TCP clients
        // Instead, operations are driven by LLM actions
        // We'll spawn a task to keep the client alive and handle periodic operations
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
            loop {
                interval.tick().await;

                // Check if client is still connected
                if let Some(client_instance) = app_state.get_client(client_id).await {
                    if client_instance.status == ClientStatus::Disconnected {
                        info!("CouchDB client {} disconnected", client_id);
                        break;
                    }
                } else {
                    // Client no longer exists
                    break;
                }

                // Optionally poll for changes if watching a database
                // This would require maintaining state about which databases are being watched
                // For now, we just keep the client alive
            }
        });

        Ok(local_addr)
    }
}

/// Execute a CouchDB action from the LLM
async fn execute_couchdb_action(
    action: &serde_json::Value,
    client_id: ClientId,
    client: &Arc<tokio::sync::Mutex<couch_rs::Client>>,
    app_state: &Arc<AppState>,
    llm_client: &OllamaClient,
    status_tx: &mpsc::UnboundedSender<String>,
) -> Result<()> {
    let action_type = action
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing action type"))?;

    match action_type {
        "create_database" => {
            let db_name = action
                .get("database")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing database name"))?;

            console_info!(status_tx, "Creating database: {}", db_name);

            let client_guard = client.lock().await;
            match client_guard.make_db(db_name).await {
                Ok(_) => {
                    console_info!(status_tx, "Database {} created successfully", db_name);
                    send_response_event(
                        client_id,
                        "create_database",
                        true,
                        serde_json::json!({"database": db_name}),
                        None,
                        app_state,
                        llm_client,
                        status_tx,
                    )
                    .await;
                }
                Err(e) => {
                    console_error!(status_tx, "Failed to create database {}: {}", db_name, e);
                    send_response_event(
                        client_id,
                        "create_database",
                        false,
                        serde_json::json!({}),
                        Some(format!("{}", e)),
                        app_state,
                        llm_client,
                        status_tx,
                    )
                    .await;
                }
            }
        }
        "delete_database" => {
            let db_name = action
                .get("database")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing database name"))?;

            console_info!(status_tx, "Deleting database: {}", db_name);

            let client_guard = client.lock().await;
            match client_guard.destroy_db(db_name).await {
                Ok(_) => {
                    console_info!(status_tx, "Database {} deleted successfully", db_name);
                    send_response_event(
                        client_id,
                        "delete_database",
                        true,
                        serde_json::json!({"database": db_name}),
                        None,
                        app_state,
                        llm_client,
                        status_tx,
                    )
                    .await;
                }
                Err(e) => {
                    console_error!(status_tx, "Failed to delete database {}: {}", db_name, e);
                    send_response_event(
                        client_id,
                        "delete_database",
                        false,
                        serde_json::json!({}),
                        Some(format!("{}", e)),
                        app_state,
                        llm_client,
                        status_tx,
                    )
                    .await;
                }
            }
        }
        "list_databases" => {
            console_info!(status_tx, "Listing all databases");

            let client_guard = client.lock().await;
            match client_guard.list_dbs().await {
                Ok(dbs) => {
                    console_info!(status_tx, "Found {} databases", dbs.len());
                    send_response_event(
                        client_id,
                        "list_databases",
                        true,
                        serde_json::json!({"databases": dbs}),
                        None,
                        app_state,
                        llm_client,
                        status_tx,
                    )
                    .await;
                }
                Err(e) => {
                    console_error!(status_tx, "Failed to list databases: {}", e);
                    send_response_event(
                        client_id,
                        "list_databases",
                        false,
                        serde_json::json!({}),
                        Some(format!("{}", e)),
                        app_state,
                        llm_client,
                        status_tx,
                    )
                    .await;
                }
            }
        }
        "create_document" => {
            let db_name = action
                .get("database")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing database name"))?;

            let doc_id = action.get("doc_id").and_then(|v| v.as_str());

            let document = action
                .get("document")
                .ok_or_else(|| anyhow::anyhow!("Missing document"))?;

            console_info!(
                status_tx,
                "Creating document in {}: {:?}",
                db_name,
                doc_id
            );

            let client_guard = client.lock().await;

            // Use raw HTTP API via req()
            let (path, method) = if let Some(id) = doc_id {
                // PUT /{db}/{docid} - create with specific ID
                (format!("/{}/{}", db_name, id), reqwest::Method::PUT)
            } else {
                // POST /{db} - auto-generate ID
                (format!("/{}", db_name), reqwest::Method::POST)
            };

            let response = match client_guard
                .req(method.clone(), &path, None)
                .json(document)
                .send()
                .await
            {
                Ok(resp) => resp,
                Err(e) => {
                    console_error!(status_tx, "Failed to create document: {}", e);
                    send_response_event(
                        client_id,
                        "create_document",
                        false,
                        serde_json::json!({}),
                        Some(format!("Request failed: {}", e)),
                        app_state,
                        llm_client,
                        status_tx,
                    )
                    .await;
                    return Ok(());
                }
            };

            let status = response.status();
            match response.json::<serde_json::Value>().await {
                Ok(result) => {
                    if status.is_success() {
                        console_info!(
                            status_tx,
                            "Document created: {} (rev: {})",
                            result.get("id").and_then(|v| v.as_str()).unwrap_or("unknown"),
                            result.get("rev").and_then(|v| v.as_str()).unwrap_or("unknown")
                        );
                        send_response_event(
                            client_id,
                            "create_document",
                            true,
                            result,
                            None,
                            app_state,
                            llm_client,
                            status_tx,
                        )
                        .await;
                    } else {
                        console_error!(
                            status_tx,
                            "Failed to create document: {} - {}",
                            status,
                            result.get("reason").and_then(|v| v.as_str()).unwrap_or("unknown error")
                        );
                        send_response_event(
                            client_id,
                            "create_document",
                            false,
                            serde_json::json!({}),
                            Some(format!(
                                "{}: {}",
                                result.get("error").and_then(|v| v.as_str()).unwrap_or("error"),
                                result.get("reason").and_then(|v| v.as_str()).unwrap_or("unknown")
                            )),
                            app_state,
                            llm_client,
                            status_tx,
                        )
                        .await;
                    }
                }
                Err(e) => {
                    console_error!(status_tx, "Failed to parse response: {}", e);
                    send_response_event(
                        client_id,
                        "create_document",
                        false,
                        serde_json::json!({}),
                        Some(format!("Response parsing failed: {}", e)),
                        app_state,
                        llm_client,
                        status_tx,
                    )
                    .await;
                }
            }
        }
        "get_document" => {
            let db_name = action
                .get("database")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing database name"))?;

            let doc_id = action
                .get("doc_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing doc_id"))?;

            console_info!(status_tx, "Getting document {}/{}", db_name, doc_id);

            let client_guard = client.lock().await;
            let db = match client_guard.db(db_name).await {
                Ok(db) => db,
                Err(e) => {
                    console_error!(status_tx, "Failed to get database {}: {}", db_name, e);
                    send_response_event(
                        client_id,
                        "get_document",
                        false,
                        serde_json::json!({}),
                        Some(format!("Database not found: {}", e)),
                        app_state,
                        llm_client,
                        status_tx,
                    )
                    .await;
                    return Ok(());
                }
            };

            match db.get_raw(doc_id).await {
                Ok(doc) => {
                    console_info!(status_tx, "Document retrieved: {}", doc_id);
                    send_response_event(
                        client_id,
                        "get_document",
                        true,
                        doc,
                        None,
                        app_state,
                        llm_client,
                        status_tx,
                    )
                    .await;
                }
                Err(e) => {
                    console_error!(status_tx, "Failed to get document {}: {}", doc_id, e);
                    send_response_event(
                        client_id,
                        "get_document",
                        false,
                        serde_json::json!({}),
                        Some(format!("{}", e)),
                        app_state,
                        llm_client,
                        status_tx,
                    )
                    .await;
                }
            }
        }
        "update_document" => {
            let db_name = action
                .get("database")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing database name"))?;

            let doc_id = action
                .get("doc_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing doc_id"))?;

            let document = action
                .get("document")
                .ok_or_else(|| anyhow::anyhow!("Missing document"))?;

            // Ensure document has _id
            let mut doc = document.clone();
            if let Some(obj) = doc.as_object_mut() {
                obj.insert("_id".to_string(), serde_json::json!(doc_id));
            }

            // Verify _rev is present (required for updates)
            let rev = doc.get("_rev").and_then(|v| v.as_str()).ok_or_else(|| {
                anyhow::anyhow!("Missing _rev field in document (required for updates)")
            })?;

            console_info!(
                status_tx,
                "Updating document {}/{} (rev: {})",
                db_name,
                doc_id,
                rev
            );

            let client_guard = client.lock().await;

            // Use raw HTTP API via req()
            // PUT /{db}/{docid} with document including _rev
            let path = format!("/{}/{}", db_name, doc_id);

            let response = match client_guard
                .req(reqwest::Method::PUT, &path, None)
                .json(&doc)
                .send()
                .await
            {
                Ok(resp) => resp,
                Err(e) => {
                    console_error!(status_tx, "Failed to update document: {}", e);
                    send_response_event(
                        client_id,
                        "update_document",
                        false,
                        serde_json::json!({}),
                        Some(format!("Request failed: {}", e)),
                        app_state,
                        llm_client,
                        status_tx,
                    )
                    .await;
                    return Ok(());
                }
            };

            let status = response.status();
            match response.json::<serde_json::Value>().await {
                Ok(result) => {
                    if status.is_success() {
                        console_info!(
                            status_tx,
                            "Document updated: {} (new rev: {})",
                            result.get("id").and_then(|v| v.as_str()).unwrap_or("unknown"),
                            result.get("rev").and_then(|v| v.as_str()).unwrap_or("unknown")
                        );
                        send_response_event(
                            client_id,
                            "update_document",
                            true,
                            result,
                            None,
                            app_state,
                            llm_client,
                            status_tx,
                        )
                        .await;
                    } else {
                        // Check for conflict (409)
                        if status.as_u16() == 409 {
                            console_error!(status_tx, "Conflict updating document {}: revision mismatch", doc_id);
                            send_conflict_event(
                                client_id,
                                db_name,
                                doc_id,
                                Some(rev),
                                app_state,
                                llm_client,
                                status_tx,
                            )
                            .await;
                        }

                        console_error!(
                            status_tx,
                            "Failed to update document: {} - {}",
                            status,
                            result.get("reason").and_then(|v| v.as_str()).unwrap_or("unknown error")
                        );
                        send_response_event(
                            client_id,
                            "update_document",
                            false,
                            serde_json::json!({}),
                            Some(format!(
                                "{}: {}",
                                result.get("error").and_then(|v| v.as_str()).unwrap_or("error"),
                                result.get("reason").and_then(|v| v.as_str()).unwrap_or("unknown")
                            )),
                            app_state,
                            llm_client,
                            status_tx,
                        )
                        .await;
                    }
                }
                Err(e) => {
                    console_error!(status_tx, "Failed to parse response: {}", e);
                    send_response_event(
                        client_id,
                        "update_document",
                        false,
                        serde_json::json!({}),
                        Some(format!("Response parsing failed: {}", e)),
                        app_state,
                        llm_client,
                        status_tx,
                    )
                    .await;
                }
            }
        }
        "delete_document" => {
            let db_name = action
                .get("database")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing database name"))?;

            let doc_id = action
                .get("doc_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing doc_id"))?;

            let rev = action
                .get("rev")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing rev"))?;

            console_info!(
                status_tx,
                "Deleting document {}/{} (rev: {})",
                db_name,
                doc_id,
                rev
            );

            let client_guard = client.lock().await;

            // Use raw HTTP API via req()
            // DELETE /{db}/{docid}?rev={rev}
            let path = format!("/{}/{}", db_name, doc_id);
            let mut params = std::collections::HashMap::new();
            params.insert("rev".to_string(), rev.to_string());

            let response = match client_guard
                .req(reqwest::Method::DELETE, &path, Some(&params))
                .send()
                .await
            {
                Ok(resp) => resp,
                Err(e) => {
                    console_error!(status_tx, "Failed to delete document: {}", e);
                    send_response_event(
                        client_id,
                        "delete_document",
                        false,
                        serde_json::json!({}),
                        Some(format!("Request failed: {}", e)),
                        app_state,
                        llm_client,
                        status_tx,
                    )
                    .await;
                    return Ok(());
                }
            };

            let status = response.status();
            match response.json::<serde_json::Value>().await {
                Ok(result) => {
                    if status.is_success() {
                        console_info!(
                            status_tx,
                            "Document deleted: {} (rev: {})",
                            result.get("id").and_then(|v| v.as_str()).unwrap_or("unknown"),
                            result.get("rev").and_then(|v| v.as_str()).unwrap_or("unknown")
                        );
                        send_response_event(
                            client_id,
                            "delete_document",
                            true,
                            result,
                            None,
                            app_state,
                            llm_client,
                            status_tx,
                        )
                        .await;
                    } else {
                        // Check for conflict (409)
                        if status.as_u16() == 409 {
                            console_error!(
                                status_tx,
                                "Conflict deleting document {}: revision mismatch",
                                doc_id
                            );
                            send_conflict_event(
                                client_id,
                                db_name,
                                doc_id,
                                Some(rev),
                                app_state,
                                llm_client,
                                status_tx,
                            )
                            .await;
                        }

                        console_error!(
                            status_tx,
                            "Failed to delete document: {} - {}",
                            status,
                            result
                                .get("reason")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown error")
                        );
                        send_response_event(
                            client_id,
                            "delete_document",
                            false,
                            serde_json::json!({}),
                            Some(format!(
                                "{}: {}",
                                result
                                    .get("error")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("error"),
                                result
                                    .get("reason")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                            )),
                            app_state,
                            llm_client,
                            status_tx,
                        )
                        .await;
                    }
                }
                Err(e) => {
                    console_error!(status_tx, "Failed to parse response: {}", e);
                    send_response_event(
                        client_id,
                        "delete_document",
                        false,
                        serde_json::json!({}),
                        Some(format!("Response parsing failed: {}", e)),
                        app_state,
                        llm_client,
                        status_tx,
                    )
                    .await;
                }
            }
        }
        "bulk_docs" => {
            let db_name = action
                .get("database")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing database name"))?;

            let docs = action
                .get("docs")
                .and_then(|v| v.as_array())
                .ok_or_else(|| anyhow::anyhow!("Missing or invalid docs array"))?;

            console_info!(
                status_tx,
                "Bulk docs: {} documents in {}",
                docs.len(),
                db_name
            );

            let client_guard = client.lock().await;

            // Use raw HTTP API via req()
            // POST /{db}/_bulk_docs with {"docs": [...]}
            let path = format!("/{}/_bulk_docs", db_name);
            let body = serde_json::json!({
                "docs": docs
            });

            let response = match client_guard
                .req(reqwest::Method::POST, &path, None)
                .json(&body)
                .send()
                .await
            {
                Ok(resp) => resp,
                Err(e) => {
                    console_error!(status_tx, "Failed to perform bulk docs: {}", e);
                    send_response_event(
                        client_id,
                        "bulk_docs",
                        false,
                        serde_json::json!({}),
                        Some(format!("Request failed: {}", e)),
                        app_state,
                        llm_client,
                        status_tx,
                    )
                    .await;
                    return Ok(());
                }
            };

            let status = response.status();
            match response.json::<serde_json::Value>().await {
                Ok(results) => {
                    if status.is_success() {
                        // Results is an array of {ok, id, rev} or {error, reason}
                        let count = if let Some(arr) = results.as_array() {
                            arr.len()
                        } else {
                            0
                        };
                        console_info!(status_tx, "Bulk docs completed: {} results", count);
                        send_response_event(
                            client_id,
                            "bulk_docs",
                            true,
                            results,
                            None,
                            app_state,
                            llm_client,
                            status_tx,
                        )
                        .await;
                    } else {
                        console_error!(
                            status_tx,
                            "Failed to perform bulk docs: {} - {}",
                            status,
                            results
                                .get("reason")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown error")
                        );
                        send_response_event(
                            client_id,
                            "bulk_docs",
                            false,
                            serde_json::json!({}),
                            Some(format!(
                                "{}: {}",
                                results
                                    .get("error")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("error"),
                                results
                                    .get("reason")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                            )),
                            app_state,
                            llm_client,
                            status_tx,
                        )
                        .await;
                    }
                }
                Err(e) => {
                    console_error!(status_tx, "Failed to parse response: {}", e);
                    send_response_event(
                        client_id,
                        "bulk_docs",
                        false,
                        serde_json::json!({}),
                        Some(format!("Response parsing failed: {}", e)),
                        app_state,
                        llm_client,
                        status_tx,
                    )
                    .await;
                }
            }
        }
        "list_documents" => {
            let db_name = action
                .get("database")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing database name"))?;

            let _include_docs = action
                .get("include_docs")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            console_info!(status_tx, "Listing documents in {}", db_name);

            let client_guard = client.lock().await;
            let db = match client_guard.db(db_name).await {
                Ok(db) => db,
                Err(e) => {
                    console_error!(status_tx, "Failed to get database {}: {}", db_name, e);
                    send_response_event(
                        client_id,
                        "list_documents",
                        false,
                        serde_json::json!({}),
                        Some(format!("Database not found: {}", e)),
                        app_state,
                        llm_client,
                        status_tx,
                    )
                    .await;
                    return Ok(());
                }
            };

            match db.get_all_raw().await {
                Ok(all_docs) => {
                    console_info!(status_tx, "Found {} documents", all_docs.total_rows);
                    // Convert DocumentCollection to JSON manually
                    let docs_json = serde_json::json!({
                        "total_rows": all_docs.total_rows,
                        "offset": all_docs.offset,
                        "rows": all_docs.rows
                    });
                    send_response_event(
                        client_id,
                        "list_documents",
                        true,
                        docs_json,
                        None,
                        app_state,
                        llm_client,
                        status_tx,
                    )
                    .await;
                }
                Err(e) => {
                    console_error!(status_tx, "Failed to list documents: {}", e);
                    send_response_event(
                        client_id,
                        "list_documents",
                        false,
                        serde_json::json!({}),
                        Some(format!("{}", e)),
                        app_state,
                        llm_client,
                        status_tx,
                    )
                    .await;
                }
            }
        }
        "query_view" => {
            console_info!(status_tx, "View queries not yet fully implemented in couch_rs");
            send_response_event(
                client_id,
                "query_view",
                false,
                serde_json::json!({}),
                Some("View queries not yet implemented".to_string()),
                app_state,
                llm_client,
                status_tx,
            )
            .await;
        }
        "watch_changes" => {
            console_info!(status_tx, "Changes feed watching not yet fully implemented");
            send_response_event(
                client_id,
                "watch_changes",
                false,
                serde_json::json!({}),
                Some("Changes feed not yet implemented".to_string()),
                app_state,
                llm_client,
                status_tx,
            )
            .await;
        }
        "disconnect" => {
            console_info!(status_tx, "Disconnecting CouchDB client {}", client_id);
            app_state
                .update_client_status(client_id, ClientStatus::Disconnected)
                .await;
        }
        _ => {
            console_error!(status_tx, "Unknown action type: {}", action_type);
        }
    }

    Ok(())
}

/// Send response event to LLM
async fn send_response_event(
    client_id: ClientId,
    operation: &str,
    success: bool,
    data: serde_json::Value,
    error: Option<String>,
    app_state: &Arc<AppState>,
    llm_client: &OllamaClient,
    status_tx: &mpsc::UnboundedSender<String>,
) {
    if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
        let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

        let mut event_data = serde_json::json!({
            "operation": operation,
            "success": success,
            "data": data,
        });

        if let Some(err) = error {
            event_data["error"] = serde_json::json!(err);
        }

        let event = Event::new(&COUCHDB_CLIENT_RESPONSE_RECEIVED_EVENT, event_data);

        match call_llm_for_client(
            llm_client,
            app_state,
            client_id.to_string(),
            &instruction,
            &memory,
            Some(&event),
            &crate::client::couchdb::actions::CouchDbClientProtocol::new(),
            status_tx,
        )
        .await
        {
            Ok(result) => {
                // Update memory if provided
                if let Some(new_memory) = result.memory_updates {
                    app_state.set_memory_for_client(client_id, new_memory).await;
                }

                // Execute any new actions from LLM
                // Note: We can't execute them here without access to the client
                // This would require a more complex callback mechanism
            }
            Err(e) => {
                error!("LLM error on response event: {}", e);
            }
        }
    }
}

/// Send conflict event to LLM when document revision mismatch occurs
async fn send_conflict_event(
    client_id: ClientId,
    database: &str,
    doc_id: &str,
    expected_rev: Option<&str>,
    app_state: &Arc<AppState>,
    llm_client: &OllamaClient,
    status_tx: &mpsc::UnboundedSender<String>,
) {
    use crate::client::couchdb::actions::COUCHDB_CLIENT_CONFLICT_EVENT;

    if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
        let memory = app_state
            .get_memory_for_client(client_id)
            .await
            .unwrap_or_default();

        let event = Event::new(
            &COUCHDB_CLIENT_CONFLICT_EVENT,
            serde_json::json!({
                "database": database,
                "doc_id": doc_id,
                "expected_rev": expected_rev,
            }),
        );

        let _ = call_llm_for_client(
            llm_client,
            app_state,
            client_id.to_string(),
            &instruction,
            &memory,
            Some(&event),
            &crate::client::couchdb::actions::CouchDbClientProtocol::new(),
            status_tx,
        )
        .await;
    }
}
