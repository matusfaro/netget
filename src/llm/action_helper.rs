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
    ActionDefinition,
};
use crate::llm::ollama_client::OllamaClient;
use crate::llm::prompt::PromptBuilder;
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
/// * `event_description` - High-level description of the event (e.g., "NFS lookup requested")
/// * `context_json` - Structured context data (e.g., NFS parameters, request data, etc.)
/// * `protocol` - Optional protocol for protocol-specific sync actions
/// * `custom_actions` - Additional custom actions specific to this call
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
    event_description: &str,
    context_json: serde_json::Value,
    protocol: Option<&dyn ProtocolActions>,
    custom_actions: Vec<ActionDefinition>,
) -> Result<ExecutionResult> {
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
    event_description: &str,
    protocol: &dyn ProtocolActions,
) -> Result<ExecutionResult> {
    call_llm_with_actions(
        llm_client,
        state,
        server_id,
        event_description,
        serde_json::json!({}), // Empty context
        Some(protocol),
        Vec::new(), // No custom actions
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
    event_description: &str,
    custom_actions: Vec<ActionDefinition>,
) -> Result<ExecutionResult> {
    call_llm_with_actions(
        llm_client,
        state,
        server_id,
        event_description,
        serde_json::json!({}), // Empty context
        None,
        custom_actions,
    )
    .await
}
