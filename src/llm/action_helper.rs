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
    ActionDefinition, ActionResponse,
    executor::{execute_actions, ExecutionResult},
    get_network_event_common_actions,
    protocol_trait::ProtocolActions,
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
/// - Prompt building with action definitions
/// - LLM API call
/// - Response parsing
/// - Action execution
///
/// # Arguments
/// * `llm_client` - Ollama client instance
/// * `state` - Application state for context
/// * `server_id` - Server ID for context
/// * `event_description` - Description of what triggered this LLM call
/// * `protocol` - Optional protocol for protocol-specific sync actions
/// * `custom_actions` - Additional custom actions specific to this call
///
/// # Returns
/// * `Ok(ExecutionResult)` - Results containing messages and protocol-specific results
/// * `Err(_)` - If LLM call or action execution failed
///
/// # Example
/// ```rust,ignore
/// // Define a custom action for SSH authentication
/// let auth_action = ActionDefinition {
///     name: "ssh_auth_decision".to_string(),
///     description: "Decide whether to allow SSH authentication".to_string(),
///     parameters: vec![
///         Parameter {
///             name: "allowed".to_string(),
///             type_hint: "boolean".to_string(),
///             description: "Whether to allow this authentication".to_string(),
///             required: true,
///         }
///     ],
///     example: json!({"type": "ssh_auth_decision", "allowed": true}),
/// };
///
/// let result = call_llm_with_actions(
///     &llm_client,
///     &state,
///     server_id,
///     "SSH authentication request for user 'alice'",
///     Some(&ssh_protocol),
///     vec![auth_action],
/// ).await?;
/// ```
pub async fn call_llm_with_actions(
    llm_client: &OllamaClient,
    state: &AppState,
    server_id: ServerId,
    event_description: &str,
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

    // Build prompt using action system
    let prompt = PromptBuilder::build_network_event_action_prompt_for_server(
        state,
        server_id,
        event_description,
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
        warn!("LLM returned empty actions array for event: {}", event_description);
    }

    // Execute actions
    let result = execute_actions(
        action_response.actions,
        state,
        protocol,
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

/// Simplified variant when no custom actions are needed
///
/// This is useful when you just want to use the standard protocol actions
/// without adding any custom behavior.
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
        Some(protocol),
        Vec::new(), // No custom actions
    )
    .await
}

/// Simplified variant for custom actions only (no protocol)
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
        None,
        custom_actions,
    )
    .await
}
