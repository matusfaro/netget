//! LLM action helper - simplified API for action-based LLM calls
//!
//! This module provides a centralized helper for all LLM interactions.
//! It encapsulates the common pattern of:
//! 1. Building prompt with actions
//! 2. Calling LLM
//! 3. Parsing action response
//! 4. Executing actions
//!
//! USE THIS HELPER FOR ALL LLM CALLS. Do not call OllamaClient.generate() directly.

use crate::llm::actions::{
    executor::{execute_actions, ExecutionResult},
    get_network_event_common_actions,
    protocol_trait::Server,
    ActionDefinition,
};
use crate::llm::ollama_client::OllamaClient;
use crate::llm::prompt::PromptBuilder;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::ServerId;
use anyhow::{Context as AnyhowContext, Result};
use std::sync::Arc;
use tracing::{debug, warn};

/// Call LLM with action-based framework
///
/// This is the PRIMARY way to interact with the LLM. It handles:
/// - Multi-turn conversation with tool calling
/// - Prompt building with action definitions
/// - LLM API call with message history
/// - Response parsing
/// - Action execution
///
/// # Arguments
/// * `llm_client` - Ollama client instance
/// * `state` - Application state for context
/// * `server_id` - Server ID for context
/// * `connection_id` - Optional connection ID for context (for scripts)
/// * `event_description` - High-level description of the event (e.g., "NFS lookup requested")
/// * `context_json` - Structured context data for the prompt
/// * `protocol` - Optional protocol for protocol-specific sync actions
/// * `custom_actions` - Additional custom actions specific to this call
/// * `event_data` - Optional structured event data for scripts
///
/// # Returns
/// * `Ok(ExecutionResult)` - Results containing messages and protocol-specific results
/// * `Err(_)` - If LLM call or action execution failed
///
/// # Example
/// ```rust,ignore
/// // NFS lookup example
/// let params = json!({
///     "operation": "lookup",
///     "path": "/home/user/file.txt",
///     "parent_id": 1
/// });
///
/// let result = call_llm_with_actions(
///     &llm_client,
///     &state,
///     server_id,
///     "NFS lookup operation requested",
///     params,
///     Some(&nfs_protocol),
///     vec![],
/// ).await?;
/// ```
#[allow(clippy::too_many_arguments)]
pub async fn call_llm_with_actions(
    llm_client: &OllamaClient,
    state: &AppState,
    server_id: ServerId,
    connection_id: Option<crate::server::connection::ConnectionId>,
    event_description: &str,
    context_json: serde_json::Value,
    protocol: Option<&dyn Server>,
    custom_actions: Vec<ActionDefinition>,
    event_data: Option<serde_json::Value>,
) -> Result<ExecutionResult> {
    // NOTE: Easy protocol handling is done in call_llm() since it requires an Event object
    // This function (call_llm_with_actions) is for legacy code paths that don't have Event objects

    // TRY EVENT HANDLER FIRST if configured
    let event_type_id = crate::scripting::ScriptManager::extract_context_type(event_description);

    match crate::llm::event_handler_executor::try_execute_event_handler(
        state,
        server_id,
        connection_id,
        &event_type_id,
        event_description,
        event_data.clone(),
        protocol,
    )
    .await?
    {
        crate::llm::event_handler_executor::EventHandlerResult::Handled(result) => {
            // Handler executed successfully (script or static)
            return Ok(result);
        }
        crate::llm::event_handler_executor::EventHandlerResult::FallbackToLlm => {
            // No handler or handler requested LLM fallback - proceed with LLM call
        }
    }

    // FALLBACK TO LLM (normal path if no handler or handler requested fallback)

    // Get model from state, auto-select if not set
    let model = crate::llm::ensure_model_selected(state.get_ollama_model().await)
        .await
        .context("Failed to ensure model is selected")?;

    // Collect all actions: common + protocol sync + custom
    let mut all_actions = get_network_event_common_actions();

    // Add provide_feedback action only if server has feedback_instructions configured
    let has_feedback_instructions = state
        .with_server_mut(server_id, |server| server.feedback_instructions.is_some())
        .await
        .unwrap_or(false);

    if has_feedback_instructions {
        all_actions.push(crate::llm::actions::common::provide_feedback_action());
    }

    // Add protocol sync actions if provided
    if let Some(proto) = protocol {
        all_actions.extend(proto.get_sync_actions());
    }

    // Add custom actions (these can override or augment the standard actions)
    all_actions.extend(custom_actions);

    debug!(
        "LLM call for event: {} (server #{}, {} actions available)",
        event_description,
        server_id.as_u32(),
        all_actions.len()
    );

    // Build system prompt using action system (NO trigger - that goes in user message)
    let system_prompt = PromptBuilder::build_network_event_action_prompt_for_server(
        state,
        server_id,
        all_actions.clone(),
    )
    .await;

    // Create conversation handler for network event with tracking
    let truncated_desc = if event_description.len() > 30 {
        format!("LLM \"{}...\"", &event_description[..27])
    } else {
        format!("LLM \"{}\"", event_description)
    };

    // Get rate limiter for network events (discards if rate limited)
    let rate_limiter = state.get_rate_limiter().await;

    let mut conversation = crate::llm::ConversationHandler::new(
        system_prompt,
        std::sync::Arc::new(llm_client.clone()),
        model,
        rate_limiter,
        crate::llm::RequestSource::Network, // Network events are discarded if rate limited
    )
    .with_tracking(
        state.clone(),
        crate::state::app_state::ConversationSource::Network {
            server_id,
            connection_id,
        },
        truncated_desc,
    );

    // Add event trigger as a user message
    let event_trigger =
        PromptBuilder::build_event_trigger_message(event_description, context_json.clone());
    conversation.add_user_message(event_trigger);

    // Get web search mode and approval channel
    let web_search_mode = state.get_web_search_mode().await;
    let approval_tx = state.get_web_approval_channel().await;

    // Generate actions with tool calling and retry
    let action_values = conversation
        .generate_with_tools_and_retry(approval_tx, web_search_mode, all_actions.clone())
        .await
        .context("LLM generate with tools failed")?;

    if action_values.is_empty() {
        warn!(
            "LLM returned empty actions array for event: {}",
            event_description
        );
    }

    // Execute all collected actions with server context
    let result = execute_actions(action_values, state, protocol, Some(server_id), None)
        .await
        .context("Failed to execute actions")?;

    debug!(
        "LLM call completed: {} messages, {} protocol results",
        result.messages.len(),
        result.protocol_results.len()
    );

    Ok(result)
}

/// Simplified variant when no custom actions or context needed
///
/// This is useful when you just want to use the standard protocol actions
/// without adding any custom behavior or structured context.
pub async fn call_llm_with_protocol(
    llm_client: &OllamaClient,
    state: &AppState,
    server_id: ServerId,
    connection_id: Option<crate::server::connection::ConnectionId>,
    event_description: &str,
    protocol: &dyn Server,
) -> Result<ExecutionResult> {
    call_llm_with_actions(
        llm_client,
        state,
        server_id,
        connection_id,
        event_description,
        serde_json::json!({}), // Empty context
        Some(protocol),
        Vec::new(), // No custom actions
        None,       // No custom event data
    )
    .await
}

/// Simplified variant for custom actions only (no protocol or context)
///
/// This is useful for special cases like authentication decisions
/// where you need a custom action but no protocol-specific actions.
pub async fn call_llm_with_custom_actions(
    llm_client: &OllamaClient,
    state: &AppState,
    server_id: ServerId,
    connection_id: Option<crate::server::connection::ConnectionId>,
    event_description: &str,
    custom_actions: Vec<ActionDefinition>,
) -> Result<ExecutionResult> {
    call_llm_with_actions(
        llm_client,
        state,
        server_id,
        connection_id,
        event_description,
        serde_json::json!({}), // Empty context
        None,
        custom_actions,
        None, // No custom event data
    )
    .await
}

/// NEW EVENT-DRIVEN API: Call LLM with Event
///
/// This is the PREFERRED way to call the LLM for protocol events.
/// You pass an Event which combines:
/// - EventType reference (event ID, description, available actions)
/// - Event data (actual context like username, path, command)
///
/// # Arguments
/// * `llm_client` - Ollama client instance
/// * `state` - Application state for context
/// * `server_id` - Server ID for context
/// * `connection_id` - Optional connection ID for context (for scripts)
/// * `event` - The Event instance (EventType + data)
/// * `protocol` - Protocol for executing protocol-specific actions
///
/// # Returns
/// * `Ok(ExecutionResult)` - Results containing messages and protocol-specific results
/// * `Err(_)` - If LLM call or action execution failed
///
/// # Example
/// ```rust,ignore
/// let event = Event::new(
///     &HTTP_REQUEST_EVENT,
///     json!({
///         "method": "GET",
///         "path": "/api/users"
///     })
/// );
///
/// let result = call_llm(
///     &llm_client,
///     &state,
///     server_id,
///     Some(connection_id),
///     &event,
///     &http_protocol,
/// ).await?;
/// ```
pub async fn call_llm(
    llm_client: &OllamaClient,
    state: &AppState,
    server_id: ServerId,
    connection_id: Option<crate::server::connection::ConnectionId>,
    event: &Event,
    protocol: &dyn Server,
) -> Result<ExecutionResult> {
    // TRY EASY PROTOCOL HANDLER FIRST if this server is managed by an easy protocol
    if let Some(easy_id) = state.get_easy_for_server(server_id).await {
        use crate::protocol::EASY_REGISTRY;
        if let Some(easy_instance) = state.get_easy_instance(easy_id).await {
            if let Some(easy_protocol) = EASY_REGISTRY.get_by_name(&easy_instance.protocol_name) {
                // Call Easy protocol handler
                let actions = easy_protocol
                    .handle_event(
                        event.clone(),
                        easy_instance.user_instruction.clone(),
                        Arc::new(llm_client.clone()),
                        Arc::new(state.clone()),
                    )
                    .await
                    .context("Easy protocol handler failed")?;

                // Execute actions and return result
                let result = crate::llm::execute_actions(
                    actions,
                    state,
                    Some(protocol),
                    Some(server_id),
                    None, // client_id
                )
                .await?;

                return Ok(result);
            }
        }
    }

    // TRY EVENT HANDLER FIRST if configured (includes scripts and static responses)
    match crate::llm::event_handler_executor::try_execute_event_handler(
        state,
        server_id,
        connection_id,
        &event.event_type.id,
        &event.event_type.description,
        Some(event.data.clone()),
        Some(protocol),
    )
    .await?
    {
        crate::llm::event_handler_executor::EventHandlerResult::Handled(result) => {
            // Handler executed successfully (script or static)
            return Ok(result);
        }
        crate::llm::event_handler_executor::EventHandlerResult::FallbackToLlm => {
            // No handler or handler requested LLM fallback - proceed with LLM call
        }
    }

    // FALLBACK TO LLM (normal path if no script or script failed/requested fallback)

    // Get model from state, auto-select if not set
    let model = crate::llm::ensure_model_selected(state.get_ollama_model().await)
        .await
        .context("Failed to ensure model is selected")?;

    // Collect all actions: common + event-specific actions
    let mut all_actions = get_network_event_common_actions();

    // Add provide_feedback action only if server has feedback_instructions configured
    let has_feedback_instructions = state
        .with_server_mut(server_id, |server| server.feedback_instructions.is_some())
        .await
        .unwrap_or(false);

    if has_feedback_instructions {
        all_actions.push(crate::llm::actions::common::provide_feedback_action());
    }

    // Add event-specific actions (these are the actions available for this event type)
    all_actions.extend(event.event_type.actions.clone());

    debug!(
        "LLM call for event '{}' (server #{}, {} actions available)",
        event.id(),
        server_id.as_u32(),
        all_actions.len()
    );

    // Use the event's prompt description
    let event_description = event.to_prompt_description();

    // Build system prompt using action system (NO trigger - that goes in user message)
    let system_prompt = PromptBuilder::build_network_event_action_prompt_for_server(
        state,
        server_id,
        all_actions.clone(),
    )
    .await;

    // Create conversation handler for network event with tracking
    // Note: Network events don't use tools (immediate response), but get retry logic
    let truncated_desc = if event_description.len() > 30 {
        format!("LLM \"{}...\"", &event_description[..27])
    } else {
        format!("LLM \"{}\"", event_description)
    };

    // Get rate limiter for network events (discards if rate limited)
    let rate_limiter = state.get_rate_limiter().await;

    let mut conversation = crate::llm::ConversationHandler::new(
        system_prompt,
        std::sync::Arc::new(llm_client.clone()),
        model,
        rate_limiter,
        crate::llm::RequestSource::Network, // Network events are discarded if rate limited
    )
    .with_tracking(
        state.clone(),
        crate::state::app_state::ConversationSource::Network {
            server_id,
            connection_id,
        },
        truncated_desc,
    );

    // Add event trigger as a user message (include event ID for mock testing compatibility)
    let event_trigger = PromptBuilder::build_event_trigger_message_with_id(
        event.id(),
        &event_description,
        event.data.clone(),
    );
    conversation.add_user_message(event_trigger);

    // Generate response with retry (no tool calling for network events)
    let actions = conversation
        .generate_with_tools_and_retry(
            None, // No web approval for network events
            crate::state::app_state::WebSearchMode::Off, // No web search for network events
            all_actions,
        )
        .await
        .context("✗  LLM failed to generate valid response after retries.\n   This may indicate:\n   1. Ollama is not running or not accessible\n   2. Model is not available or not loaded\n   3. Network/connection issues\n   \n   Use `/model` to check and select an available model")?;

    if actions.is_empty() {
        warn!("LLM returned empty actions array for event: {}", event.id());
    }

    // Execute actions with server context
    let result = execute_actions(actions, state, Some(protocol), Some(server_id), None)
        .await
        .context("Failed to execute actions")?;

    debug!(
        "LLM call completed: {} messages, {} protocol results",
        result.messages.len(),
        result.protocol_results.len()
    );

    Ok(result)
}
/// Call LLM for client protocol events (simplified version for MVP)
/// Result from client LLM call
#[derive(Debug, Clone)]
pub struct ClientLlmResult {
    pub actions: Vec<serde_json::Value>,
    pub memory_updates: Option<String>,
}

/// Call LLM for client protocol events (simplified version for MVP)
///
/// This is a simplified version of call_llm for client protocols.
/// Unlike servers, clients don't have complex scripting or connection tracking.
#[allow(clippy::too_many_arguments)]
pub async fn call_llm_for_client(
    llm_client: &OllamaClient,
    state: &AppState,
    client_id: String,
    instruction: &str,
    memory: &str,
    event: Option<&Event>,
    protocol: &dyn crate::llm::actions::client_trait::Client,
    status_tx: &tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<ClientLlmResult> {
    // Get client actions
    let mut all_actions = protocol.get_async_actions(state);

    // Add provide_feedback action only if client has feedback_instructions configured
    // Parse client_id from string format "client-123"
    if let Some(cid) = crate::state::ClientId::from_string(&client_id) {
        let has_feedback_instructions = state
            .with_client_mut(cid, |client| client.feedback_instructions.is_some())
            .await
            .unwrap_or(false);

        if has_feedback_instructions {
            all_actions.push(crate::llm::actions::common::provide_feedback_action());
        }
    }

    // Build simple prompt for client
    let system_prompt =
        format!(
        "You are controlling a network client ({}). Your instruction: {}\n\nAvailable actions:\n{}",
        protocol.protocol_name(),
        instruction,
        all_actions.iter().map(|a| a.to_prompt_text()).collect::<Vec<_>>().join("\n\n")
    );

    // Build user message
    let user_message = if let Some(ev) = event {
        format!(
            "Event: {}\nData: {}",
            ev.id(),
            serde_json::to_string_pretty(&ev.data).unwrap_or_default()
        )
    } else {
        "Waiting for instructions".to_string()
    };

    // Add memory context if present
    let full_message = if !memory.is_empty() {
        format!("Memory: {}\n\n{}", memory, user_message)
    } else {
        user_message
    };

    // Get current model from state, auto-select if not set
    let current_model = state.get_ollama_model().await;
    let model = crate::llm::ensure_model_selected(current_model.clone())
        .await
        .context("Failed to ensure model is selected")?;

    // If model was auto-selected (wasn't set before), notify via status_tx
    if current_model.is_none() {
        let _ = status_tx.send(format!(
            "⚠  Auto-selected model: {} (no model was configured)",
            model
        ));
    }

    // Get rate limiter for client calls (network-like, discards if rate limited)
    let rate_limiter = state.get_rate_limiter().await;

    // Create conversation with correct parameter order
    let mut conversation = crate::llm::ConversationHandler::new(
        system_prompt,
        std::sync::Arc::new(llm_client.clone()),
        model,
        rate_limiter,
        crate::llm::RequestSource::Network, // Client calls are network-initiated, discarded if rate limited
    )
    .with_status_tx(status_tx.clone());

    // Add user message
    conversation.add_user_message(full_message);

    // Generate response with actions (no web approval or tools for clients)
    let actions = conversation
        .generate_with_tools_and_retry(
            None,
            crate::state::app_state::WebSearchMode::Off,
            all_actions,
        )
        .await?;

    // For now, memory updates are not extracted from client responses
    // This can be enhanced later if needed
    let memory_updates = None;

    Ok(ClientLlmResult {
        actions,
        memory_updates,
    })
}

/// Call LLM for feedback processing (server or client adjustment)
///
/// This is invoked when feedback has accumulated for a server/client with feedback_instructions.
/// The LLM analyzes the feedback and returns actions to adjust the instance behavior.
///
/// # Arguments
/// * `llm_client` - Ollama client for LLM invocations
/// * `state` - Application state
/// * `server_id` - Server ID if processing server feedback
/// * `client_id` - Client ID if processing client feedback
/// * `feedback_instructions` - Instructions for how to process feedback
/// * `current_instruction` - Current instruction of the instance
/// * `memory` - Current memory of the instance
/// * `feedback_entries` - Accumulated feedback entries
/// * `status_tx` - Channel for status messages
///
/// # Returns
/// * `Ok(Vec<serde_json::Value>)` - Actions to adjust the instance
/// * `Err(_)` - If LLM invocation fails
#[allow(clippy::too_many_arguments)]
pub async fn call_llm_for_feedback(
    llm_client: &OllamaClient,
    state: &AppState,
    server_id: Option<crate::state::ServerId>,
    client_id: Option<crate::state::ClientId>,
    feedback_instructions: &str,
    current_instruction: &str,
    memory: &str,
    feedback_entries: &[serde_json::Value],
    status_tx: &tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<Vec<serde_json::Value>> {
    use crate::llm::actions::get_user_input_common_actions;
    use crate::llm::prompt::PromptBuilder;

    // Get available adjustment actions (user input actions for modifying server/client)
    let selected_mode = state.get_selected_scripting_mode().await;
    let scripting_env = state.get_scripting_env().await;
    let is_open_server_enabled = true;
    let is_open_client_enabled = true;
    let available_actions = get_user_input_common_actions(
        selected_mode,
        &scripting_env,
        is_open_server_enabled,
        is_open_client_enabled,
    );

    // Build feedback processing prompt
    let system_prompt = PromptBuilder::build_feedback_system_prompt(
        state,
        server_id,
        client_id,
        feedback_instructions,
        current_instruction,
        memory,
        feedback_entries,
        available_actions,
    )
    .await;

    // Get current model from state, auto-select if not set
    let current_model = state.get_ollama_model().await;
    let model = crate::llm::ensure_model_selected(current_model.clone())
        .await
        .context("Failed to ensure model is selected")?;

    // If model was auto-selected, notify via status_tx
    if current_model.is_none() {
        let _ = status_tx.send(format!(
            "⚠  Auto-selected model: {} (no model was configured)",
            model
        ));
    }

    let instance_type = if server_id.is_some() {
        "server"
    } else {
        "client"
    };
    let instance_id = server_id
        .map(|id| id.as_u32())
        .or_else(|| client_id.map(|id| id.as_u32()))
        .unwrap_or(0);

    debug!(
        "LLM feedback processing for {} #{} ({} feedback entries)",
        instance_type,
        instance_id,
        feedback_entries.len()
    );

    // Get rate limiter for feedback processing (user-initiated, should not be discarded)
    let rate_limiter = state.get_rate_limiter().await;

    // Create conversation handler with tracking
    let conversation_source = if let Some(sid) = server_id {
        crate::state::app_state::ConversationSource::Network {
            server_id: sid,
            connection_id: None,
        }
    } else {
        // Client feedback source (use Task as placeholder since we don't have a Client variant yet)
        crate::state::app_state::ConversationSource::Task {
            task_name: format!("feedback-client-{}", instance_id),
        }
    };

    let mut conversation = crate::llm::ConversationHandler::new(
        system_prompt,
        std::sync::Arc::new(llm_client.clone()),
        model,
        rate_limiter,
        crate::llm::RequestSource::User, // Feedback is user-initiated (via debounce timer)
    )
    .with_status_tx(status_tx.clone())
    .with_tracking(
        state.clone(),
        conversation_source,
        format!(
            "Feedback processing ({} entries)",
            feedback_entries.len()
        ),
    );

    // Add user message to trigger feedback processing
    conversation.add_user_message("Analyze the accumulated feedback and suggest adjustments.".to_string());

    // Generate actions with retry (no tools for feedback processing)
    let web_search_mode = state.get_web_search_mode().await;
    let actions = conversation
        .generate_with_tools_and_retry(
            state.get_web_approval_channel().await,
            web_search_mode,
            Vec::new(), // No additional actions
        )
        .await
        .context("✗  LLM failed to generate feedback processing response after retries")?;

    if actions.is_empty() {
        warn!(
            "LLM returned empty actions for {} #{} feedback processing (no adjustments needed)",
            instance_type, instance_id
        );
    } else {
        debug!(
            "LLM feedback processing completed for {} #{}: {} adjustment actions",
            instance_type,
            instance_id,
            actions.len()
        );
    }

    Ok(actions)
}
