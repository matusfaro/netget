//! DynamoDB client implementation
pub mod actions;

pub use actions::DynamoDbClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::{Event, StartupParams};
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::dynamodb::actions::DYNAMODB_CLIENT_RESPONSE_RECEIVED_EVENT;

/// DynamoDB client that interacts with AWS DynamoDB or local instances
pub struct DynamoDbClient;

impl DynamoDbClient {
    /// Connect to a DynamoDB instance with integrated LLM actions
    pub async fn connect_with_llm_actions(
        _remote_addr: String,
        _llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<StartupParams>,
    ) -> Result<SocketAddr> {
        // Extract startup parameters
        let region = startup_params
            .as_ref()
            .and_then(|p| Some(p.get_string("region")))
            .unwrap_or_else(|| "us-east-1".to_string());

        let endpoint_url = startup_params
            .as_ref()
            .map(|p| p.get_string("endpoint_url"));

        let access_key_id = startup_params
            .as_ref()
            .map(|p| p.get_string("access_key_id"));

        let secret_access_key = startup_params
            .as_ref()
            .map(|p| p.get_string("secret_access_key"));

        info!("DynamoDB client {} initializing for region {}", client_id, region);

        // Store configuration in protocol_data
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "region".to_string(),
                serde_json::json!(region.clone()),
            );
            if let Some(endpoint) = &endpoint_url {
                client.set_protocol_field(
                    "endpoint_url".to_string(),
                    serde_json::json!(endpoint),
                );
            }
            if let Some(key_id) = &access_key_id {
                client.set_protocol_field(
                    "access_key_id".to_string(),
                    serde_json::json!(key_id),
                );
            }
            if let Some(secret_key) = &secret_access_key {
                client.set_protocol_field(
                    "secret_access_key".to_string(),
                    serde_json::json!(secret_key),
                );
            }
        }).await;

        // Update status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!(
            "[CLIENT] DynamoDB client {} ready for region {}{}",
            client_id,
            region,
            endpoint_url.as_ref().map(|e| format!(" (endpoint: {})", e)).unwrap_or_default()
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Spawn background monitor task
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("DynamoDB client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return a dummy local address (DynamoDB is HTTP-based)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Execute a PutItem operation
    pub async fn put_item(
        client_id: ClientId,
        table_name: String,
        item: serde_json::Map<String, serde_json::Value>,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        info!("DynamoDB client {} PutItem to table {}", client_id, table_name);

        // Get DynamoDB configuration
        let (region, endpoint_url, access_key_id, secret_access_key) =
            Self::get_config(&app_state, client_id).await?;

        // Build AWS config
        let config = Self::build_aws_config(
            &region,
            endpoint_url.as_deref(),
            access_key_id.as_deref(),
            secret_access_key.as_deref(),
        ).await?;

        // Create DynamoDB client
        let dynamodb_client = aws_sdk_dynamodb::Client::new(&config);

        // Convert JSON item to AttributeValue map
        let mut attribute_map = std::collections::HashMap::new();
        for (key, value) in item {
            if let Some(attr_value) = Self::json_to_attribute_value(&value) {
                attribute_map.insert(key, attr_value);
            }
        }

        // Execute PutItem
        match dynamodb_client
            .put_item()
            .table_name(&table_name)
            .set_item(Some(attribute_map))
            .send()
            .await
        {
            Ok(_) => {
                info!("DynamoDB client {} PutItem succeeded", client_id);

                // Call LLM with success event
                Self::call_llm_with_response(
                    client_id,
                    "put_item",
                    true,
                    Some(serde_json::json!({"table_name": table_name})),
                    None,
                    &app_state,
                    &llm_client,
                    &status_tx,
                ).await?;

                Ok(())
            }
            Err(e) => {
                error!("DynamoDB client {} PutItem failed: {}", client_id, e);

                // Call LLM with error event
                Self::call_llm_with_response(
                    client_id,
                    "put_item",
                    false,
                    None,
                    Some(e.to_string()),
                    &app_state,
                    &llm_client,
                    &status_tx,
                ).await?;

                Err(e.into())
            }
        }
    }

    /// Execute a GetItem operation
    pub async fn get_item(
        client_id: ClientId,
        table_name: String,
        key: serde_json::Map<String, serde_json::Value>,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        info!("DynamoDB client {} GetItem from table {}", client_id, table_name);

        // Get DynamoDB configuration
        let (region, endpoint_url, access_key_id, secret_access_key) =
            Self::get_config(&app_state, client_id).await?;

        // Build AWS config
        let config = Self::build_aws_config(
            &region,
            endpoint_url.as_deref(),
            access_key_id.as_deref(),
            secret_access_key.as_deref(),
        ).await?;

        // Create DynamoDB client
        let dynamodb_client = aws_sdk_dynamodb::Client::new(&config);

        // Convert JSON key to AttributeValue map
        let mut key_map = std::collections::HashMap::new();
        for (key_name, value) in key {
            if let Some(attr_value) = Self::json_to_attribute_value(&value) {
                key_map.insert(key_name, attr_value);
            }
        }

        // Execute GetItem
        match dynamodb_client
            .get_item()
            .table_name(&table_name)
            .set_key(Some(key_map))
            .send()
            .await
        {
            Ok(output) => {
                let item_json = if let Some(item) = output.item {
                    Self::attribute_map_to_json(&item)
                } else {
                    serde_json::json!(null)
                };

                info!("DynamoDB client {} GetItem succeeded", client_id);

                // Call LLM with success event
                Self::call_llm_with_response(
                    client_id,
                    "get_item",
                    true,
                    Some(serde_json::json!({"table_name": table_name, "item": item_json})),
                    None,
                    &app_state,
                    &llm_client,
                    &status_tx,
                ).await?;

                Ok(())
            }
            Err(e) => {
                error!("DynamoDB client {} GetItem failed: {}", client_id, e);

                // Call LLM with error event
                Self::call_llm_with_response(
                    client_id,
                    "get_item",
                    false,
                    None,
                    Some(e.to_string()),
                    &app_state,
                    &llm_client,
                    &status_tx,
                ).await?;

                Err(e.into())
            }
        }
    }

    /// Get DynamoDB configuration from client state
    async fn get_config(
        app_state: &AppState,
        client_id: ClientId,
    ) -> Result<(String, Option<String>, Option<String>, Option<String>)> {
        let region = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("region")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No region found")?;

        let endpoint_url = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("endpoint_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten();

        let access_key_id = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("access_key_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten();

        let secret_access_key = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("secret_access_key")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten();

        Ok((region, endpoint_url, access_key_id, secret_access_key))
    }

    /// Build AWS SDK config
    async fn build_aws_config(
        region: &str,
        endpoint_url: Option<&str>,
        access_key_id: Option<&str>,
        secret_access_key: Option<&str>,
    ) -> Result<aws_config::SdkConfig> {
        use aws_config::BehaviorVersion;

        let mut config_loader = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(region.to_string()));

        // Set custom endpoint if provided (for DynamoDB Local or LocalStack)
        if let Some(endpoint) = endpoint_url {
            config_loader = config_loader.endpoint_url(endpoint);
        }

        // Set credentials if provided
        if let (Some(key_id), Some(secret_key)) = (access_key_id, secret_access_key) {
            use aws_config::meta::credentials::CredentialsProviderChain;
            use aws_credential_types::Credentials;

            let credentials = Credentials::new(
                key_id,
                secret_key,
                None, // session token
                None, // expiry
                "netget_dynamodb_client",
            );

            let provider = CredentialsProviderChain::first_try(
                "Static",
                aws_credential_types::provider::SharedCredentialsProvider::new(credentials),
            );

            config_loader = config_loader.credentials_provider(provider);
        }

        Ok(config_loader.load().await)
    }

    /// Convert JSON value to DynamoDB AttributeValue
    fn json_to_attribute_value(json: &serde_json::Value) -> Option<aws_sdk_dynamodb::types::AttributeValue> {
        use aws_sdk_dynamodb::types::AttributeValue;

        match json {
            serde_json::Value::Object(map) => {
                // Expected format: {"S": "value"} or {"N": "123"} etc.
                if let Some((type_key, value)) = map.iter().next() {
                    match type_key.as_str() {
                        "S" => value.as_str().map(|s| AttributeValue::S(s.to_string())),
                        "N" => value.as_str().map(|s| AttributeValue::N(s.to_string())),
                        "B" => value.as_str().map(|s| {
                            // Base64 decode binary data
                            if let Ok(bytes) = base64::Engine::decode(
                                &base64::engine::general_purpose::STANDARD,
                                s,
                            ) {
                                AttributeValue::B(aws_smithy_types::Blob::new(bytes))
                            } else {
                                AttributeValue::B(aws_smithy_types::Blob::new(Vec::new()))
                            }
                        }),
                        "BOOL" => value.as_bool().map(AttributeValue::Bool),
                        "NULL" => Some(AttributeValue::Null(true)),
                        _ => None,
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Convert DynamoDB AttributeValue map to JSON
    fn attribute_map_to_json(
        map: &std::collections::HashMap<String, aws_sdk_dynamodb::types::AttributeValue>,
    ) -> serde_json::Value {
        use aws_sdk_dynamodb::types::AttributeValue;

        let mut json_map = serde_json::Map::new();
        for (key, value) in map {
            let json_value = match value {
                AttributeValue::S(s) => serde_json::json!({"S": s}),
                AttributeValue::N(n) => serde_json::json!({"N": n}),
                AttributeValue::B(b) => {
                    let b64 = base64::Engine::encode(
                        &base64::engine::general_purpose::STANDARD,
                        b.as_ref(),
                    );
                    serde_json::json!({"B": b64})
                }
                AttributeValue::Bool(b) => serde_json::json!({"BOOL": b}),
                AttributeValue::Null(_) => serde_json::json!({"NULL": true}),
                _ => serde_json::json!({"UNKNOWN": "unsupported_type"}),
            };
            json_map.insert(key.clone(), json_value);
        }
        serde_json::Value::Object(json_map)
    }

    /// Call LLM with DynamoDB response
    async fn call_llm_with_response(
        client_id: ClientId,
        operation: &str,
        success: bool,
        data: Option<serde_json::Value>,
        error: Option<String>,
        app_state: &AppState,
        llm_client: &OllamaClient,
        status_tx: &mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::dynamodb::actions::DynamoDbClientProtocol::new());

            let mut event_data = serde_json::json!({
                "operation": operation,
                "success": success,
            });

            if let Some(d) = data {
                event_data["data"] = d;
            }
            if let Some(e) = error {
                event_data["error"] = serde_json::json!(e);
            }

            let event = Event::new(&DYNAMODB_CLIENT_RESPONSE_RECEIVED_EVENT, event_data);

            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

            match call_llm_for_client(
                llm_client,
                app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                protocol.as_ref(),
                status_tx,
            ).await {
                Ok(ClientLlmResult { actions: _, memory_updates }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }
                }
                Err(e) => {
                    error!("LLM error for DynamoDB client {}: {}", client_id, e);
                }
            }
        }

        Ok(())
    }
}
