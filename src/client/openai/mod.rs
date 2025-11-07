//! OpenAI client implementation
pub mod actions;

pub use actions::OpenAiClientProtocol;

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
use crate::client::openai::actions::OPENAI_CLIENT_RESPONSE_RECEIVED_EVENT;

/// OpenAI client that connects to the OpenAI API
pub struct OpenAiClient;

impl OpenAiClient {
    /// Connect to OpenAI API with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        _llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        startup_params: Option<StartupParams>,
    ) -> Result<SocketAddr> {
        // Extract API key from startup params
        let api_key = startup_params
            .as_ref()
            .map(|p| p.get_string("api_key"))
            .context("OpenAI API key is required")?;

        let default_model = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("default_model"))
            .unwrap_or_else(|| "gpt-3.5-turbo".to_string());

        let organization = startup_params
            .as_ref()
            .and_then(|p| p.get_optional_string("organization"));

        info!("OpenAI client {} initializing with API endpoint: {}", client_id, remote_addr);

        // Store configuration in protocol_data
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "api_key".to_string(),
                serde_json::json!(api_key),
            );
            client.set_protocol_field(
                "default_model".to_string(),
                serde_json::json!(default_model),
            );
            client.set_protocol_field(
                "api_endpoint".to_string(),
                serde_json::json!(remote_addr),
            );
            if let Some(org) = organization {
                client.set_protocol_field(
                    "organization".to_string(),
                    serde_json::json!(org),
                );
            }
        }).await;

        // Update status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] OpenAI client {} ready (endpoint: {})", client_id, remote_addr));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // For OpenAI client, we'll spawn a background task that monitors for client removal
        // The actual API requests are made on-demand via actions
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("OpenAI client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return a dummy local address (OpenAI is a remote API, not a local connection)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Make a chat completion request
    pub async fn make_chat_completion(
        client_id: ClientId,
        messages: serde_json::Value,
        model: Option<String>,
        temperature: Option<f64>,
        max_tokens: Option<u32>,
        functions: Option<serde_json::Value>,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get API configuration from client
        let (api_key, default_model, api_endpoint) = app_state.with_client_mut(client_id, |client| {
            let key = client.get_protocol_field("api_key")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let model = client.get_protocol_field("default_model")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let endpoint = client.get_protocol_field("api_endpoint")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            (key, model, endpoint)
        }).await.unwrap_or((None, None, None));

        let api_key = api_key.context("No API key found")?;
        let model_to_use = model.unwrap_or_else(|| default_model.unwrap_or_else(|| "gpt-3.5-turbo".to_string()));

        info!("OpenAI client {} making chat completion request with model: {}", client_id, model_to_use);

        // Build OpenAI client
        use async_openai::{Client as OpenAiApiClient, types::*};

        let mut config = async_openai::config::OpenAIConfig::new()
            .with_api_key(&api_key);

        // Override API base if custom endpoint is provided
        if let Some(endpoint) = api_endpoint {
            if !endpoint.is_empty() && endpoint != "https://api.openai.com/v1" {
                config = config.with_api_base(&endpoint);
            }
        }

        let openai_client = OpenAiApiClient::with_config(config);

        // Parse messages array
        let messages_array = messages.as_array()
            .context("Messages must be an array")?;

        let mut chat_messages = Vec::new();
        for msg in messages_array {
            let role = msg.get("role")
                .and_then(|v| v.as_str())
                .context("Message role is required")?;
            let content = msg.get("content")
                .and_then(|v| v.as_str())
                .context("Message content is required")?;

            let chat_message = match role {
                "system" => ChatCompletionRequestMessage::System(
                    ChatCompletionRequestSystemMessage {
                        content: ChatCompletionRequestSystemMessageContent::Text(content.to_string()),
                        name: None,
                    }
                ),
                "user" => ChatCompletionRequestMessage::User(
                    ChatCompletionRequestUserMessage {
                        content: ChatCompletionRequestUserMessageContent::Text(content.to_string()),
                        name: None,
                    }
                ),
                "assistant" => ChatCompletionRequestMessage::Assistant(
                    ChatCompletionRequestAssistantMessage {
                        content: Some(ChatCompletionRequestAssistantMessageContent::Text(content.to_string())),
                        name: None,
                        tool_calls: None,
                        refusal: None,
                        #[allow(deprecated)]
                        function_call: None,
                    }
                ),
                _ => return Err(anyhow::anyhow!("Unknown message role: {}", role)),
            };
            chat_messages.push(chat_message);
        }

        // Build request
        let mut request = CreateChatCompletionRequestArgs::default();
        request.model(&model_to_use);
        request.messages(chat_messages);

        if let Some(temp) = temperature {
            request.temperature(temp as f32);
        }

        if let Some(tokens) = max_tokens {
            request.max_tokens(tokens as u16);
        }

        // TODO: Add function calling support when needed
        if functions.is_some() {
            info!("OpenAI client {}: Function calling requested but not yet implemented", client_id);
        }

        let request = request.build()
            .context("Failed to build chat completion request")?;

        // Make request
        match openai_client.chat().create(request).await {
            Ok(response) => {
                let choice = response.choices.first()
                    .context("No choices in OpenAI response")?;

                let content = choice.message.content.as_ref()
                    .map(|s| s.to_string())
                    .unwrap_or_default();

                let usage = serde_json::json!({
                    "prompt_tokens": response.usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0),
                    "completion_tokens": response.usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0),
                    "total_tokens": response.usage.as_ref().map(|u| u.total_tokens).unwrap_or(0),
                });

                info!("OpenAI client {} received response ({} tokens)",
                    client_id,
                    response.usage.as_ref().map(|u| u.total_tokens).unwrap_or(0)
                );

                // Call LLM with response
                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                    let protocol = Arc::new(crate::client::openai::actions::OpenAiClientProtocol::new());
                    let event = Event::new(
                        &OPENAI_CLIENT_RESPONSE_RECEIVED_EVENT,
                        serde_json::json!({
                            "response_type": "chat_completion",
                            "content": content,
                            "model": response.model,
                            "usage": usage,
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
                            // Update memory
                            if let Some(mem) = memory_updates {
                                app_state.set_memory_for_client(client_id, mem).await;
                            }
                        }
                        Err(e) => {
                            error!("LLM error for OpenAI client {}: {}", client_id, e);
                        }
                    }
                }

                Ok(())
            }
            Err(e) => {
                error!("OpenAI client {} request failed: {}", client_id, e);
                let _ = status_tx.send(format!("[ERROR] OpenAI request failed: {}", e));

                // Send error event to LLM
                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                    let protocol = Arc::new(crate::client::openai::actions::OpenAiClientProtocol::new());
                    let event = Event::new(
                        &OPENAI_CLIENT_RESPONSE_RECEIVED_EVENT,
                        serde_json::json!({
                            "response_type": "error",
                            "content": e.to_string(),
                        }),
                    );

                    let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();
                    let _ = call_llm_for_client(
                        &llm_client,
                        &app_state,
                        client_id.to_string(),
                        &instruction,
                        &memory,
                        Some(&event),
                        protocol.as_ref(),
                        &status_tx,
                    ).await;
                }

                Err(e.into())
            }
        }
    }

    /// Make an embedding request
    pub async fn make_embedding_request(
        client_id: ClientId,
        input: serde_json::Value,
        model: Option<String>,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get API configuration from client
        let (api_key, api_endpoint) = app_state.with_client_mut(client_id, |client| {
            let key = client.get_protocol_field("api_key")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let endpoint = client.get_protocol_field("api_endpoint")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            (key, endpoint)
        }).await.unwrap_or((None, None));

        let api_key = api_key.context("No API key found")?;
        let model_to_use = model.unwrap_or_else(|| "text-embedding-ada-002".to_string());

        info!("OpenAI client {} making embedding request with model: {}", client_id, model_to_use);

        // Build OpenAI client
        use async_openai::{Client as OpenAiApiClient, types::*};

        let mut config = async_openai::config::OpenAIConfig::new()
            .with_api_key(&api_key);

        if let Some(endpoint) = api_endpoint {
            if !endpoint.is_empty() && endpoint != "https://api.openai.com/v1" {
                config = config.with_api_base(&endpoint);
            }
        }

        let openai_client = OpenAiApiClient::with_config(config);

        // Parse input (can be string or array)
        let input_value = if let Some(text) = input.as_str() {
            EmbeddingInput::String(text.to_string())
        } else if let Some(arr) = input.as_array() {
            let strings: Vec<String> = arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect();
            EmbeddingInput::StringArray(strings)
        } else {
            return Err(anyhow::anyhow!("Input must be a string or array of strings"));
        };

        // Build request
        let request = CreateEmbeddingRequestArgs::default()
            .model(&model_to_use)
            .input(input_value)
            .build()
            .context("Failed to build embedding request")?;

        // Make request
        match openai_client.embeddings().create(request).await {
            Ok(response) => {
                let embeddings: Vec<Vec<f32>> = response.data.iter()
                    .map(|e| e.embedding.clone())
                    .collect();

                let usage = serde_json::json!({
                    "prompt_tokens": response.usage.prompt_tokens,
                    "total_tokens": response.usage.total_tokens,
                });

                info!("OpenAI client {} received {} embeddings ({} tokens)",
                    client_id,
                    embeddings.len(),
                    response.usage.total_tokens
                );

                // Call LLM with response
                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                    let protocol = Arc::new(crate::client::openai::actions::OpenAiClientProtocol::new());
                    let event = Event::new(
                        &OPENAI_CLIENT_RESPONSE_RECEIVED_EVENT,
                        serde_json::json!({
                            "response_type": "embedding",
                            "content": format!("Generated {} embeddings", embeddings.len()),
                            "model": response.model,
                            "usage": usage,
                            "embeddings_count": embeddings.len(),
                            "embedding_dimensions": embeddings.first().map(|e| e.len()).unwrap_or(0),
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
                            // Update memory
                            if let Some(mem) = memory_updates {
                                app_state.set_memory_for_client(client_id, mem).await;
                            }
                        }
                        Err(e) => {
                            error!("LLM error for OpenAI client {}: {}", client_id, e);
                        }
                    }
                }

                Ok(())
            }
            Err(e) => {
                error!("OpenAI client {} embedding request failed: {}", client_id, e);
                let _ = status_tx.send(format!("[ERROR] OpenAI embedding request failed: {}", e));
                Err(e.into())
            }
        }
    }
}
