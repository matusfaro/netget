//! S3 client implementation
pub mod actions;

pub use actions::S3ClientProtocol;

use crate::llm::actions::client_trait::Client;
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::client::llm_budget::call_llm_for_client;
use crate::client::s3::actions::S3_CLIENT_RESPONSE_RECEIVED_EVENT;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

/// S3 client that interacts with AWS S3 or S3-compatible services
pub struct S3Client;

impl S3Client {
    /// Connect to an S3 service with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        _llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        info!("S3 client {} initializing for {}", client_id, remote_addr);

        // Parse endpoint URL and region from startup parameters
        let (endpoint_url, region, access_key_id, secret_access_key) = app_state
            .with_client_mut(client_id, |client| {
                let endpoint = client
                    .get_protocol_field("endpoint_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&remote_addr)
                    .to_string();

                let region = client
                    .get_protocol_field("region")
                    .and_then(|v| v.as_str())
                    .unwrap_or("us-east-1")
                    .to_string();

                let access_key = client
                    .get_protocol_field("access_key_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let secret_key = client
                    .get_protocol_field("secret_access_key")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                (endpoint, region, access_key, secret_key)
            })
            .await
            .unwrap_or_else(|| {
                (
                    remote_addr.clone(),
                    "us-east-1".to_string(),
                    String::new(),
                    String::new(),
                )
            });

        // Build AWS SDK configuration
        use aws_config::BehaviorVersion;
        use aws_sdk_s3::config::{Credentials, Region};

        let creds = Credentials::new(
            &access_key_id,
            &secret_access_key,
            None, // session token
            None, // expiry
            "netget-s3-client",
        );

        let mut config_builder = aws_sdk_s3::config::Builder::new()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(region.clone()))
            .credentials_provider(creds);

        // Set custom endpoint if provided (for MinIO, LocalStack, etc.)
        if !endpoint_url.is_empty() && endpoint_url != remote_addr {
            config_builder = config_builder.endpoint_url(&endpoint_url);
        }

        let config = config_builder.build();
        let _s3_client = aws_sdk_s3::Client::from_conf(config);

        // Store client metadata
        app_state
            .with_client_mut(client_id, |client| {
                client.set_protocol_field(
                    "s3_client_initialized".to_string(),
                    serde_json::json!(true),
                );
                client.set_protocol_field("endpoint".to_string(), serde_json::json!(endpoint_url));
                client.set_protocol_field("region".to_string(), serde_json::json!(region));
                client.set_protocol_field(
                    "access_key_id".to_string(),
                    serde_json::json!(access_key_id),
                );
                client.set_protocol_field(
                    "secret_access_key".to_string(),
                    serde_json::json!(secret_access_key),
                );
            })
            .await;

        // Update status
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!(
            "[CLIENT] S3 client {} ready for {}",
            client_id, remote_addr
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Call LLM initially with connected event
        let remote_addr_clone = remote_addr.clone();
        let llm_client_clone = _llm_client.clone();
        let app_state_clone = app_state.clone();
        let status_tx_clone = status_tx.clone();
        let region_clone = region.clone();

        // Registered with AppState so stop_client can abort this task —
        // dropping a JoinHandle only detaches it in Tokio.
        let task_registrar = app_state.clone();
        let task_handle = tokio::spawn(async move {
            use crate::client::s3::actions::S3_CLIENT_CONNECTED_EVENT;

            // Get initial instruction
            let instruction = match app_state_clone.get_instruction_for_client(client_id).await {
                Some(instr) => instr,
                None => {
                    error!("S3 client {} has no instruction", client_id);
                    return;
                }
            };

            let protocol = Arc::new(crate::client::s3::actions::S3ClientProtocol::new());
            let event = Event::new(
                &S3_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "endpoint": remote_addr_clone,
                    "region": region_clone,
                }),
            );

            let memory = app_state_clone
                .get_memory_for_client(client_id)
                .await
                .unwrap_or_default();

            match call_llm_for_client(
                &llm_client_clone,
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
                        use crate::llm::actions::client_trait::ClientActionResult;
                        match protocol.execute_action(action) {
                            Ok(ClientActionResult::Custom { name, data }) => {
                                // Execute S3 operation
                                if let Err(e) = Self::execute_operation(
                                    client_id,
                                    name,
                                    data,
                                    app_state_clone.clone(),
                                    llm_client_clone.clone(),
                                    status_tx_clone.clone(),
                                )
                                .await
                                {
                                    error!("S3 client {} operation error: {}", client_id, e);
                                    let _ = status_tx_clone
                                        .send(format!("[ERROR] S3 operation failed: {}", e));
                                }
                            }
                            Ok(ClientActionResult::Disconnect) => {
                                info!("S3 client {} disconnecting", client_id);
                                app_state_clone
                                    .update_client_status(client_id, ClientStatus::Disconnected)
                                    .await;
                                let _ = status_tx_clone.send("__UPDATE_UI__".to_string());
                                return;
                            }
                            Ok(ClientActionResult::WaitForMore) => {
                                // S3 is request-response, wait for next user action
                                break;
                            }
                            Err(e) => {
                                error!("S3 client {} action error: {}", client_id, e);
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error for S3 client {}: {}", client_id, e);
                }
            }

            // Monitor for client removal
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                if app_state_clone.get_client(client_id).await.is_none() {
                    info!("S3 client {} stopped", client_id);
                    break;
                }
            }
        });
        task_registrar
            .register_client_task(client_id, task_handle)
            .await;

        // Return a dummy local address (S3 is HTTP-based)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Execute an S3 operation
    pub async fn execute_operation(
        client_id: ClientId,
        operation_name: String,
        operation_data: serde_json::Value,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        info!(
            "S3 client {} executing operation: {}",
            client_id, operation_name
        );

        // Get S3 client configuration
        let (endpoint_url, region, access_key_id, secret_access_key) = app_state
            .with_client_mut(client_id, |client| {
                let endpoint = client
                    .get_protocol_field("endpoint")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let region = client
                    .get_protocol_field("region")
                    .and_then(|v| v.as_str())
                    .unwrap_or("us-east-1")
                    .to_string();

                let access_key = client
                    .get_protocol_field("access_key_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let secret_key = client
                    .get_protocol_field("secret_access_key")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                (endpoint, region, access_key, secret_key)
            })
            .await
            .unwrap_or_else(|| {
                (
                    String::new(),
                    "us-east-1".to_string(),
                    String::new(),
                    String::new(),
                )
            });

        // Build AWS SDK client
        use aws_config::BehaviorVersion;
        use aws_sdk_s3::config::{Credentials, Region};

        let creds = Credentials::new(
            &access_key_id,
            &secret_access_key,
            None,
            None,
            "netget-s3-client",
        );

        let mut config_builder = aws_sdk_s3::config::Builder::new()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(region))
            .credentials_provider(creds);

        if !endpoint_url.is_empty() {
            config_builder = config_builder.endpoint_url(&endpoint_url);
        }

        let config = config_builder.build();
        let s3_client = aws_sdk_s3::Client::from_conf(config);

        // Execute the operation
        let result = match operation_name.as_str() {
            "s3_put_object" => Self::put_object(&s3_client, operation_data).await,
            "s3_get_object" => Self::get_object(&s3_client, operation_data).await,
            "s3_list_buckets" => Self::list_buckets(&s3_client).await,
            "s3_list_objects" => Self::list_objects(&s3_client, operation_data).await,
            "s3_delete_object" => Self::delete_object(&s3_client, operation_data).await,
            "s3_head_object" => Self::head_object(&s3_client, operation_data).await,
            "s3_create_bucket" => Self::create_bucket(&s3_client, operation_data).await,
            "s3_delete_bucket" => Self::delete_bucket(&s3_client, operation_data).await,
            _ => Err(anyhow::anyhow!("Unknown S3 operation: {}", operation_name)),
        };

        // Call LLM with result
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::s3::actions::S3ClientProtocol::new());
            let event = match &result {
                Ok(response_data) => Event::new(
                    &S3_CLIENT_RESPONSE_RECEIVED_EVENT,
                    serde_json::json!({
                        "operation": operation_name,
                        "success": true,
                        "result": response_data,
                    }),
                ),
                Err(e) => Event::new(
                    &S3_CLIENT_RESPONSE_RECEIVED_EVENT,
                    serde_json::json!({
                        "operation": operation_name,
                        "success": false,
                        "error": e.to_string(),
                    }),
                ),
            };

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
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }
                }
                Err(e) => {
                    error!("LLM error for S3 client {}: {}", client_id, e);
                }
            }
        }

        result.map(|_| ())
    }

    async fn put_object(
        client: &aws_sdk_s3::Client,
        data: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let bucket = data["bucket"].as_str().context("Missing bucket")?;
        let key = data["key"].as_str().context("Missing key")?;
        let body = data["body"].as_str().context("Missing body")?;
        let content_type = data["content_type"].as_str();

        // The `body` parameter was documented as "text or base64 for binary" while this did
        // `body.as_bytes()`, so a model following the documentation stored the literal base64
        // ASCII in the object. Sniffing is not an option - a string can be valid text and
        // valid base64 at once, and only the sender knows which it means - so the encoding is
        // explicit, exactly as `send_tcp_data` was fixed.
        let body_bytes = match data["encoding"].as_str().unwrap_or("utf8") {
            "utf8" => body.as_bytes().to_vec(),
            "base64" => {
                use base64::Engine as _;
                base64::engine::general_purpose::STANDARD
                    .decode(body)
                    .context(
                        "put_object was given encoding=\"base64\" but `body` is not valid \
                         base64. Send the bytes as base64, or set encoding=\"utf8\" to store \
                         the text as written.",
                    )?
            }
            other => anyhow::bail!(
                "Unknown encoding {:?} for put_object; use \"utf8\" (default) or \"base64\"",
                other
            ),
        };

        let mut request = client
            .put_object()
            .bucket(bucket)
            .key(key)
            .body(body_bytes.into());

        if let Some(ct) = content_type {
            request = request.content_type(ct);
        }

        let response = request.send().await.context("Failed to put object")?;

        Ok(serde_json::json!({
            "bucket": bucket,
            "key": key,
            "etag": response.e_tag().unwrap_or(""),
        }))
    }

    async fn get_object(
        client: &aws_sdk_s3::Client,
        data: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let bucket = data["bucket"].as_str().context("Missing bucket")?;
        let key = data["key"].as_str().context("Missing key")?;

        let response = client
            .get_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .context("Failed to get object")?;

        let content_type = response
            .content_type()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let content_length = response.content_length().unwrap_or(0);

        let body_bytes = response
            .body
            .collect()
            .await
            .context("Failed to read object body")?
            .into_bytes();

        // Report which of the two `body` is, rather than lossily rendering bytes into
        // U+FFFD and losing them. That made the round trip impossible in both directions:
        // binary read back could not be written anywhere, and the model had no way to tell a
        // file containing a replacement character from one that had been mangled.
        let (body, body_encoding) = match std::str::from_utf8(&body_bytes) {
            Ok(text) => (text.to_string(), "utf8"),
            Err(_) => {
                use base64::Engine as _;
                (
                    base64::engine::general_purpose::STANDARD.encode(&body_bytes),
                    "base64",
                )
            }
        };

        Ok(serde_json::json!({
            "bucket": bucket,
            "key": key,
            "content_type": content_type,
            "content_length": content_length,
            "body": body,
            // Feed straight back into put_object's `encoding` to copy an object verbatim.
            "body_encoding": body_encoding,
        }))
    }

    async fn list_buckets(client: &aws_sdk_s3::Client) -> Result<serde_json::Value> {
        let response = client
            .list_buckets()
            .send()
            .await
            .context("Failed to list buckets")?;

        let buckets: Vec<serde_json::Value> = response
            .buckets()
            .iter()
            .map(|b| {
                serde_json::json!({
                    "name": b.name().unwrap_or(""),
                    "creation_date": b.creation_date()
                        .map(|d| d.to_string())
                        .unwrap_or_default(),
                })
            })
            .collect();

        Ok(serde_json::json!({
            "buckets": buckets,
            "count": buckets.len(),
        }))
    }

    async fn list_objects(
        client: &aws_sdk_s3::Client,
        data: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let bucket = data["bucket"].as_str().context("Missing bucket")?;
        let prefix = data["prefix"].as_str();
        let max_keys = data["max_keys"].as_i64().map(|n| n as i32);

        let mut request = client.list_objects_v2().bucket(bucket);

        if let Some(p) = prefix {
            request = request.prefix(p);
        }

        if let Some(mk) = max_keys {
            request = request.max_keys(mk);
        }

        let response = request.send().await.context("Failed to list objects")?;

        let objects: Vec<serde_json::Value> = response
            .contents()
            .iter()
            .map(|obj| {
                serde_json::json!({
                    "key": obj.key().unwrap_or(""),
                    "size": obj.size().unwrap_or(0),
                    "last_modified": obj.last_modified()
                        .map(|d| d.to_string())
                        .unwrap_or_default(),
                    "etag": obj.e_tag().unwrap_or(""),
                })
            })
            .collect();

        Ok(serde_json::json!({
            "bucket": bucket,
            "objects": objects,
            "count": objects.len(),
            "is_truncated": response.is_truncated(),
        }))
    }

    async fn delete_object(
        client: &aws_sdk_s3::Client,
        data: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let bucket = data["bucket"].as_str().context("Missing bucket")?;
        let key = data["key"].as_str().context("Missing key")?;

        client
            .delete_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .context("Failed to delete object")?;

        Ok(serde_json::json!({
            "bucket": bucket,
            "key": key,
            "deleted": true,
        }))
    }

    async fn head_object(
        client: &aws_sdk_s3::Client,
        data: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let bucket = data["bucket"].as_str().context("Missing bucket")?;
        let key = data["key"].as_str().context("Missing key")?;

        let response = client
            .head_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .context("Failed to head object")?;

        Ok(serde_json::json!({
            "bucket": bucket,
            "key": key,
            "content_type": response.content_type().unwrap_or(""),
            "content_length": response.content_length().unwrap_or(0),
            "etag": response.e_tag().unwrap_or(""),
            "last_modified": response.last_modified()
                .map(|d| d.to_string())
                .unwrap_or_default(),
        }))
    }

    async fn create_bucket(
        client: &aws_sdk_s3::Client,
        data: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let bucket = data["bucket"].as_str().context("Missing bucket")?;

        client
            .create_bucket()
            .bucket(bucket)
            .send()
            .await
            .context("Failed to create bucket")?;

        Ok(serde_json::json!({
            "bucket": bucket,
            "created": true,
        }))
    }

    async fn delete_bucket(
        client: &aws_sdk_s3::Client,
        data: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let bucket = data["bucket"].as_str().context("Missing bucket")?;

        client
            .delete_bucket()
            .bucket(bucket)
            .send()
            .await
            .context("Failed to delete bucket")?;

        Ok(serde_json::json!({
            "bucket": bucket,
            "deleted": true,
        }))
    }
}
