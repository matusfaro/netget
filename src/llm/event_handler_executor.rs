//! Event handler executor - executes configured event handlers (script/static/llm)
//!
//! This module checks if an event has a configured handler and executes it accordingly.
//! Supports three handler types:
//! - Script: Execute inline script code
//! - Static: Execute predefined actions
//! - LLM: Delegate to LLM (fallback/default)

use crate::llm::actions::executor::{execute_actions, ExecutionResult};
use crate::llm::actions::protocol_trait::Server;
use crate::scripting::EventHandlerType;
use crate::state::app_state::AppState;
use crate::state::ServerId;
use anyhow::{Context as AnyhowContext, Result};
use tracing::{debug, warn};

/// Result from checking event handlers
pub enum EventHandlerResult {
    /// Handler executed successfully with result
    Handled(ExecutionResult),
    /// No handler configured or handler requested LLM fallback
    FallbackToLlm,
}

/// Check and execute event handler for the given event
///
/// # Arguments
/// * `state` - Application state
/// * `server_id` - Server ID for context
/// * `connection_id` - Optional connection ID
/// * `event_type_id` - Event type identifier (e.g., "tcp_data_received")
/// * `event_description` - Human-readable event description
/// * `event_data` - Structured event data for scripts
/// * `protocol` - Optional protocol for action execution
///
/// # Returns
/// * `Ok(EventHandlerResult::Handled(...))` - Handler executed successfully
/// * `Ok(EventHandlerResult::FallbackToLlm)` - No handler or fallback requested
/// * `Err(_)` - Handler execution failed critically
pub async fn try_execute_event_handler(
    state: &AppState,
    server_id: ServerId,
    connection_id: Option<crate::server::connection::ConnectionId>,
    event_type_id: &str,
    event_description: &str,
    event_data: Option<serde_json::Value>,
    protocol: Option<&dyn Server>,
) -> Result<EventHandlerResult> {
    // Get event handler configuration
    let event_handler_config = state.get_event_handler_config(server_id).await;

    let Some(config) = event_handler_config else {
        // No event handler configuration - use LLM
        return Ok(EventHandlerResult::FallbackToLlm);
    };

    // Find matching handler for this event type
    let Some(handler_type) = config.find_handler(event_type_id) else {
        // No matching handler - use LLM
        debug!("No handler matches event '{}', using LLM", event_type_id);
        return Ok(EventHandlerResult::FallbackToLlm);
    };

    match handler_type {
        EventHandlerType::Llm => {
            // LLM handler explicitly configured
            debug!("LLM handler configured for event '{}'", event_type_id);
            Ok(EventHandlerResult::FallbackToLlm)
        }

        EventHandlerType::Script { language, code } => {
            // Execute script handler
            execute_script_handler(
                state,
                server_id,
                connection_id,
                event_type_id,
                event_description,
                event_data,
                language,
                code,
                protocol,
            )
            .await
        }

        EventHandlerType::Static { actions } => {
            // Execute static handler
            execute_static_handler(
                state,
                event_type_id,
                event_description,
                actions,
                protocol,
            )
            .await
        }
    }
}

/// Execute a script handler
async fn execute_script_handler(
    state: &AppState,
    server_id: ServerId,
    connection_id: Option<crate::server::connection::ConnectionId>,
    event_type_id: &str,
    event_description: &str,
    event_data: Option<serde_json::Value>,
    language: &str,
    code: &str,
    protocol: Option<&dyn Server>,
) -> Result<EventHandlerResult> {
    // Get server info to build script input
    let server_info = state.get_server(server_id).await;

    let Some(server) = server_info else {
        warn!("Server #{} not found for script execution", server_id.as_u32());
        return Ok(EventHandlerResult::FallbackToLlm);
    };

    // Build connection context if available
    let connection_context = if let Some(conn_id) = connection_id {
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
    let event_json = event_data.unwrap_or_else(|| {
        serde_json::json!({"description": event_description})
    });

    let script_input = crate::scripting::types::ScriptInput {
        event_type_id: event_type_id.to_string(),
        server: crate::scripting::types::ServerContext {
            id: server.id.as_u32(),
            port: server.port,
            stack: crate::protocol::server_registry::registry()
                .stack_name_by_protocol(&server.protocol_name)
                .unwrap_or("UNKNOWN")
                .to_string(),
            memory: server.memory.clone(),
            instruction: server.instruction.clone(),
        },
        connection: connection_context,
        event: event_json,
    };

    // Parse language
    let script_language = match language.to_lowercase().as_str() {
        "python" => crate::scripting::ScriptLanguage::Python,
        "javascript" | "js" => crate::scripting::ScriptLanguage::JavaScript,
        "go" => crate::scripting::ScriptLanguage::Go,
        "perl" => crate::scripting::ScriptLanguage::Perl,
        _ => {
            warn!("Unknown script language '{}', falling back to LLM", language);
            return Ok(EventHandlerResult::FallbackToLlm);
        }
    };

    // Check if language is available
    let scripting_env = state.get_scripting_env().await;
    if !scripting_env.is_available(script_language) {
        warn!(
            "Script language {} not available, falling back to LLM",
            script_language.as_str()
        );
        return Ok(EventHandlerResult::FallbackToLlm);
    }

    // Build ScriptConfig for execution
    let script_config = crate::scripting::types::ScriptConfig {
        language: script_language,
        source: crate::scripting::types::ScriptSource::Inline(code.to_string()),
        handles_contexts: vec![event_type_id.to_string()],
    };

    // Execute the script
    match crate::scripting::executor::execute_script(&script_config, &script_input) {
        Ok(response) => {
            debug!(
                "Script handled event '{}' ({} actions)",
                event_type_id,
                response.actions.len()
            );

            // Register SCRIPT conversation for tracking
            let truncated_desc = if event_description.len() > 30 {
                format!("SCRIPT \"{}...\"", &event_description[..27])
            } else {
                format!("SCRIPT \"{}\"", event_description)
            };
            let conv_id = format!(
                "script-{}-{:x}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis(),
                rand::random::<u32>()
            );
            state
                .register_conversation(
                    conv_id.clone(),
                    crate::state::app_state::ConversationSource::Network {
                        server_id,
                        connection_id,
                    },
                    truncated_desc,
                )
                .await;

            // Execute the script's actions
            let result = execute_actions(response.actions, state, protocol)
                .await
                .context("Failed to execute script actions")?;

            // End conversation tracking
            state.end_conversation(&conv_id).await;

            Ok(EventHandlerResult::Handled(result))
        }
        Err(e) => {
            warn!("Script execution failed: {}, falling back to LLM", e);
            Ok(EventHandlerResult::FallbackToLlm)
        }
    }
}

/// Execute a static handler
async fn execute_static_handler(
    state: &AppState,
    event_type_id: &str,
    event_description: &str,
    actions: &[serde_json::Value],
    protocol: Option<&dyn Server>,
) -> Result<EventHandlerResult> {
    debug!(
        "Static handler executing for event '{}' ({} actions)",
        event_type_id,
        actions.len()
    );

    // Execute the static actions
    let result = execute_actions(actions.to_vec(), state, protocol)
        .await
        .context("Failed to execute static actions")?;

    // Log as STATIC interaction (no conversation tracking needed for static responses)
    debug!("Static handler completed: {}", event_description);

    Ok(EventHandlerResult::Handled(result))
}
