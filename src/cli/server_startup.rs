//! Server startup logic for TUI mode
//!
//! Handles spawning TCP and HTTP servers based on application state

use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::events::ActionExecutionError;
use crate::llm::OllamaClient;
use crate::state::app_state::AppState;
use crate::state::ServerId;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// Check if an error is due to address already in use
fn is_addr_in_use_error(err: &anyhow::Error) -> bool {
    if let Some(io_err) = err.downcast_ref::<std::io::Error>() {
        io_err.kind() == std::io::ErrorKind::AddrInUse
    } else {
        false
    }
}

/// Start a specific server by ID
pub async fn start_server_by_id(
    state: &AppState,
    server_id: ServerId,
    llm_client: &OllamaClient,
    status_tx: &mpsc::UnboundedSender<String>,
) -> Result<(), ActionExecutionError> {
    // Get server info
    let server = match state.get_server(server_id).await {
        Some(s) => s,
        None => {
            console_error!(status_tx, "[ERROR] Server #{} not found", server_id.as_u32());
            return Ok(());
        }
    };

    // Build listen address
    let listen_addr: SocketAddr = format!("127.0.0.1:{}", server.port)
        .parse()
        .map_err(|e| ActionExecutionError::Fatal(anyhow::anyhow!("Invalid address: {}", e)))?;

    let protocol_name = server.protocol_name.clone();
    let msg = format!(
        "[SERVER] Starting server #{} ({}) on {}",
        server_id.as_u32(),
        protocol_name,
        listen_addr
    );
    let _ = status_tx.send(msg.clone());

    // Actually spawn the server using the registry
    use crate::state::server::ServerStatus;

    // Get protocol implementation from registry
    let protocol = crate::protocol::server_registry::registry()
        .get(&protocol_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown protocol: {}", protocol_name))?;

    // Check privilege requirements before spawning
    let metadata = protocol.metadata();
    let system_caps = state.get_system_capabilities().await;

    if !metadata.privilege_requirement.is_met_by(&system_caps) {
        let error_msg = format!(
            "Cannot start {} server on port {}: {}. Current capabilities: {}",
            protocol_name,
            server.port,
            metadata.privilege_requirement.description(),
            system_caps.description()
        );

        // Provide helpful suggestion based on platform
        let suggestion = if cfg!(target_os = "linux") {
            match &metadata.privilege_requirement {
                crate::protocol::metadata::PrivilegeRequirement::PrivilegedPort(port) => {
                    format!("\nSuggestion: Run as root (sudo) or use a port >= 1024 (e.g., {}, {}, {})",
                        port + 8000, port + 10000, 8080)
                }
                crate::protocol::metadata::PrivilegeRequirement::RawSockets => {
                    "\nSuggestion: Run as root or grant CAP_NET_RAW: sudo setcap cap_net_raw+ep /path/to/netget".to_string()
                }
                crate::protocol::metadata::PrivilegeRequirement::Root => {
                    "\nSuggestion: Run as root (sudo netget ...)".to_string()
                }
                _ => String::new(),
            }
        } else if cfg!(target_os = "macos") {
            "\nSuggestion: Run as root (sudo netget ...)".to_string()
        } else {
            "\nSuggestion: Run as Administrator".to_string()
        };

        let full_error = format!("{}{}", error_msg, suggestion);

        state
            .update_server_status(server_id, ServerStatus::Error(full_error.clone()))
            .await;
        console_error!(status_tx, "[ERROR] {}", full_error);
        console_info!(status_tx, "__UPDATE_UI__");
        return Err(ActionExecutionError::PrivilegeDenied {
            requirement: metadata.privilege_requirement.description(),
            message: full_error,
        });
    }

    // Build type-safe startup params if provided
    let startup_params = if let Some(params_json) = server.startup_params.clone() {
        // Get the parameter schema from the protocol
        let schema = protocol.get_startup_parameters();
        // Create validated StartupParams
        Some(crate::protocol::StartupParams::new(params_json, schema))
    } else {
        None
    };

    // Build spawn context
    let spawn_ctx = crate::protocol::SpawnContext {
        listen_addr,
        llm_client: llm_client.clone(),
        state: Arc::new(state.clone()),
        status_tx: status_tx.clone(),
        server_id,
        startup_params,
    };

    // Spawn the server using the protocol's spawn method
    match protocol.spawn(spawn_ctx).await {
        Ok(actual_addr) => {
            // Update server with actual listen address
            state.update_server_local_addr(server_id, actual_addr).await;
            state
                .update_server_status(server_id, ServerStatus::Running)
                .await;
            console_info!(status_tx, "[SERVER] {} server #{} listening on {}");
            console_info!(status_tx, "__UPDATE_UI__");
        }
        Err(e) => {
            // Check if error is due to port already in use
            if is_addr_in_use_error(&e) {
                // Return retryable error with context for LLM
                console_info!(status_tx, "[INFO] Port {} is already in use for {} server, will retry with LLM suggestion");
                return Err(ActionExecutionError::PortConflict {
                    port: server.port,
                    protocol: protocol_name.clone(),
                    underlying_error: e.to_string(),
                });
            }

            // For other errors, fail immediately
            state
                .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                .await;
            console_error!(status_tx, "[ERROR] Failed to start {} server #{}: {}");
            console_info!(status_tx, "__UPDATE_UI__");
            return Err(ActionExecutionError::Fatal(e));
        }
    }

    Ok(())
}

/// Start a server from action parameters (used by /load command)
/// Returns the server ID on success
#[allow(clippy::too_many_arguments)]
pub async fn start_server_from_action(
    state: &AppState,
    port: u16,
    base_stack: &str,
    _send_first: bool,
    initial_memory: Option<String>,
    instruction: String,
    startup_params: Option<serde_json::Value>,
    event_handlers: Option<Vec<serde_json::Value>>,
    scheduled_tasks: Option<Vec<crate::llm::actions::common::ServerTaskDefinition>>,
) -> Result<ServerId> {
    use crate::state::server::ServerStatus;

    // Get default listen address (always 127.0.0.1 for security)
    let listen_addr: SocketAddr = format!("127.0.0.1:{}", port)
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid port: {}", e))?;

    // Get protocol from registry
    let protocol = crate::protocol::server_registry::registry()
        .get(base_stack)
        .ok_or_else(|| anyhow::anyhow!("Unknown protocol: {}", base_stack))?;

    // Check privilege requirements
    let metadata = protocol.metadata();
    let system_caps = state.get_system_capabilities().await;

    if !metadata.privilege_requirement.is_met_by(&system_caps) {
        let error_msg = format!(
            "Cannot start {} server on port {}: {}",
            base_stack,
            port,
            metadata.privilege_requirement.description()
        );
        return Err(anyhow::anyhow!(error_msg));
    }

    // Create server instance
    let server = crate::state::server::ServerInstance {
        id: ServerId::new(0), // Will be assigned by add_server
        port,
        protocol_name: base_stack.to_string(),
        instruction: instruction.clone(),
        memory: String::new(),
        status: ServerStatus::Starting,
        connections: Default::default(),
        local_addr: None,
        handle: None,
        created_at: std::time::Instant::now(),
        status_changed_at: std::time::Instant::now(),
        startup_params: startup_params.clone(),
        event_handler_config: None,
        protocol_data: serde_json::Value::Null,
        log_files: Default::default(),
    };

    let server_id = state.add_server(server).await;

    // Set initial memory if provided
    if let Some(mem) = initial_memory {
        state.set_memory(server_id, mem).await;
    }

    // Configure event handlers if provided
    if let Some(handlers) = event_handlers {
        use crate::scripting::{EventHandlerConfig, EventHandler};

        let event_handlers: Vec<EventHandler> = handlers
            .into_iter()
            .filter_map(|h| serde_json::from_value(h).ok())
            .collect();

        if !event_handlers.is_empty() {
            state
                .with_server_mut(server_id, |s| {
                    s.event_handler_config = Some(EventHandlerConfig {
                        handlers: event_handlers,
                    });
                })
                .await;
        }
    }

    // Create scheduled tasks if provided
    if let Some(tasks) = scheduled_tasks {
        for task_def in tasks {
            use crate::state::task::{ScheduledTask, TaskScope, TaskType, TaskStatus, TaskId};
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
                scope: TaskScope::Server(server_id),
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
        let schema = protocol.get_startup_parameters();
        Some(crate::protocol::StartupParams::new(params_json, schema))
    } else {
        None
    };

    // Create a temporary status channel (commands don't use rolling TUI status)
    let (status_tx, _status_rx) = mpsc::unbounded_channel();

    // Build spawn context
    let spawn_ctx = crate::protocol::SpawnContext {
        listen_addr,
        llm_client: OllamaClient::new("http://localhost:11434"),
        state: Arc::new(state.clone()),
        status_tx,
        server_id,
        startup_params: startup_params_obj,
    };

    // Spawn the server
    match protocol.spawn(spawn_ctx).await {
        Ok(actual_addr) => {
            state.update_server_local_addr(server_id, actual_addr).await;
            state
                .update_server_status(server_id, ServerStatus::Running)
                .await;
            Ok(server_id)
        }
        Err(e) => {
            state
                .update_server_status(server_id, ServerStatus::Error(e.to_string()))
                .await;
            Err(e)
        }
    }
}
