//! Action executor
//!
//! This module executes arrays of actions returned by the LLM.
//! It handles both common actions and protocol-specific actions.

use super::{
    common::CommonAction,
    protocol_trait::{ActionResult, Server},
};
use crate::state::app_state::AppState;
use anyhow::{Context as AnyhowContext, Result};
use tracing::{debug, warn};

/// Result of executing all actions
pub struct ExecutionResult {
    /// Messages to display to the user
    pub messages: Vec<String>,

    /// Protocol-specific action results
    pub protocol_results: Vec<ActionResult>,

    /// Raw action JSON (for protocols that need to manually process actions)
    /// This is used by protocols like mDNS and NFS that have special manual processing
    pub raw_actions: Vec<serde_json::Value>,
}

impl Default for ExecutionResult {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionResult {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            protocol_results: Vec::new(),
            raw_actions: Vec::new(),
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
/// * `server_id` - Optional server ID for context (used for feedback, memory, etc.)
/// * `client_id` - Optional client ID for context (used for client feedback)
///
/// # Returns
/// * `Ok(ExecutionResult)` - Results of execution
/// * `Err(_)` - If execution failed critically
pub async fn execute_actions(
    actions: Vec<serde_json::Value>,
    state: &AppState,
    protocol: Option<&dyn Server>,
    server_id: Option<crate::state::ServerId>,
    client_id: Option<crate::state::ClientId>,
) -> Result<ExecutionResult> {
    let mut result = ExecutionResult::new();

    // Store raw actions for protocols that need manual processing (mDNS, NFS, etc.)
    result.raw_actions = actions.clone();

    for (i, action) in actions.iter().enumerate() {
        debug!("Executing action {}: {:?}", i, action);

        // Try to parse as common action first
        if let Ok(common_action) = CommonAction::from_json(action) {
            execute_common_action(common_action, state, &mut result, server_id, client_id)
                .await
                .with_context(|| format!("Failed to execute common action: {:?}", action))?;
            continue;
        }

        // Try protocol-specific action
        if let Some(proto) = protocol {
            match proto.execute_action(action.clone()) {
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
    _result: &mut ExecutionResult,
    server_id: Option<crate::state::ServerId>,
    client_id: Option<crate::state::ClientId>,
) -> Result<()> {
    match action {
        CommonAction::ShowMessage { .. } => {
            // ShowMessage is handled by the caller (event handler) to avoid duplicate output
            // This match arm exists to satisfy exhaustiveness checking
        }

        CommonAction::OpenServer { .. } => {
            // This should be handled by the caller (user command handler)
            // because it requires spawning a new server task
            warn!("open_server action cannot be executed by action executor - must be handled by caller");
        }

        CommonAction::CloseServer { .. } => {
            // This should be handled by the caller
            warn!("close_server action cannot be executed by action executor - must be handled by caller");
        }

        CommonAction::CloseAllServers => {
            // This should be handled by the caller
            warn!("close_all_servers action cannot be executed by action executor - must be handled by caller");
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
            let sid = server_id.or_else(|| state.get_first_server_id_sync());
            if let Some(server_id) = sid {
                state.set_memory(server_id, value).await;
                debug!("Server #{} memory set", server_id.as_u32());
            }
        }

        CommonAction::AppendMemory { value } => {
            let sid = server_id.or_else(|| state.get_first_server_id_sync());
            if let Some(server_id) = sid {
                let current = state.get_memory(server_id).await.unwrap_or_default();
                let new_memory = if current.is_empty() {
                    value
                } else {
                    format!("{}\n{}", current, value)
                };
                state.set_memory(server_id, new_memory).await;
                debug!("Server #{} memory appended", server_id.as_u32());
            }
        }

        CommonAction::AppendToLog {
            output_name,
            content,
        } => {
            let sid = server_id.or_else(|| state.get_first_server_id_sync());
            if let Some(server_id) = sid {
                // Get or create the log file path
                let log_path = state
                    .with_server_mut(server_id, |server| {
                        server.get_or_create_log_path(&output_name)
                    })
                    .await;

                if let Some(log_path) = log_path {
                    // Append content to the log file
                    match tokio::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&log_path)
                        .await
                    {
                        Ok(mut file) => {
                            use tokio::io::AsyncWriteExt;
                            let log_line = format!("{}\n", content);
                            if let Err(e) = file.write_all(log_line.as_bytes()).await {
                                warn!("Failed to write to log file {:?}: {}", log_path, e);
                            } else {
                                debug!("Appended to log file {:?}", log_path);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to open log file {:?}: {}", log_path, e);
                        }
                    }
                } else {
                    warn!("No server found to append log for");
                }
            }
        }
        CommonAction::ScheduleTask { .. }
        | CommonAction::CancelTask { .. }
        | CommonAction::ListTasks => {
            // Task scheduling handled by event handler, not executor
        }

        CommonAction::OpenClient { .. }
        | CommonAction::CloseClient { .. }
        | CommonAction::CloseAllClients
        | CommonAction::CloseConnectionById { .. }
        | CommonAction::ReconnectClient { .. }
        | CommonAction::UpdateClientInstruction { .. } => {
            // Client and connection management handled by event handler, not executor
        }

        CommonAction::ProvideFeedback { feedback } => {
            // Accumulate feedback for later processing (debounced + LLM invocation)
            if let Some(sid) = server_id {
                state
                    .add_server_feedback(sid, feedback)
                    .await
                    .unwrap_or_else(|e| {
                        warn!("Failed to add server feedback: {}", e);
                    });
                debug!("Server #{} feedback accumulated", sid.as_u32());
            } else if let Some(cid) = client_id {
                state
                    .add_client_feedback(cid, feedback)
                    .await
                    .unwrap_or_else(|e| {
                        warn!("Failed to add client feedback: {}", e);
                    });
                debug!("Client #{} feedback accumulated", cid.as_u32());
            } else {
                warn!("provide_feedback action called without server_id or client_id context");
            }
        }

        #[cfg(feature = "sqlite")]
        CommonAction::CreateDatabase { .. }
        | CommonAction::ExecuteSql { .. }
        | CommonAction::ListDatabases
        | CommonAction::DeleteDatabase { .. } => {
            // SQLite operations handled by event handler, not executor
        }
    }

    Ok(())
}

/// Extract server management actions that need special handling
///
/// These actions cannot be executed directly by the executor and must
/// be handled by the caller (usually the user command handler in main.rs)
pub fn extract_server_management_actions(actions: &[serde_json::Value]) -> Vec<CommonAction> {
    actions
        .iter()
        .filter_map(|action| {
            if let Ok(common_action) = CommonAction::from_json(action) {
                match common_action {
                    CommonAction::OpenServer { .. }
                    | CommonAction::CloseServer { .. }
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
