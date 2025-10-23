//! Common LLM response handling logic

use std::sync::Arc;
use tracing::info;
use crate::state::app_state::AppState;
use super::ollama_client::LlmResponse;

/// Result of processing an LLM response - contains flags and data for protocol-specific handling
pub struct ProcessedResponse {
    /// Data to send over the connection
    pub output: Option<String>,
    /// Whether to close the connection
    pub close_connection: bool,
    /// Whether to wait for more data
    pub wait_for_more: bool,
    /// Whether to shutdown the server
    pub shutdown_server: bool,
}

/// Handle common LLM response actions (memory updates, logging)
/// Returns the processed response with flags for protocol-specific handling
pub async fn handle_llm_response(
    response: LlmResponse,
    app_state: &Arc<AppState>,
) -> ProcessedResponse {
    // Handle memory updates
    if let Some(set_mem) = response.set_memory {
        app_state.set_memory(set_mem).await;
    }
    if let Some(append_mem) = response.append_memory {
        let current = app_state.get_memory().await;
        let new_memory = if current.is_empty() {
            append_mem
        } else {
            format!("{}\n{}", current, append_mem)
        };
        app_state.set_memory(new_memory).await;
    }

    // Handle log messages
    if let Some(log_msg) = response.log_message {
        info!("{}", log_msg);
    }

    ProcessedResponse {
        output: response.output,
        close_connection: response.close_connection,
        wait_for_more: response.wait_for_more,
        shutdown_server: response.shutdown_server,
    }
}
