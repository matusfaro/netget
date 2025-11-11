//! SQS client implementation
pub mod actions;

pub use actions::SqsClientProtocol;

use anyhow::{Context, Result};
use aws_config::BehaviorVersion;
use aws_sdk_sqs::types::{MessageAttributeValue, QueueAttributeName};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::client::sqs::actions::{
    SQS_CLIENT_CONNECTED_EVENT, SQS_MESSAGE_RECEIVED_EVENT, SQS_MESSAGE_SENT_EVENT,
};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::Client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

/// SQS client that connects to an AWS SQS queue
pub struct SqsClient;

impl SqsClient {
    /// Connect to an SQS queue with integrated LLM actions
    pub async fn connect_with_llm_actions(
        _remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<crate::protocol::StartupParams>,
    ) -> Result<SocketAddr> {
        // Extract startup parameters
        let queue_url = startup_params
            .as_ref()
            .map(|p| p.get_string("queue_url"))
            .context("Missing required 'queue_url' parameter")?;

        let region = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("region"));

        let endpoint_url = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("endpoint_url"));

        info!(
            "SQS client {} connecting to queue: {}",
            client_id, queue_url
        );

        // Configure AWS SDK
        let mut config_loader = aws_config::defaults(BehaviorVersion::latest());

        if let Some(reg) = &region {
            config_loader = config_loader.region(aws_config::Region::new(reg.clone()));
        }

        let config = config_loader.load().await;

        // Build SQS client
        let mut sqs_builder = aws_sdk_sqs::config::Builder::from(&config);

        if let Some(endpoint) = &endpoint_url {
            sqs_builder = sqs_builder.endpoint_url(endpoint);
        }

        let sqs_config = sqs_builder.build();
        let sqs = aws_sdk_sqs::Client::from_conf(sqs_config);

        // Update client status to connected
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!(
            "[CLIENT] SQS client {} connected to {}",
            client_id, queue_url
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        info!("SQS client {} connected", client_id);

        // Call LLM with connected event
        let protocol = Arc::new(SqsClientProtocol::new());
        let event = Event::new(
            &SQS_CLIENT_CONNECTED_EVENT,
            serde_json::json!({
                "queue_url": queue_url.clone(),
            }),
        );

        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
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

                    // Execute actions from initial connection
                    Self::execute_actions(
                        actions,
                        &sqs,
                        &queue_url,
                        protocol.clone(),
                        &llm_client,
                        &app_state,
                        &status_tx,
                        client_id,
                    )
                    .await?;
                }
                Err(e) => {
                    error!("LLM error for SQS client {}: {}", client_id, e);
                }
            }
        }

        // Return a dummy local address (SQS is HTTP-based, no real socket)
        // Use localhost with client_id as port for uniqueness
        let dummy_addr: SocketAddr = format!("127.0.0.1:{}", 10000 + client_id.as_u32())
            .parse()
            .context("Failed to create dummy socket address")?;

        Ok(dummy_addr)
    }

    /// Execute SQS actions from LLM
    fn execute_actions<'a>(
        actions: Vec<serde_json::Value>,
        sqs: &'a aws_sdk_sqs::Client,
        queue_url: &'a str,
        protocol: Arc<SqsClientProtocol>,
        llm_client: &'a OllamaClient,
        app_state: &'a Arc<AppState>,
        status_tx: &'a mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            for action in actions {
                match protocol.execute_action(action) {
                    Ok(crate::llm::actions::client_trait::ClientActionResult::Custom {
                        name,
                        data,
                    }) => match name.as_str() {
                        "send_message" => {
                            Self::send_message(
                                &sqs,
                                &queue_url,
                                &data,
                                protocol.clone(),
                                &llm_client,
                                &app_state,
                                &status_tx,
                                client_id,
                            )
                            .await?;
                        }
                        "receive_messages" => {
                            Self::receive_messages(
                                &sqs,
                                &queue_url,
                                &data,
                                protocol.clone(),
                                &llm_client,
                                &app_state,
                                &status_tx,
                                client_id,
                            )
                            .await?;
                        }
                        "delete_message" => {
                            Self::delete_message(&sqs, &queue_url, &data, client_id).await?;
                        }
                        "purge_queue" => {
                            Self::purge_queue(&sqs, &queue_url, client_id).await?;
                        }
                        "get_queue_attributes" => {
                            Self::get_queue_attributes(&sqs, &queue_url, &data, client_id).await?;
                        }
                        _ => {
                            debug!("Unknown SQS action: {}", name);
                        }
                    },
                    Ok(crate::llm::actions::client_trait::ClientActionResult::Disconnect) => {
                        info!("SQS client {} disconnecting", client_id);
                        app_state
                            .update_client_status(client_id, ClientStatus::Disconnected)
                            .await;
                        let _ = status_tx
                            .send(format!("[CLIENT] SQS client {} disconnected", client_id));
                        let _ = status_tx.send("__UPDATE_UI__".to_string());
                        break;
                    }
                    _ => {}
                }
            }
            Ok(())
        })
    }

    /// Send a message to the SQS queue
    async fn send_message(
        sqs: &aws_sdk_sqs::Client,
        queue_url: &str,
        data: &serde_json::Value,
        protocol: Arc<SqsClientProtocol>,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<()> {
        let message_body = data
            .get("message_body")
            .and_then(|v| v.as_str())
            .context("Missing message_body")?;

        let mut request = sqs
            .send_message()
            .queue_url(queue_url)
            .message_body(message_body);

        // Add delay if specified
        if let Some(delay) = data.get("delay_seconds").and_then(|v| v.as_i64()) {
            request = request.delay_seconds(delay as i32);
        }

        // Add message attributes if specified
        if let Some(attrs) = data.get("message_attributes").and_then(|v| v.as_object()) {
            for (key, value) in attrs {
                if let Some(value_str) = value.as_str() {
                    let msg_attr = MessageAttributeValue::builder()
                        .data_type("String")
                        .string_value(value_str)
                        .build()?;
                    request = request.message_attributes(key.clone(), msg_attr);
                }
            }
        }

        let response = request
            .send()
            .await
            .context("Failed to send message to SQS")?;

        let message_id = response.message_id().unwrap_or("unknown");
        info!("SQS client {} sent message: {}", client_id, message_id);

        // Call LLM with sent event
        let event = Event::new(
            &SQS_MESSAGE_SENT_EVENT,
            serde_json::json!({
                "message_id": message_id,
            }),
        );

        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let memory = app_state
                .get_memory_for_client(client_id)
                .await
                .unwrap_or_default();

            if let Ok(ClientLlmResult {
                actions,
                memory_updates,
            }) = call_llm_for_client(
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
                // Update memory
                if let Some(mem) = memory_updates {
                    app_state.set_memory_for_client(client_id, mem).await;
                }

                // Execute follow-up actions
                Self::execute_actions(
                    actions, sqs, queue_url, protocol, llm_client, app_state, status_tx, client_id,
                )
                .await?;
            }
        }

        Ok(())
    }

    /// Receive messages from the SQS queue
    async fn receive_messages(
        sqs: &aws_sdk_sqs::Client,
        queue_url: &str,
        data: &serde_json::Value,
        protocol: Arc<SqsClientProtocol>,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<()> {
        let mut request = sqs.receive_message().queue_url(queue_url);

        // Set max messages (default 1, max 10)
        if let Some(max) = data.get("max_messages").and_then(|v| v.as_i64()) {
            request = request.max_number_of_messages(max.min(10).max(1) as i32);
        }

        // Set wait time for long polling (default 0, max 20)
        if let Some(wait) = data.get("wait_time_seconds").and_then(|v| v.as_i64()) {
            request = request.wait_time_seconds(wait.min(20).max(0) as i32);
        }

        // Set visibility timeout
        if let Some(timeout) = data.get("visibility_timeout").and_then(|v| v.as_i64()) {
            request = request.visibility_timeout(timeout as i32);
        }

        let response = request
            .send()
            .await
            .context("Failed to receive messages from SQS")?;

        let messages = response.messages();
        info!(
            "SQS client {} received {} messages",
            client_id,
            messages.len()
        );

        if !messages.is_empty() {
            // Build messages array for LLM
            let messages_json: Vec<serde_json::Value> = messages
                .iter()
                .map(|msg| {
                    // Convert attributes to JSON-serializable format
                    let attributes: std::collections::HashMap<String, String> = msg
                        .attributes()
                        .map(|attrs| {
                            attrs
                                .iter()
                                .map(|(k, v)| (k.as_str().to_string(), v.clone()))
                                .collect()
                        })
                        .unwrap_or_default();

                    let message_attributes: std::collections::HashMap<String, String> = msg
                        .message_attributes()
                        .map(|attrs| {
                            attrs
                                .iter()
                                .map(|(k, v)| {
                                    (k.clone(), v.string_value().unwrap_or("").to_string())
                                })
                                .collect()
                        })
                        .unwrap_or_default();

                    serde_json::json!({
                        "message_id": msg.message_id().unwrap_or(""),
                        "receipt_handle": msg.receipt_handle().unwrap_or(""),
                        "body": msg.body().unwrap_or(""),
                        "attributes": attributes,
                        "message_attributes": message_attributes,
                    })
                })
                .collect();

            // Call LLM with received messages
            let event = Event::new(
                &SQS_MESSAGE_RECEIVED_EVENT,
                serde_json::json!({
                    "messages": messages_json,
                }),
            );

            if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                let memory = app_state
                    .get_memory_for_client(client_id)
                    .await
                    .unwrap_or_default();

                if let Ok(ClientLlmResult {
                    actions,
                    memory_updates,
                }) = call_llm_for_client(
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
                    // Update memory
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }

                    // Execute actions (e.g., delete messages)
                    Self::execute_actions(
                        actions, sqs, queue_url, protocol, llm_client, app_state, status_tx,
                        client_id,
                    )
                    .await?;
                }
            }
        }

        Ok(())
    }

    /// Delete a message from the queue
    async fn delete_message(
        sqs: &aws_sdk_sqs::Client,
        queue_url: &str,
        data: &serde_json::Value,
        client_id: ClientId,
    ) -> Result<()> {
        let receipt_handle = data
            .get("receipt_handle")
            .and_then(|v| v.as_str())
            .context("Missing receipt_handle")?;

        sqs.delete_message()
            .queue_url(queue_url)
            .receipt_handle(receipt_handle)
            .send()
            .await
            .context("Failed to delete message from SQS")?;

        info!("SQS client {} deleted message", client_id);
        Ok(())
    }

    /// Purge all messages from the queue
    async fn purge_queue(
        sqs: &aws_sdk_sqs::Client,
        queue_url: &str,
        client_id: ClientId,
    ) -> Result<()> {
        sqs.purge_queue()
            .queue_url(queue_url)
            .send()
            .await
            .context("Failed to purge SQS queue")?;

        info!("SQS client {} purged queue", client_id);
        Ok(())
    }

    /// Get queue attributes
    async fn get_queue_attributes(
        sqs: &aws_sdk_sqs::Client,
        queue_url: &str,
        data: &serde_json::Value,
        client_id: ClientId,
    ) -> Result<()> {
        let mut request = sqs.get_queue_attributes().queue_url(queue_url);

        // Add specific attributes if requested
        if let Some(attr_names) = data.get("attribute_names").and_then(|v| v.as_array()) {
            for attr in attr_names {
                if let Some(attr_str) = attr.as_str() {
                    // Convert string to QueueAttributeName enum
                    match attr_str {
                        "ApproximateNumberOfMessages" => {
                            request = request
                                .attribute_names(QueueAttributeName::ApproximateNumberOfMessages);
                        }
                        "QueueArn" => {
                            request = request.attribute_names(QueueAttributeName::QueueArn);
                        }
                        _ => {
                            request = request.attribute_names(QueueAttributeName::from(attr_str));
                        }
                    }
                }
            }
        } else {
            // Request all attributes
            request = request.attribute_names(QueueAttributeName::All);
        }

        let response = request
            .send()
            .await
            .context("Failed to get queue attributes from SQS")?;

        let attributes = response.attributes();
        info!(
            "SQS client {} got queue attributes: {:?}",
            client_id, attributes
        );
        Ok(())
    }
}
