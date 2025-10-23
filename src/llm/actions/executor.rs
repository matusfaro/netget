//! Action executor
//!
//! This module executes arrays of actions returned by the LLM.
//! It handles both common actions and protocol-specific actions.

use super::{
    common::CommonAction,
    context::NetworkContext,
    protocol_trait::{ActionResult, ProtocolActions},
};
use crate::state::app_state::AppState;
use anyhow::{Context as AnyhowContext, Result};
use tracing::{debug, info, warn};

/// Result of executing all actions
pub struct ExecutionResult {
    /// Messages to display to the user
    pub messages: Vec<String>,

    /// Protocol-specific action results
    pub protocol_results: Vec<ActionResult>,
}

impl ExecutionResult {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            protocol_results: Vec::new(),
        }
    }

    pub fn add_message(&mut self, message: String) {
        self.messages.push(message);
    }

    pub fn add_protocol_result(&mut self, result: ActionResult) {
        self.protocol_results.push(result);
    }
}

/// Execute an array of actions from LLM response
///
/// # Arguments
/// * `actions` - Array of action JSON objects from LLM
/// * `state` - Application state
/// * `protocol` - Optional protocol for protocol-specific actions
/// * `context` - Optional network context for sync actions
///
/// # Returns
/// * `Ok(ExecutionResult)` - Results of execution
/// * `Err(_)` - If execution failed critically
pub async fn execute_actions(
    actions: Vec<serde_json::Value>,
    state: &AppState,
    protocol: Option<&dyn ProtocolActions>,
    context: Option<&NetworkContext>,
) -> Result<ExecutionResult> {
    let mut result = ExecutionResult::new();

    for (i, action) in actions.iter().enumerate() {
        debug!("Executing action {}: {:?}", i, action);

        // Try to parse as common action first
        if let Ok(common_action) = CommonAction::from_json(action) {
            execute_common_action(common_action, state, &mut result)
                .await
                .with_context(|| format!("Failed to execute common action: {:?}", action))?;
            continue;
        }

        // Try protocol-specific action
        if let Some(proto) = protocol {
            match proto.execute_action(action.clone(), context) {
                Ok(action_result) => {
                    debug!(
                        "Protocol action executed successfully: {:?}",
                        proto.protocol_name()
                    );
                    result.add_protocol_result(action_result);
                    continue;
                }
                Err(e) => {
                    warn!(
                        "Failed to execute protocol action for {}: {}",
                        proto.protocol_name(),
                        e
                    );
                    // Don't fail completely, just log and continue
                    continue;
                }
            }
        }

        // Unknown action type
        warn!("Unknown action type, skipping: {:?}", action);
    }

    Ok(result)
}

/// Execute a common action
async fn execute_common_action(
    action: CommonAction,
    state: &AppState,
    result: &mut ExecutionResult,
) -> Result<()> {
    match action {
        CommonAction::ShowMessage { message } => {
            info!("LLM message: {}", message);
            result.add_message(message);
        }

        CommonAction::OpenServer { .. } => {
            // This should be handled by the caller (user command handler)
            // because it requires spawning a new server task
            warn!("open_server action cannot be executed by action executor - must be handled by caller");
        }

        CommonAction::CloseServer => {
            // This should be handled by the caller
            warn!("close_server action cannot be executed by action executor - must be handled by caller");
        }

        CommonAction::UpdateInstruction { .. } => {
            // This should be handled by the caller
            warn!("update_instruction action cannot be executed by action executor - must be handled by caller");
        }

        CommonAction::ChangeModel { .. } => {
            // This should be handled by the caller
            warn!("change_model action cannot be executed by action executor - must be handled by caller");
        }

        CommonAction::SetMemory { value } => {
            state.set_memory(value).await;
            debug!("Global memory set");
        }

        CommonAction::AppendMemory { value } => {
            let current = state.get_memory().await;
            let new_memory = if current.is_empty() {
                value
            } else {
                format!("{}\n{}", current, value)
            };
            state.set_memory(new_memory).await;
            debug!("Global memory appended");
        }
    }

    Ok(())
}

/// Extract server management actions that need special handling
///
/// These actions cannot be executed directly by the executor and must
/// be handled by the caller (usually the user command handler in main.rs)
pub fn extract_server_management_actions(
    actions: &[serde_json::Value],
) -> Vec<CommonAction> {
    actions
        .iter()
        .filter_map(|action| {
            if let Ok(common_action) = CommonAction::from_json(action) {
                match common_action {
                    CommonAction::OpenServer { .. }
                    | CommonAction::CloseServer
                    | CommonAction::UpdateInstruction { .. }
                    | CommonAction::ChangeModel { .. } => Some(common_action),
                    _ => None,
                }
            } else {
                None
            }
        })
        .collect()
}
