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
            let _ = status_tx.send(format!("[ERROR] Server #{} not found", server_id.as_u32()));
            return Ok(());
        }
    };

    // Build listen address
    let listen_addr: SocketAddr = format!("127.0.0.1:{}", server.port)
        .parse()
        .map_err(|e| ActionExecutionError::Fatal(anyhow::anyhow!("Invalid address: {}", e)))?;

    let protocol_name = server.protocol_name.clone();

    // Actually spawn the server using the registry
    use crate::state::server::ServerStatus;

    // Get protocol implementation from registry
    let protocol = crate::protocol::server_registry::registry()
        .get(&protocol_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown protocol: {}", protocol_name))?;

    // Check privilege requirements before spawning
    let metadata = protocol.metadata();
    let system_caps = state.get_system_capabilities().await;

    // Determine if the actual port being used requires privileges
    let requires_privileges = match &metadata.privilege_requirement {
        crate::protocol::metadata::PrivilegeRequirement::PrivilegedPort(_) => {
            // Only require privileges if actually binding to a privileged port
            // Port 0 means OS-assigned port, which will always be unprivileged (>1024)
            server.port > 0 && server.port < 1024
        }
        _ => {
            // For other requirements (RawSockets, Root, etc.), check as normal
            !metadata.privilege_requirement.is_met_by(&system_caps)
        }
    };

    if requires_privileges && !system_caps.can_bind_privileged_ports {
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
        let _ = status_tx.send(format!("[ERROR] {}", full_error));
        let _ = status_tx.send("__UPDATE_UI__".to_string());
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
    // NOTE: This is the legacy path for unmigrated protocols
    // Migrated protocols should be started via start_server_from_action
    let spawn_ctx = crate::protocol::SpawnContext {
        listen_addr,
        mac_address: None,
        interface: None,
        host: None,
        port: None,
        llm_client: llm_client.clone(),
        state: Arc::new(state.clone()),
        status_tx: status_tx.clone(),
        server_id,
        startup_params,
    };

    // Spawn the server using the protocol's spawn method
    match protocol.spawn(spawn_ctx).await {
        Ok(actual_addr) => {
            // Send startup message with actual port
            let msg = format!(
                "[SERVER] Starting server #{} ({}) on {}",
                server_id.as_u32(),
                protocol_name,
                actual_addr
            );
            let _ = status_tx.send(msg);

            // Update server with actual listen address
            state.update_server_local_addr(server_id, actual_addr).await;
            state
                .update_server_status(server_id, ServerStatus::Running)
                .await;
            // Send update message with actual bound address (for tests that use port 0)
            if server.port == 0 || server.port != actual_addr.port() {
                let update_msg = format!(
                    "[SERVER] Server #{} ({}) listening on {}",
                    server_id.as_u32(),
                    protocol_name,
                    actual_addr
                );
                let _ = status_tx.send(update_msg);
            }
            // Note: protocol-specific "listening on" message is also sent by the protocol's spawn method to tracing
            let _ = status_tx.send("__UPDATE_UI__".to_string());
        }
        Err(e) => {
            // Check if error is due to port already in use
            if is_addr_in_use_error(&e) {
                // Return retryable error with context for LLM
                let _ = status_tx.send(format!(
                    "[INFO] Port {} is already in use for {} server, will retry with LLM suggestion",
                    server.port,
                    protocol_name
                ));
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
            let _ = status_tx.send(format!(
                "[ERROR] Failed to start {} server #{}: {}",
                protocol_name,
                server_id.as_u32(),
                e
            ));
            let _ = status_tx.send("__UPDATE_UI__".to_string());
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
    // NEW: Flexible binding parameters (all optional)
    mac_address: Option<String>,
    interface: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    base_stack: &str,
    _send_first: bool,
    initial_memory: Option<String>,
    instruction: String,
    startup_params: Option<serde_json::Value>,
    event_handlers: Option<Vec<serde_json::Value>>,
    scheduled_tasks: Option<Vec<crate::llm::actions::common::ServerTaskDefinition>>,
    feedback_instructions: Option<String>,
    status_tx: mpsc::UnboundedSender<String>,
) -> Result<ServerId> {
    use crate::state::server::ServerStatus;

    // Get protocol from registry
    let protocol = crate::protocol::server_registry::registry()
        .get(base_stack)
        .ok_or_else(|| anyhow::anyhow!("Unknown protocol: {}", base_stack))?;

    // === DUAL PATH LOGIC: Migrated vs Unmigrated Protocols ===
    //
    // Migrated protocols return Some(...) from default_binding()
    // Unmigrated protocols return None and use legacy listen_addr
    //
    let (final_mac, final_interface, final_host, final_port, use_new_path, listen_addr) =
        if let Some(defaults) = protocol.default_binding() {
            // NEW PATH: Protocol has been migrated, use flexible binding
            let (mac, iface, host_str, port_num) = defaults.apply(
                mac_address.clone(),
                interface.clone(),
                host.clone(),
                port,
            );

            // For port-based protocols with port 0, find available port
            let final_port_num = if let Some(p) = port_num {
                if p == 0 {
                    // Find available port
                    use tokio::net::TcpListener;
                    let bind_host = host_str.as_deref().unwrap_or("127.0.0.1");
                    let listener = TcpListener::bind(format!("{}:0", bind_host))
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to find available port: {}", e))?;
                    let found_port = listener
                        .local_addr()
                        .map_err(|e| anyhow::anyhow!("Failed to get local address: {}", e))?
                        .port();
                    drop(listener);
                    Some(found_port)
                } else {
                    Some(p)
                }
            } else {
                None
            };

            // Construct legacy listen_addr for backwards compatibility
            // (protocols still receive this field, but new protocols should ignore it)
            let legacy_addr = match (&host_str, final_port_num) {
                (Some(h), Some(p)) => {
                    format!("{}:{}", h, p)
                        .parse()
                        .unwrap_or_else(|_| "127.0.0.1:0".parse().unwrap())
                }
                _ => "127.0.0.1:0".parse().unwrap(),
            };

            (mac, iface, host_str, final_port_num, true, legacy_addr)
        } else {
            // OLD PATH: Protocol hasn't been migrated, use backwards-compatible behavior
            // Port field is required for unmigrated protocols
            let port_value = port.ok_or_else(|| {
                anyhow::anyhow!(
                    "Protocol '{}' requires 'port' parameter (unmigrated protocol)",
                    base_stack
                )
            })?;

            // If port is 0, find an available port automatically
            let actual_port = if port_value == 0 {
                use tokio::net::TcpListener;
                let listener = TcpListener::bind("127.0.0.1:0")
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to find available port: {}", e))?;
                let found_port = listener
                    .local_addr()
                    .map_err(|e| anyhow::anyhow!("Failed to get local address: {}", e))?
                    .port();
                drop(listener);
                found_port
            } else {
                port_value
            };

            // Get default listen address (always 127.0.0.1 for security)
            let listen_addr: SocketAddr = format!("127.0.0.1:{}", actual_port)
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid port: {}", e))?;

            (
                None,
                None,
                Some("127.0.0.1".to_string()),
                Some(actual_port),
                false,
                listen_addr,
            )
        };

    // Check privilege requirements
    let metadata = protocol.metadata();
    let system_caps = state.get_system_capabilities().await;

    // Determine if the actual port being used requires privileges
    let requires_privileges = match &metadata.privilege_requirement {
        crate::protocol::metadata::PrivilegeRequirement::PrivilegedPort(_) => {
            // Only require privileges if actually binding to a privileged port
            // Port 0 means OS-assigned port, which will always be unprivileged (>1024)
            // final_port has already been resolved to an actual port number
            match final_port {
                Some(p) => p > 0 && p < 1024,
                None => false, // Interface-based protocols don't use ports
            }
        }
        _ => {
            // For other requirements (RawSockets, Root, etc.), check as normal
            !metadata.privilege_requirement.is_met_by(&system_caps)
        }
    };

    if requires_privileges && !system_caps.can_bind_privileged_ports {
        let error_msg = format!(
            "Cannot start {} server: {}",
            base_stack,
            metadata.privilege_requirement.description()
        );
        return Err(anyhow::anyhow!(error_msg));
    }

    // Create server instance
    // NOTE: For unmigrated protocols, port is always Some(_)
    // For migrated protocols, port may be None (interface-based protocols like ICMP)
    let display_port = final_port.unwrap_or(0);

    let server = crate::state::server::ServerInstance {
        id: ServerId::new(0), // Will be assigned by add_server
        port: display_port,
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
        feedback_instructions,
        feedback_buffer: Vec::new(),
        last_feedback_processed: None,
    };

    let server_id = state.add_server(server).await;

    // Set initial memory if provided
    if let Some(mem) = initial_memory {
        state.set_memory(server_id, mem).await;
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

    // Build spawn context
    // Use the configured LLM client from state (includes mock config, lock settings, etc.)
    // If not available (shouldn't happen), fall back to creating a new client
    let llm_client = if let Some(client) = state.get_llm_client().await {
        client
    } else {
        let ollama_url = state.get_ollama_url().await;
        OllamaClient::new(ollama_url)
    };

    let spawn_ctx = crate::protocol::SpawnContext {
        listen_addr,
        mac_address: final_mac,
        interface: final_interface,
        host: final_host,
        port: final_port,
        llm_client,
        state: Arc::new(state.clone()),
        status_tx: status_tx.clone(),
        server_id,
        startup_params: startup_params_obj,
    };

    // Spawn the server
    match protocol.spawn(spawn_ctx).await {
        Ok(actual_addr) => {
            // Send startup message with actual address
            let msg = format!(
                "[SERVER] Starting server #{} ({}) on {}",
                server_id.as_u32(),
                base_stack,
                actual_addr
            );
            let _ = status_tx.send(msg);

            // Update server with actual listen address
            state.update_server_local_addr(server_id, actual_addr).await;
            state
                .update_server_status(server_id, ServerStatus::Running)
                .await;

            // Send update message with actual bound address (for tests that use port 0)
            if final_port.unwrap_or(0) == 0 || final_port.unwrap_or(0) != actual_addr.port() {
                let update_msg = format!(
                    "[SERVER] Server #{} ({}) listening on {}",
                    server_id.as_u32(),
                    base_stack,
                    actual_addr
                );
                let _ = status_tx.send(update_msg);
            }

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
