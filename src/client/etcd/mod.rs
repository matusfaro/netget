//! etcd client implementation
pub mod actions;

pub use actions::EtcdClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::etcd::actions::{ETCD_CLIENT_CONNECTED_EVENT, ETCD_CLIENT_RESPONSE_RECEIVED_EVENT};

/// etcd client that connects to remote etcd servers
pub struct EtcdClient;

impl EtcdClient {
    /// Connect to an etcd server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        info!("etcd client {} connecting to {}", client_id, remote_addr);

        // Parse endpoint (etcd-client expects a Vec of endpoints)
        let endpoints = vec![remote_addr.clone()];

        // Connect to etcd using etcd-client
        let mut etcd_client = etcd_client::Client::connect(&endpoints, None)
            .await
            .context("Failed to connect to etcd server")?;

        info!("etcd client {} connected successfully to {}", client_id, remote_addr);

        // Store client state
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "etcd_connected".to_string(),
                serde_json::json!(true),
            );
            client.set_protocol_field(
                "endpoints".to_string(),
                serde_json::json!(endpoints),
            );
        }).await;

        // Update status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] etcd client {} connected to {}", client_id, remote_addr));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Send connected event to LLM
        let connected_event = Event::new(
            &ETCD_CLIENT_CONNECTED_EVENT,
            serde_json::json!({
                "remote_addr": remote_addr,
            }),
        );

        // Call LLM with connected event
        match call_llm_for_client(
            client_id,
            app_state.clone(),
            llm_client.clone(),
            status_tx.clone(),
            Some(&connected_event),
        )
        .await
        {
            Ok(ClientLlmResult::ActionsGenerated { actions, memory: _ }) => {
                debug!("etcd client {} LLM generated {} actions on connect", client_id, actions.len());
            }
            Ok(ClientLlmResult::NoActions) => {
                debug!("etcd client {} LLM generated no actions on connect", client_id);
            }
            Err(e) => {
                error!("etcd client {} LLM call failed on connect: {}", client_id, e);
            }
        }

        // Spawn background task that monitors for client closure
        // etcd operations are made on-demand via actions
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("etcd client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return a dummy local address (etcd client is connection-based but doesn't expose local addr)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Execute a get operation
    pub async fn get_key(
        client_id: ClientId,
        key: String,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get endpoint from client state
        let endpoints: Vec<String> = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("endpoints")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
        }).await.flatten().context("No endpoints found")?;

        info!("etcd client {} getting key: {}", client_id, key);

        // Connect and get key
        let mut etcd_client = etcd_client::Client::connect(&endpoints, None)
            .await
            .context("Failed to reconnect to etcd server")?;

        let resp = etcd_client.get(key.clone(), None)
            .await
            .context("Failed to get key from etcd")?;

        // Build response data
        let kvs: Vec<serde_json::Value> = resp.kvs().iter().map(|kv| {
            serde_json::json!({
                "key": String::from_utf8_lossy(kv.key()).to_string(),
                "value": String::from_utf8_lossy(kv.value()).to_string(),
                "create_revision": kv.create_revision(),
                "mod_revision": kv.mod_revision(),
                "version": kv.version(),
                "lease": kv.lease(),
            })
        }).collect();

        debug!("etcd client {} received {} key-value pairs", client_id, kvs.len());

        // Send response event to LLM
        let response_event = Event::new(
            &ETCD_CLIENT_RESPONSE_RECEIVED_EVENT,
            serde_json::json!({
                "operation": "get",
                "key": key,
                "kvs": kvs,
                "count": resp.count(),
                "more": resp.more(),
            }),
        );

        // Call LLM with response event
        match call_llm_for_client(
            client_id,
            app_state.clone(),
            llm_client.clone(),
            status_tx.clone(),
            Some(&response_event),
        )
        .await
        {
            Ok(ClientLlmResult::ActionsGenerated { actions, memory: _ }) => {
                debug!("etcd client {} LLM generated {} actions after get", client_id, actions.len());
            }
            Ok(ClientLlmResult::NoActions) => {
                debug!("etcd client {} LLM generated no actions after get", client_id);
            }
            Err(e) => {
                error!("etcd client {} LLM call failed after get: {}", client_id, e);
            }
        }

        Ok(())
    }

    /// Execute a put operation
    pub async fn put_key(
        client_id: ClientId,
        key: String,
        value: String,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get endpoint from client state
        let endpoints: Vec<String> = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("endpoints")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
        }).await.flatten().context("No endpoints found")?;

        info!("etcd client {} putting key: {} = {}", client_id, key, value);

        // Connect and put key
        let mut etcd_client = etcd_client::Client::connect(&endpoints, None)
            .await
            .context("Failed to reconnect to etcd server")?;

        let resp = etcd_client.put(key.clone(), value.clone(), None)
            .await
            .context("Failed to put key to etcd")?;

        debug!("etcd client {} put completed, header revision: {}", client_id, resp.header().unwrap().revision());

        // Send response event to LLM
        let response_event = Event::new(
            &ETCD_CLIENT_RESPONSE_RECEIVED_EVENT,
            serde_json::json!({
                "operation": "put",
                "key": key,
                "value": value,
                "revision": resp.header().map(|h| h.revision()).unwrap_or(0),
            }),
        );

        // Call LLM with response event
        match call_llm_for_client(
            client_id,
            app_state.clone(),
            llm_client.clone(),
            status_tx.clone(),
            Some(&response_event),
        )
        .await
        {
            Ok(ClientLlmResult::ActionsGenerated { actions, memory: _ }) => {
                debug!("etcd client {} LLM generated {} actions after put", client_id, actions.len());
            }
            Ok(ClientLlmResult::NoActions) => {
                debug!("etcd client {} LLM generated no actions after put", client_id);
            }
            Err(e) => {
                error!("etcd client {} LLM call failed after put: {}", client_id, e);
            }
        }

        Ok(())
    }

    /// Execute a delete operation
    pub async fn delete_key(
        client_id: ClientId,
        key: String,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get endpoint from client state
        let endpoints: Vec<String> = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("endpoints")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
        }).await.flatten().context("No endpoints found")?;

        info!("etcd client {} deleting key: {}", client_id, key);

        // Connect and delete key
        let mut etcd_client = etcd_client::Client::connect(&endpoints, None)
            .await
            .context("Failed to reconnect to etcd server")?;

        let resp = etcd_client.delete(key.clone(), None)
            .await
            .context("Failed to delete key from etcd")?;

        debug!("etcd client {} delete completed, deleted {} keys", client_id, resp.deleted());

        // Send response event to LLM
        let response_event = Event::new(
            &ETCD_CLIENT_RESPONSE_RECEIVED_EVENT,
            serde_json::json!({
                "operation": "delete",
                "key": key,
                "deleted": resp.deleted(),
            }),
        );

        // Call LLM with response event
        match call_llm_for_client(
            client_id,
            app_state.clone(),
            llm_client.clone(),
            status_tx.clone(),
            Some(&response_event),
        )
        .await
        {
            Ok(ClientLlmResult::ActionsGenerated { actions, memory: _ }) => {
                debug!("etcd client {} LLM generated {} actions after delete", client_id, actions.len());
            }
            Ok(ClientLlmResult::NoActions) => {
                debug!("etcd client {} LLM generated no actions after delete", client_id);
            }
            Err(e) => {
                error!("etcd client {} LLM call failed after delete: {}", client_id, e);
            }
        }

        Ok(())
    }
}
