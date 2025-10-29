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
    protocol_trait::ProtocolActions,
    ActionDefinition, ActionResponse,
};
use crate::llm::ollama_client::OllamaClient;
use crate::llm::prompt::PromptBuilder;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::ServerId;
use anyhow::{Context as AnyhowContext, Result};
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
pub async fn call_llm_with_actions(
    llm_client: &OllamaClient,
    state: &AppState,
    server_id: ServerId,
    connection_id: Option<crate::network::connection::ConnectionId>,
    event_description: &str,
    context_json: serde_json::Value,
    protocol: Option<&dyn ProtocolActions>,
    custom_actions: Vec<ActionDefinition>,
    event_data: Option<serde_json::Value>,
) -> Result<ExecutionResult> {
    // TRY SCRIPT FIRST if configured
    let script_config = state.get_script_config(server_id).await;
    if let Some(ref config) = script_config {
        // Extract context type from event description
        let context_type = crate::scripting::ScriptManager::extract_context_type(event_description);

        // Check if script handles this context
        if config.handles_context(&context_type) {
            // Get server info to build script input
            let server_info = state.get_server(server_id).await;

            if let Some(server) = server_info {
                // Build connection context if available
                let connection_context = if let Some(conn_id) = connection_id {
                    // Try to get connection info from server
                    server.connections.get(&conn_id).map(|conn_state| {
                        crate::scripting::types::ConnectionContext {
                            id: conn_id.to_string(),
                            remote_addr: conn_state.remote_addr.to_string(),
                            bytes_received: conn_state.bytes_received,
                            bytes_sent: conn_state.bytes_sent,
                        }
                    })
                } else {
                    None
                };

                // Build structured input for script
                let event_json = event_data.clone().unwrap_or_else(|| {
                    serde_json::json!({"description": event_description})
                });

                let script_input = crate::scripting::types::ScriptInput {
                    context_type: context_type.clone(),
                    server: crate::scripting::types::ServerContext {
                        id: server.id.as_u32(),
                        port: server.port,
                        stack: server.base_stack.name().to_string(),
                        memory: server.memory.clone(),
                        instruction: server.instruction.clone(),
                    },
                    connection: connection_context,
                    event: event_json,
                };

                // Try to execute the script
                match crate::scripting::ScriptManager::try_execute(Some(config), &script_input) {
                    Ok(Some(script_response)) => {
                        // Script handled it successfully!
                        debug!(
                            "Script handled event (context: {}, {} actions)",
                            context_type,
                            script_response.actions.len()
                        );

                        // Execute the script's actions
                        let result = execute_actions(
                            script_response.actions,
                            state,
                            protocol,
                        )
                        .await
                        .context("Failed to execute script actions")?;

                        return Ok(result);
                    }
                    Ok(None) => {
                        // Script requested fallback or doesn't handle this context
                        debug!("Script returned None, falling back to LLM");
                    }
                    Err(e) => {
                        // Script execution failed, fall back to LLM
                        warn!("Script execution failed ({}), falling back to LLM", e);
                    }
                }
            }
        } else {
            debug!(
                "Script does not handle context '{}', using LLM",
                context_type
            );
        }
    }

    // FALLBACK TO LLM (normal path if no script or script failed/requested fallback)

    // Get model from state
    let model = state.get_ollama_model().await;

    // Collect all actions: common + protocol sync + custom
    let mut all_actions = get_network_event_common_actions();

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

    // Use multi-turn generation with tools and message-based context
    let state_clone = state.clone();
    let event_desc = event_description.to_string();
    let context_clone = context_json.clone();
    let actions_clone = all_actions.clone();

    let action_values = llm_client
        .generate_with_tools(
            &model,
            || {
                let state = state_clone.clone();
                let event_description = event_desc.clone();
                let context = context_clone.clone();
                let all_actions = actions_clone.clone();
                async move {
                    PromptBuilder::build_network_event_action_prompt_for_server(
                        &state,
                        server_id,
                        &event_description,
                        context,
                        all_actions,
                    )
                    .await
                }
            },
            5, // max 5 iterations
        )
        .await
        .context("LLM generate with tools failed")?;

    if action_values.is_empty() {
        warn!(
            "LLM returned empty actions array for event: {}",
            event_description
        );
    }

    // Execute all collected actions
    let result = execute_actions(action_values, state, protocol)
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
    connection_id: Option<crate::network::connection::ConnectionId>,
    event_description: &str,
    protocol: &dyn ProtocolActions,
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
        None, // No custom event data
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
    connection_id: Option<crate::network::connection::ConnectionId>,
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
    connection_id: Option<crate::network::connection::ConnectionId>,
    event: &Event,
    protocol: &dyn ProtocolActions,
) -> Result<ExecutionResult> {
    // TRY SCRIPT FIRST if configured
    let script_config = state.get_script_config(server_id).await;
    if let Some(ref config) = script_config {
        // Use the event ID as context_type for script routing
        let context_type = event.id().to_string();

        // Check if script handles this context
        if config.handles_context(&context_type) {
            // Get server info to build script input
            let server_info = state.get_server(server_id).await;

            if let Some(server) = server_info {
                // Build connection context if available
                let connection_context = if let Some(conn_id) = connection_id {
                    // Try to get connection info from server
                    server.connections.get(&conn_id).map(|conn_state| {
                        crate::scripting::types::ConnectionContext {
                            id: conn_id.to_string(),
                            remote_addr: conn_state.remote_addr.to_string(),
                            bytes_received: conn_state.bytes_received,
                            bytes_sent: conn_state.bytes_sent,
                        }
                    })
                } else {
                    None
                };

                // Build structured input for script
                let script_input = crate::scripting::types::ScriptInput {
                    context_type: context_type.clone(),
                    server: crate::scripting::types::ServerContext {
                        id: server.id.as_u32(),
                        port: server.port,
                        stack: server.base_stack.name().to_string(),
                        memory: server.memory.clone(),
                        instruction: server.instruction.clone(),
                    },
                    connection: connection_context,
                    event: event.data.clone(),
                };

                // Try to execute the script
                match crate::scripting::ScriptManager::try_execute(Some(config), &script_input) {
                    Ok(Some(script_response)) => {
                        // Script handled it successfully!
                        debug!(
                            "Script handled event (context: {}, {} actions)",
                            context_type,
                            script_response.actions.len()
                        );

                        // Execute the script's actions
                        let result = execute_actions(
                            script_response.actions,
                            state,
                            Some(protocol),
                        )
                        .await
                        .context("Failed to execute script actions")?;

                        return Ok(result);
                    }
                    Ok(None) => {
                        // Script requested fallback or doesn't handle this context
                        debug!("Script returned None, falling back to LLM");
                    }
                    Err(e) => {
                        // Script execution failed, fall back to LLM
                        warn!("Script execution failed ({}), falling back to LLM", e);
                    }
                }
            }
        } else {
            debug!(
                "Script does not handle context '{}', using LLM",
                context_type
            );
        }
    }

    // FALLBACK TO LLM (normal path if no script or script failed/requested fallback)

    // Get model from state
    let model = state.get_ollama_model().await;

    // Collect all actions: common + event-specific actions
    let mut all_actions = get_network_event_common_actions();

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

    // Build prompt using action system
    let prompt = PromptBuilder::build_network_event_action_prompt_for_server(
        state,
        server_id,
        &event_description,
        event.data.clone(), // Use event data as context
        all_actions,
    )
    .await;

    // Call LLM (uses crate-private generate method)
    let llm_output = llm_client
        .generate(&model, &prompt)
        .await
        .context("LLM generate call failed")?;

    // Parse action response
    let action_response = ActionResponse::from_str(&llm_output)
        .context("Failed to parse LLM response as ActionResponse")?;

    if action_response.actions.is_empty() {
        warn!("LLM returned empty actions array for event: {}", event.id());
    }

    // Execute actions
    let result = execute_actions(
        action_response.actions,
        state,
        Some(protocol),
    )
    .await
    .context("Failed to execute actions")?;

    debug!(
        "LLM call completed: {} messages, {} protocol results",
        result.messages.len(),
        result.protocol_results.len()
    );

    Ok(result)
}
