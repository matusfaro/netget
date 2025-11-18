//! Client startup logic for TUI mode
//!
//! Handles connecting clients based on application state

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::events::ActionExecutionError;
use crate::llm::OllamaClient;
use crate::state::app_state::AppState;
use crate::state::ClientId;

/// Start a specific client by ID
pub async fn start_client_by_id(
    state: &AppState,
    client_id: ClientId,
    llm_client: &OllamaClient,
    status_tx: &mpsc::UnboundedSender<String>,
) -> Result<(), ActionExecutionError> {
    // Get client info
    let client = match state.get_client(client_id).await {
        Some(c) => c,
        None => {
            let _ = status_tx.send(format!("[ERROR] Client #{} not found", client_id.as_u32()));
            return Ok(());
        }
    };

    let protocol_name = client.protocol_name.clone();
    let remote_addr = client.remote_addr.clone();

    let msg = format!(
        "[CLIENT] Starting client #{} ({}) connecting to {}",
        client_id.as_u32(),
        protocol_name,
        remote_addr
    );
    let _ = status_tx.send(msg.clone());

    // Actually connect the client using the registry
    use crate::state::client::ClientStatus;

    // Get protocol implementation from registry
    let protocol = crate::protocol::CLIENT_REGISTRY
        .get(&protocol_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown client protocol: {}", protocol_name))?;

    // Build type-safe startup params if provided
    let startup_params = if let Some(params_json) = client.startup_params.clone() {
        // Get the parameter schema from the protocol
        let schema = protocol.get_startup_parameters();
        // Create validated StartupParams
        Some(crate::protocol::StartupParams::new(params_json, schema))
    } else {
        None
    };

    // Build connect context
    let connect_ctx = crate::protocol::ConnectContext {
        remote_addr: remote_addr.clone(),
        llm_client: llm_client.clone(),
        state: Arc::new(state.clone()),
        status_tx: status_tx.clone(),
        client_id,
        startup_params,
    };

    // Connect the client using the protocol's connect method
    match protocol.connect(connect_ctx).await {
        Ok(local_addr) => {
            // Update client status to connected
            state
                .update_client_status(client_id, ClientStatus::Connected)
                .await;
            let _ = status_tx.send(format!(
                "[CLIENT] {} client #{} connected to {} (local: {})",
                protocol_name,
                client_id.as_u32(),
                remote_addr,
                local_addr
            ));
            let _ = status_tx.send("__UPDATE_UI__".to_string());
        }
        Err(e) => {
            // For connection errors, set client status to error
            state
                .update_client_status(client_id, ClientStatus::Error(e.to_string()))
                .await;
            let _ = status_tx.send(format!(
                "[ERROR] Failed to connect {} client #{} to {}: {}",
                protocol_name,
                client_id.as_u32(),
                remote_addr,
                e
            ));
            let _ = status_tx.send("__UPDATE_UI__".to_string());
            return Err(ActionExecutionError::Fatal(e));
        }
    }

    Ok(())
}

/// Start a client from action parameters (used by /load command)
/// Returns the client ID on success
#[allow(clippy::too_many_arguments)]
pub async fn start_client_from_action(
    state: &AppState,
    protocol: &str,
    remote_addr: &str,
    instruction: String,
    startup_params: Option<serde_json::Value>,
    initial_memory: Option<String>,
    event_handlers: Option<Vec<serde_json::Value>>,
    scheduled_tasks: Option<Vec<crate::llm::actions::common::ServerTaskDefinition>>,
    feedback_instructions: Option<String>,
    llm_client: OllamaClient,
) -> Result<ClientId> {
    use crate::state::client::ClientStatus;

    // Get protocol from registry
    let protocol_impl = crate::protocol::CLIENT_REGISTRY
        .get(protocol)
        .ok_or_else(|| anyhow::anyhow!("Unknown client protocol: {}", protocol))?;

    // Create client instance
    let client = crate::state::client::ClientInstance {
        id: ClientId::new(0), // Will be assigned by add_client
        remote_addr: remote_addr.to_string(),
        protocol_name: protocol.to_string(),
        instruction: instruction.clone(),
        memory: String::new(),
        status: ClientStatus::Connecting,
        connection: None,
        handle: None,
        created_at: std::time::Instant::now(),
        status_changed_at: std::time::Instant::now(),
        startup_params: startup_params.clone(),
        event_handler_config: None,
        protocol_data: serde_json::Value::Null,
        log_files: Default::default(),
        feedback_instructions,
        feedback_buffer: Vec::new(),
        last_feedback_processed: None,
    };

    let client_id = state.add_client(client).await;

    // Set initial memory if provided
    if let Some(mem) = initial_memory {
        state
            .with_client_mut(client_id, |c| {
                c.memory = mem;
            })
            .await;
    }

    // Configure event handlers if provided
    if let Some(handlers) = event_handlers {
        use crate::scripting::{EventHandler, EventHandlerConfig};

        let event_handlers: Vec<EventHandler> = handlers
            .into_iter()
            .filter_map(|h| serde_json::from_value(h).ok())
            .collect();

        if !event_handlers.is_empty() {
            state
                .with_client_mut(client_id, |c| {
                    c.event_handler_config = Some(EventHandlerConfig {
                        handlers: event_handlers,
                    });
                })
                .await;
        }
    }

    // Create scheduled tasks if provided
    if let Some(tasks) = scheduled_tasks {
        for task_def in tasks {
            use crate::state::task::{ScheduledTask, TaskId, TaskScope, TaskStatus, TaskType};
            use std::time::{Duration, Instant};

            // Determine task type
            let task_type = if task_def.recurring {
                TaskType::Recurring {
                    interval_secs: task_def.interval_secs.unwrap_or(60),
                    max_executions: task_def.max_executions,
                    executions_count: 0,
                }
            } else {
                TaskType::OneShot {
                    delay_secs: task_def.delay_secs.unwrap_or(0),
                }
            };

            // Calculate next execution time
            let delay = if task_def.recurring {
                Duration::from_secs(0) // Start immediately for recurring
            } else {
                Duration::from_secs(task_def.delay_secs.unwrap_or(0))
            };

            let task = ScheduledTask {
                id: TaskId::new(rand::random()),
                name: task_def.task_id,
                scope: TaskScope::Client(client_id),
                task_type,
                instruction: task_def.instruction,
                context: task_def.context,
                status: TaskStatus::Scheduled,
                created_at: Instant::now(),
                next_execution: Instant::now() + delay,
                last_error: None,
                failure_count: 0,
            };

            state.add_task(task).await;
        }
    }

    // Build startup params
    let startup_params_obj = if let Some(params_json) = startup_params {
        let schema = protocol_impl.get_startup_parameters();
        Some(crate::protocol::StartupParams::new(params_json, schema))
    } else {
        None
    };

    // Create a temporary status channel (commands don't use rolling TUI status)
    let (status_tx, _status_rx) = mpsc::unbounded_channel();

    // Build connect context
    let connect_ctx = crate::protocol::ConnectContext {
        remote_addr: remote_addr.to_string(),
        llm_client: llm_client.clone(),
        state: Arc::new(state.clone()),
        status_tx,
        client_id,
        startup_params: startup_params_obj,
    };

    // Connect the client
    match protocol_impl.connect(connect_ctx).await {
        Ok(_local_addr) => {
            state
                .update_client_status(client_id, ClientStatus::Connected)
                .await;
            Ok(client_id)
        }
        Err(e) => {
            state
                .update_client_status(client_id, ClientStatus::Error(e.to_string()))
                .await;
            Err(e)
        }
    }
}
