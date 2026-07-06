//! Sampling LLM backend for MCP STDIO mode
//!
//! When the MCP client supports sampling, this backend routes LLM calls
//! through MCP's sampling/createMessage API instead of local Ollama/OpenAI.
//! This allows the MCP client's LLM (e.g., Claude) to directly control
//! network protocol servers.

use std::sync::Arc;

use crate::llm::ollama_client::{ChatRequest, ChatResponse, Message, TokenUsage, ToolCall};
use anyhow::{Context, Result};
use rmcp::model::{
    CreateMessageRequest, CreateMessageRequestParams, Role, SamplingContent, SamplingMessage,
    SamplingMessageContent, ServerRequest,
};
use rmcp::service::{Peer, RoleServer};
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::{debug, error, info, trace};

/// A sampling request sent from OllamaClient to the MCP STDIO transport
#[derive(Debug)]
pub struct SamplingRequest {
    /// Unique request ID for matching responses
    pub id: u64,
    /// Conversation messages (system, user, assistant, tool)
    pub messages: Vec<Message>,
    /// Tool schemas for the LLM to use
    pub tools: Vec<serde_json::Value>,
    /// Maximum tokens to generate
    pub max_tokens: u32,
    /// Channel to send the response back
    pub response_tx: oneshot::Sender<Result<SamplingResponse>>,
}

/// A sampling response received from the MCP client
#[derive(Debug, Clone)]
pub struct SamplingResponse {
    /// Text content from the LLM
    pub content: Option<String>,
    /// Tool calls from the LLM (if supported by MCP client)
    pub tool_calls: Vec<ToolCall>,
    /// Model that was used
    pub model: String,
    /// Stop reason
    pub stop_reason: String,
}

/// Spawn the background task that forwards sampling requests to the MCP client.
///
/// The task reads the peer from `peer_slot` on every request, so it always targets
/// the most recently initialized client. This supports client reconnects (STDIO)
/// and multiple sessions sharing one state (HTTP transport). Requests are processed
/// serially - MCP clients are not required to handle concurrent sampling requests.
pub fn spawn_sampling_forwarder(
    peer_slot: Arc<Mutex<Option<Peer<RoleServer>>>>,
    mut rx: mpsc::UnboundedReceiver<SamplingRequest>,
) {
    tokio::spawn(async move {
        info!("Sampling forwarder started");
        while let Some(request) = rx.recv().await {
            let peer = peer_slot.lock().await.clone();
            let Some(peer) = peer else {
                let _ = request.response_tx.send(Err(anyhow::anyhow!(
                    "No MCP client connected - cannot forward sampling request"
                )));
                continue;
            };

            let SamplingRequest {
                id,
                messages,
                tools,
                max_tokens,
                response_tx,
            } = request;
            debug!("Forwarding sampling request #{}", id);

            // Convert to MCP sampling format
            let params = chat_request_to_sampling_params(&ChatRequest {
                messages,
                tools,
                model: String::new(), // MCP client chooses the model
            });

            // Build SamplingMessage list from params
            let mcp_messages: Vec<SamplingMessage> = params["messages"]
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .filter_map(|m| {
                    let role = match m["role"].as_str()? {
                        "user" => Role::User,
                        "assistant" => Role::Assistant,
                        _ => return None,
                    };
                    let text = m["content"]["text"].as_str()?.to_string();
                    Some(SamplingMessage::new(role, SamplingMessageContent::text(text)))
                })
                .collect();

            let mut create_params = CreateMessageRequestParams::new(mcp_messages, max_tokens);
            create_params.system_prompt = params["systemPrompt"].as_str().map(|s| s.to_string());

            let mcp_request =
                ServerRequest::CreateMessageRequest(CreateMessageRequest::new(create_params));
            match peer.send_request(mcp_request).await {
                Ok(rmcp::model::ClientResult::CreateMessageResult(msg_result)) => {
                    let content = match &msg_result.message.content {
                        SamplingContent::Single(SamplingMessageContent::Text(t)) => {
                            Some(t.text.clone())
                        }
                        _ => None,
                    };
                    let response = SamplingResponse {
                        content,
                        tool_calls: Vec::new(), // MCP sampling does not return native tool calls
                        model: msg_result.model.clone(),
                        stop_reason: msg_result.stop_reason.clone().unwrap_or_default(),
                    };
                    let _ = response_tx.send(Ok(response));
                }
                Ok(_) => {
                    let _ = response_tx.send(Err(anyhow::anyhow!(
                        "Unexpected response type from MCP client"
                    )));
                }
                Err(e) => {
                    error!("Sampling request failed: {}", e);
                    let _ = response_tx.send(Err(anyhow::anyhow!(
                        "Sampling request to MCP client failed: {}",
                        e
                    )));
                }
            }
        }
        info!("Sampling forwarder stopped");
    });
}

/// Convert a ChatRequest into MCP sampling format
///
/// The MCP sampling/createMessage request format:
/// ```json
/// {
///   "messages": [{ "role": "user", "content": { "type": "text", "text": "..." } }],
///   "systemPrompt": "...",
///   "maxTokens": 4096
/// }
/// ```
pub fn chat_request_to_sampling_params(request: &ChatRequest) -> serde_json::Value {
    let mut system_prompt = None;
    let mut messages = Vec::new();

    for msg in &request.messages {
        match msg.role.as_str() {
            "system" => {
                // MCP sampling has a separate systemPrompt field
                system_prompt = Some(msg.content.clone());
            }
            "user" | "assistant" => {
                messages.push(serde_json::json!({
                    "role": msg.role,
                    "content": {
                        "type": "text",
                        "text": msg.content,
                    }
                }));
            }
            "tool" => {
                // Tool results are sent as user messages with tool context
                messages.push(serde_json::json!({
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": format!("[Tool Result{}]: {}",
                            msg.tool_call_id.as_ref().map(|id| format!(" ({})", id)).unwrap_or_default(),
                            msg.content
                        ),
                    }
                }));
            }
            _ => {
                debug!("Skipping unknown message role: {}", msg.role);
            }
        }
    }

    let mut params = serde_json::json!({
        "messages": messages,
        "maxTokens": 4096,
    });

    if let Some(system) = system_prompt {
        params["systemPrompt"] = serde_json::Value::String(system);
    }

    // Include tools if available (MCP sampling supports tool calling)
    if !request.tools.is_empty() {
        // Convert from OpenAI format to MCP format
        let mcp_tools: Vec<serde_json::Value> = request
            .tools
            .iter()
            .filter_map(|tool| {
                let function = tool.get("function")?;
                Some(serde_json::json!({
                    "name": function.get("name")?,
                    "description": function.get("description").unwrap_or(&serde_json::Value::Null),
                    "inputSchema": function.get("parameters").unwrap_or(&serde_json::json!({"type": "object"})),
                }))
            })
            .collect();

        if !mcp_tools.is_empty() {
            params["tools"] = serde_json::Value::Array(mcp_tools);
        }
    }

    params
}

/// Convert a SamplingResponse into a ChatResponse
pub fn sampling_response_to_chat_response(response: SamplingResponse) -> ChatResponse {
    ChatResponse {
        content: response.content,
        tool_calls: response.tool_calls,
        token_usage: TokenUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        },
    }
}

/// Handle a sampling request by forwarding it through the channel
///
/// This is called from OllamaClient when the backend is Sampling.
/// It sends the request through the channel and waits for the response.
pub async fn execute_sampling_request(
    request_tx: &mpsc::UnboundedSender<SamplingRequest>,
    chat_request: &ChatRequest,
) -> Result<ChatResponse> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let (response_tx, response_rx) = oneshot::channel();

    let sampling_request = SamplingRequest {
        id,
        messages: chat_request.messages.clone(),
        tools: chat_request.tools.clone(),
        max_tokens: 4096,
        response_tx,
    };

    trace!("Sending sampling request #{}", id);

    request_tx
        .send(sampling_request)
        .map_err(|_| anyhow::anyhow!("Sampling channel closed - MCP client disconnected"))?;

    // Wait for response with timeout
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        response_rx,
    )
    .await
    .context("Sampling request timed out after 120 seconds")?
    .context("Sampling response channel closed")?
    .context("Sampling request failed")?;

    debug!(
        "Received sampling response #{}: content={}, tool_calls={}",
        id,
        response.content.as_ref().map(|c| c.len()).unwrap_or(0),
        response.tool_calls.len()
    );

    Ok(sampling_response_to_chat_response(response))
}
