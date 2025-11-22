//! Easy protocol startup logic

use anyhow::{Context, Result};
use serde_json::Value as JsonValue;
use std::sync::Arc;
use tracing::info;

use crate::llm::OllamaClient;
use crate::protocol::EASY_REGISTRY;
use crate::state::{AppState, EasyId, EasyInstance, EasyStatus};

/// Start an easy protocol instance
///
/// This function:
/// 1. Creates an EasyInstance and adds it to AppState
/// 2. Calls the Easy protocol's generate_startup_action() to get underlying protocol action
/// 3. Executes the action to start the underlying server/client
/// 4. Links the underlying server/client to the easy instance for event routing
///
/// # Arguments
/// * `protocol_name` - Easy protocol name (e.g., "http-easy")
/// * `user_instruction` - Optional custom instruction from user
/// * `port` - Optional port override
/// * `state` - Application state
/// * `llm_client` - LLM client
///
/// # Returns
/// Easy protocol ID
pub async fn start_easy_protocol(
    protocol_name: &str,
    user_instruction: Option<String>,
    port: Option<u16>,
    state: Arc<AppState>,
    llm_client: Arc<OllamaClient>,
) -> Result<EasyId> {
    // Get easy protocol from registry
    let easy_protocol = EASY_REGISTRY
        .get_by_name(protocol_name)
        .ok_or_else(|| anyhow::anyhow!("Easy protocol '{}' not found", protocol_name))?;

    let underlying_protocol = easy_protocol.underlying_protocol();

    info!(
        "Starting easy protocol '{}' (wrapping '{}')",
        protocol_name, underlying_protocol
    );

    // Create easy instance
    let easy_instance = EasyInstance::new(
        EasyId::new(0), // Will be assigned by AppState
        protocol_name.to_string(),
        underlying_protocol.to_string(),
        user_instruction.clone(),
    );

    // Add to state and get assigned ID
    let easy_id = state.add_easy_instance(easy_instance).await;

    // Update status to Starting
    state
        .update_easy_status(easy_id, EasyStatus::Starting)
        .await;

    // Generate startup action for underlying protocol
    let action = easy_protocol
        .generate_startup_action(user_instruction.clone(), port)
        .context("Failed to generate startup action")?;

    info!("Generated startup action: {}", serde_json::to_string_pretty(&action)?);

    // Execute the startup action (open_server or open_client)
    match execute_startup_action(&action, &state, &llm_client).await {
        Ok(underlying_id) => {
            // Link underlying server/client to easy instance
            if action["type"] == "open_server" {
                if let Some(server_id) = underlying_id.as_u64() {
                    let server_id = crate::state::ServerId::new(server_id as u32);
                    state.link_server_to_easy(server_id, easy_id).await;
                    info!(
                        "Linked easy instance {} to server {}",
                        easy_id, server_id
                    );
                }
            } else if action["type"] == "open_client" {
                if let Some(client_id) = underlying_id.as_u64() {
                    let client_id = crate::state::ClientId::new(client_id as u32);
                    state.link_client_to_easy(client_id, easy_id).await;
                    info!(
                        "Linked easy instance {} to client {}",
                        easy_id, client_id
                    );
                }
            }

            // Update status to Running
            state
                .update_easy_status(easy_id, EasyStatus::Running)
                .await;

            Ok(easy_id)
        }
        Err(e) => {
            // Update status to Error
            state
                .update_easy_status(easy_id, EasyStatus::Error(e.to_string()))
                .await;
            Err(e).context("Failed to start underlying protocol")
        }
    }
}

/// Execute a startup action (open_server or open_client) and return the ID of the created instance
async fn execute_startup_action(
    action: &JsonValue,
    state: &Arc<AppState>,
    _llm_client: &Arc<OllamaClient>,
) -> Result<JsonValue> {
    let action_type = action["type"].as_str().ok_or_else(|| {
        anyhow::anyhow!("Startup action missing 'type' field")
    })?;

    match action_type {
        "open_server" => {
            // Extract server parameters
            let protocol = action["protocol"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'protocol' field"))?;
            let port = action["port"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'port' field"))? as u16;
            let instruction = action["instruction"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'instruction' field"))?;

            // Create status channel for server startup messages
            let (status_tx, _status_rx) = tokio::sync::mpsc::unbounded_channel();

            // Call server_startup to create the server
            let server_id = crate::cli::server_startup::start_server_from_action(
                &state,
                None,        // mac_address
                None,        // interface
                None,        // host
                Some(port),  // port
                protocol,
                false, // send_first
                None,  // initial_memory
                instruction.to_string(),
                None, // startup_params
                None, // event_handlers
                None, // scheduled_tasks
                None, // feedback_instructions
                status_tx,
            )
            .await
            .context("Failed to start server")?;

            Ok(JsonValue::Number(server_id.as_u32().into()))
        }
        "open_client" => {
            // Not implemented yet - would need similar approach for clients
            Err(anyhow::anyhow!("open_client not yet supported for easy protocols"))
        }
        _ => Err(anyhow::anyhow!("Unknown action type: {}", action_type)),
    }
}
