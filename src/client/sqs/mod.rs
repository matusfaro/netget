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

use crate::client::llm_budget::call_llm_for_client;
use crate::client::sqs::actions::{
    SQS_CLIENT_CONNECTED_EVENT, SQS_MESSAGE_RECEIVED_EVENT, SQS_MESSAGE_SENT_EVENT,
};
use crate::llm::actions::client_trait::{Client, ClientActionResult};
use crate::llm::actions::protocol_trait::Protocol;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::client_handles::{ClientCommand, ClientSendOutcome};
use crate::state::{AccessLogOwner, ClientId, ClientStatus};
use crate::utils::truncate::truncate_for_log;

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
            .transpose()?
            .context("Missing required 'queue_url' parameter")?;

        let region = startup_params
            .as_ref()
            .map(|p| p.get_optional_string("region"))
            .transpose()?
            .flatten();

        let endpoint_url = startup_params
            .as_ref()
            .map(|p| p.get_optional_string("endpoint_url"))
            .transpose()?
            .flatten();

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

        let protocol = Arc::new(SqsClientProtocol::new());

        // Command channel for injected actions (the dashboard's [ send ] row).
        // Registered BEFORE the connected-event LLM call below: a dashboard-created
        // client defaults to a `*` -> manual routing rule, which parks that call until
        // a human answers, and [ send ] has to work for the whole park. It is also the
        // only long-lived task this client has ever had - `connect` used to return with
        // nothing running, so nothing could act on the queue afterwards.
        let command_rx =
            crate::client::command_support::register_command_channel(&app_state, client_id).await;
        let cmd_task = tokio::spawn(Self::command_loop(
            command_rx,
            sqs.clone(),
            queue_url.clone(),
            protocol.clone(),
            client_id,
            app_state.clone(),
            llm_client.clone(),
            status_tx.clone(),
        ));
        app_state.register_client_task(client_id, cmd_task).await;

        // Call LLM with connected event
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
                let result = match protocol.execute_action(action) {
                    Ok(result) => result,
                    Err(e) => {
                        error!("SQS client {} rejected action: {}", client_id, e);
                        continue;
                    }
                };
                let outcome = Self::apply_action(
                    result,
                    &sqs,
                    queue_url,
                    protocol.clone(),
                    llm_client,
                    app_state,
                    status_tx,
                    client_id,
                )
                .await;
                if matches!(outcome, ClientSendOutcome::Disconnected) {
                    info!("SQS client {} disconnecting", client_id);
                    app_state
                        .update_client_status(client_id, ClientStatus::Disconnected)
                        .await;
                    app_state.remove_client_handle(client_id).await;
                    let _ =
                        status_tx.send(format!("[CLIENT] SQS client {} disconnected", client_id));
                    let _ = status_tx.send("__UPDATE_UI__".to_string());
                    break;
                }
                debug!("SQS client {} action outcome: {:?}", client_id, outcome);
            }
            Ok(())
        })
    }

    /// Apply one executed action against the queue. Shared by the connected-event
    /// LLM path, by response follow-ups, and by injected commands, so the three
    /// cannot diverge.
    ///
    /// The AWS SDK owns the socket and reports no wire byte count, so a completed
    /// operation is `Executed { detail }` naming the operation and its result -
    /// never `Sent`, which would be a fabricated byte count.
    #[allow(clippy::too_many_arguments)]
    fn apply_action<'a>(
        result: ClientActionResult,
        sqs: &'a aws_sdk_sqs::Client,
        queue_url: &'a str,
        protocol: Arc<SqsClientProtocol>,
        llm_client: &'a OllamaClient,
        app_state: &'a Arc<AppState>,
        status_tx: &'a mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ClientSendOutcome> + Send + 'a>> {
        Box::pin(async move {
            match result {
                ClientActionResult::Custom { name, data } => {
                    let outcome = match name.as_str() {
                        "send_message" => {
                            Self::send_message(
                                sqs,
                                queue_url,
                                &data,
                                protocol.clone(),
                                llm_client,
                                app_state,
                                status_tx,
                                client_id,
                            )
                            .await
                        }
                        "receive_messages" => {
                            Self::receive_messages(
                                sqs,
                                queue_url,
                                &data,
                                protocol.clone(),
                                llm_client,
                                app_state,
                                status_tx,
                                client_id,
                            )
                            .await
                        }
                        "delete_message" => {
                            Self::delete_message(sqs, queue_url, &data, client_id).await
                        }
                        "purge_queue" => Self::purge_queue(sqs, queue_url, client_id).await,
                        "get_queue_attributes" => {
                            Self::get_queue_attributes(sqs, queue_url, &data, client_id).await
                        }
                        other => Err(anyhow::anyhow!("Unknown SQS operation: {}", other)),
                    };
                    match outcome {
                        Ok(value) => ClientSendOutcome::Executed {
                            detail: format!(
                                "{} completed: {}",
                                name,
                                truncate_for_log(&value.to_string(), 200)
                            ),
                        },
                        Err(e) => {
                            error!("SQS client {} operation {} failed: {}", client_id, name, e);
                            let _ = status_tx
                                .send(format!("[ERROR] SQS operation {} failed: {}", name, e));
                            ClientSendOutcome::Executed {
                                detail: format!(
                                    "{} failed: {}",
                                    name,
                                    truncate_for_log(&e.to_string(), 200)
                                ),
                            }
                        }
                    }
                }
                ClientActionResult::Disconnect => ClientSendOutcome::Disconnected,
                ClientActionResult::WaitForMore => ClientSendOutcome::Executed {
                    detail: "wait_for_more".to_string(),
                },
                ClientActionResult::NoAction => ClientSendOutcome::Executed {
                    detail: "no_action".to_string(),
                },
                ClientActionResult::SendData(_) => ClientSendOutcome::Executed {
                    detail: "send_data has no meaning for the SQS client: it speaks the SQS API \
                             through the AWS SDK, not a socket this client owns"
                        .to_string(),
                },
                ClientActionResult::Multiple(_) => ClientSendOutcome::Executed {
                    detail:
                        "the SQS client's own verbs never produce Multiple; nothing was applied"
                            .to_string(),
                },
            }
        })
    }

    /// Drain injected commands until the channel closes (the client was removed) or
    /// an injected `disconnect` ends the session.
    ///
    /// `command_support::handle_stream_client_command` cannot serve this client:
    /// every SQS verb yields `ClientActionResult::Custom` and there is no write half
    /// to put bytes on. Actions therefore go through [`Self::apply_action`] - the
    /// same function the LLM path uses - and the outcome is logged and replied
    /// exactly the way the generic arm does it.
    #[allow(clippy::too_many_arguments)]
    async fn command_loop(
        mut command_rx: mpsc::Receiver<ClientCommand>,
        sqs: aws_sdk_sqs::Client,
        queue_url: String,
        protocol: Arc<SqsClientProtocol>,
        client_id: ClientId,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) {
        while let Some(command) = command_rx.recv().await {
            let action = command.action.clone();
            let outcome = match protocol.execute_action(action.clone()) {
                Err(e) => ClientSendOutcome::Rejected {
                    error: e.to_string(),
                },
                Ok(result) => {
                    Self::apply_action(
                        result,
                        &sqs,
                        &queue_url,
                        protocol.clone(),
                        &llm_client,
                        &app_state,
                        &status_tx,
                        client_id,
                    )
                    .await
                }
            };
            let disconnected = matches!(outcome, ClientSendOutcome::Disconnected);

            app_state
                .record_access_log(
                    AccessLogOwner::Client(client_id.as_u32()),
                    protocol.protocol_name(),
                    None,
                    "injected_action",
                    action,
                    vec![serde_json::to_value(&outcome).unwrap_or(serde_json::Value::Null)],
                )
                .await;
            let _ = status_tx.send("__UPDATE_UI__".to_string());
            crate::client::command_support::reply(command, Ok(outcome));

            if disconnected {
                info!("SQS client {} disconnecting on injected action", client_id);
                app_state
                    .update_client_status(client_id, ClientStatus::Disconnected)
                    .await;
                break;
            }
        }

        info!("SQS client {} command loop stopped", client_id);
        // Never leave the dashboard offering [ send ] into a client that is gone.
        app_state.remove_client_handle(client_id).await;
        let _ = status_tx.send("__UPDATE_UI__".to_string());
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
    ) -> Result<serde_json::Value> {
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

        let message_id = response.message_id().unwrap_or("unknown").to_string();
        info!("SQS client {} sent message: {}", client_id, message_id);

        // Raise the sent event (and run whatever the model answers with) from its own
        // registered task rather than inline. A dashboard-created client defaults to a
        // `*` -> manual routing rule, so this LLM call can park for minutes waiting for a
        // human; awaiting it here would wedge the command loop and make an injected action
        // that in fact succeeded look to the dashboard like a timeout.
        let event = Event::new(
            &SQS_MESSAGE_SENT_EVENT,
            serde_json::json!({
                "message_id": message_id,
            }),
        );
        Self::spawn_event_notification(
            event,
            sqs.clone(),
            queue_url.to_string(),
            protocol,
            llm_client.clone(),
            app_state.clone(),
            status_tx.clone(),
            client_id,
        )
        .await;

        Ok(serde_json::json!({ "message_id": message_id }))
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
    ) -> Result<serde_json::Value> {
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
        let message_count = messages.len();
        let received_ids: Vec<String> = messages
            .iter()
            .map(|m| m.message_id().unwrap_or("").to_string())
            .collect();
        info!(
            "SQS client {} received {} messages",
            client_id, message_count
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

            // Spawned for the same reason as in `send_message`: a manual routing rule can
            // park this call, and the command loop must not be held for it.
            Self::spawn_event_notification(
                event,
                sqs.clone(),
                queue_url.to_string(),
                protocol,
                llm_client.clone(),
                app_state.clone(),
                status_tx.clone(),
                client_id,
            )
            .await;
        }

        Ok(serde_json::json!({
            "count": message_count,
            "message_ids": received_ids,
        }))
    }

    /// Raise one client event off the caller's task, and run whatever the model answers
    /// with through the normal action path. Never awaited by a caller that holds the
    /// command loop: an event handler may park this call for a human answer.
    #[allow(clippy::too_many_arguments)]
    async fn spawn_event_notification(
        event: Event,
        sqs: aws_sdk_sqs::Client,
        queue_url: String,
        protocol: Arc<SqsClientProtocol>,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) {
        let registrar = app_state.clone();
        let handle = tokio::spawn(async move {
            let Some(instruction) = app_state.get_instruction_for_client(client_id).await else {
                return;
            };
            let memory = app_state
                .get_memory_for_client(client_id)
                .await
                .unwrap_or_default();

            let Ok(ClientLlmResult {
                actions,
                memory_updates,
            }) = call_llm_for_client(
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
            else {
                return;
            };

            if let Some(mem) = memory_updates {
                app_state.set_memory_for_client(client_id, mem).await;
            }

            if let Err(e) = Self::execute_actions(
                actions,
                &sqs,
                &queue_url,
                protocol,
                &llm_client,
                &app_state,
                &status_tx,
                client_id,
            )
            .await
            {
                error!("SQS client {} follow-up actions failed: {}", client_id, e);
            }
        });
        registrar.register_client_task(client_id, handle).await;
    }

    /// Delete a message from the queue
    async fn delete_message(
        sqs: &aws_sdk_sqs::Client,
        queue_url: &str,
        data: &serde_json::Value,
        client_id: ClientId,
    ) -> Result<serde_json::Value> {
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
        Ok(serde_json::json!({ "deleted": true }))
    }

    /// Purge all messages from the queue
    async fn purge_queue(
        sqs: &aws_sdk_sqs::Client,
        queue_url: &str,
        client_id: ClientId,
    ) -> Result<serde_json::Value> {
        sqs.purge_queue()
            .queue_url(queue_url)
            .send()
            .await
            .context("Failed to purge SQS queue")?;

        info!("SQS client {} purged queue", client_id);
        Ok(serde_json::json!({ "purged": true }))
    }

    /// Get queue attributes
    async fn get_queue_attributes(
        sqs: &aws_sdk_sqs::Client,
        queue_url: &str,
        data: &serde_json::Value,
        client_id: ClientId,
    ) -> Result<serde_json::Value> {
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

        let attributes: std::collections::HashMap<String, String> = response
            .attributes()
            .map(|attrs| {
                attrs
                    .iter()
                    .map(|(k, v)| (k.as_str().to_string(), v.clone()))
                    .collect()
            })
            .unwrap_or_default();
        info!(
            "SQS client {} got queue attributes: {:?}",
            client_id, attributes
        );
        Ok(serde_json::json!({ "attributes": attributes }))
    }
}
