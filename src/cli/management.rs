//! Management surface for creating and updating servers and clients.
//!
//! This module is the single source of truth for the *shape* of a server or
//! client instance — every field the `open_server` / `open_client` actions and
//! the MCP `start_server` / `start_client` tools accept — and for the two
//! operations that surface takes: **create** (spawn a brand new instance) and
//! **update** (mutate a running one, in place where possible, with a clean
//! stop+start where a change genuinely requires rebinding).
//!
//! Design rules (see the root `CLAUDE.md`):
//!
//! - **No per-protocol logic here.** Create routes through
//!   `server_startup::start_server_from_action` /
//!   `client_startup::start_client_from_action`; update reuses those same
//!   executors for the restart path. There is no per-protocol match statement.
//! - **Validate before mutating.** Startup parameters are validated against the
//!   protocol's `get_startup_parameters()` *before* the running instance is
//!   touched, so a bad update names the offending key (via `StartupParamError`,
//!   propagated with `?`) and leaves the old instance running.
//! - **Storage-free.** This is orchestration; it persists nothing itself.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::llm::actions::common::ServerTaskDefinition;
use crate::llm::actions::ParameterDefinition;
use crate::llm::OllamaClient;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ServerId};

/// The management "form": everything a caller can specify about a server
/// instance. Optional fields mean "unspecified" — on **create** an unspecified
/// field falls back to its protocol default, on **update** an unspecified field
/// is left unchanged.
///
/// This is the struct both create and update consume, so the two paths can
/// never drift apart on which fields exist.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerForm {
    /// Protocol name (e.g. "http", "tcp", "dns"). Required to create; on update
    /// it is immutable — supplying a different protocol is an error.
    pub protocol: String,
    /// Port to bind (socket protocols). `Some(0)` asks the OS to assign one.
    pub port: Option<u16>,
    /// Host/IP to bind (socket protocols). Defaults to the protocol default.
    pub host: Option<String>,
    /// Interface to bind (layer-2 / raw protocols such as arp, datalink, icmp).
    pub interface: Option<String>,
    /// Source MAC (layer-2 protocols), only meaningful with `interface`.
    pub mac_address: Option<String>,
    /// Speak first on connect (FTP/SMTP-style greeting). Only honoured by
    /// protocols that declare a `send_first` startup parameter.
    #[serde(default)]
    pub send_first: bool,
    /// Natural-language LLM instruction (the fallback handling path).
    pub instruction: Option<String>,
    /// Seed the instance's LLM memory.
    pub initial_memory: Option<String>,
    /// Protocol-specific startup parameters (validated against the schema).
    pub startup_params: Option<Value>,
    /// Deterministic event handlers (script / static / llm), matched in order.
    pub event_handlers: Option<Vec<Value>>,
    /// Scheduled LLM tasks scoped to this server.
    pub scheduled_tasks: Option<Vec<ServerTaskDefinition>>,
    /// Instructions for the automatic feedback loop.
    pub feedback_instructions: Option<String>,
}

/// The management form for a client instance. Mirrors [`ServerForm`] against the
/// `open_client` / `start_client` surface.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientForm {
    /// Protocol name (e.g. "redis", "http"). Required to create; immutable on
    /// update.
    pub protocol: String,
    /// Remote server address, "host:port". Required to create; changing it on
    /// update forces a reconnect (restart).
    pub remote_addr: Option<String>,
    /// Natural-language LLM instruction (the fallback handling path).
    pub instruction: Option<String>,
    /// Seed the client's LLM memory.
    pub initial_memory: Option<String>,
    /// Protocol-specific startup parameters (validated against the schema).
    pub startup_params: Option<Value>,
    /// Deterministic event handlers, matched in order.
    pub event_handlers: Option<Vec<Value>>,
    /// Scheduled LLM tasks scoped to this client.
    pub scheduled_tasks: Option<Vec<ServerTaskDefinition>>,
    /// Instructions for the automatic feedback loop.
    pub feedback_instructions: Option<String>,
}

/// Outcome of an update: which instance it now is, and whether applying it
/// required a stop+start (which drops live connections) rather than an in-place
/// change.
#[derive(Debug, Clone)]
pub struct UpdateOutcome {
    /// The instance id after the update. Equal to the input id for a hot update;
    /// a **new** id when a restart was required (stop+start allocates a fresh id).
    pub id: u32,
    /// True when the change forced a clean stop+start.
    pub restarted: bool,
    /// Human-readable summary of what changed, for the TUI / MCP / status stream.
    pub summary: String,
}

/// Declared startup parameters for a server protocol, or `None` if the protocol
/// is not compiled into this build. Used by the TUI create form to show what a
/// chosen protocol accepts.
pub fn server_declared_params(protocol: &str) -> Option<Vec<ParameterDefinition>> {
    crate::protocol::server_registry::registry()
        .resolve(protocol)
        .ok()
        .map(|p| p.get_startup_parameters())
}

/// Declared startup parameters for a client protocol, or `None` if it is not
/// compiled into this build.
pub fn client_declared_params(protocol: &str) -> Option<Vec<ParameterDefinition>> {
    crate::protocol::CLIENT_REGISTRY
        .resolve(protocol)
        .ok()
        .map(|p| p.get_startup_parameters())
}

/// Validate that every key in `params` was declared in the protocol's schema,
/// *before* any running instance is touched.
///
/// Delegates to the built-in [`StartupParams::new`] check — the same validation
/// `start_server_from_action` applies at spawn — so an undeclared key is reported
/// (naming the key, via `StartupParamError`) and the running instance is left
/// alone. Running it here, before `remove_server`, is what preserves the old
/// instance when a restart-triggering `startup_params` update is bad.
fn validate_params_declared(
    params: &Value,
    schema: &[ParameterDefinition],
    protocol: &str,
) -> Result<()> {
    crate::protocol::spawn_context::StartupParams::new(params.clone(), schema.to_vec())
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Invalid startup_params for {}: {}", protocol, e))
}

impl ServerForm {
    /// Create a brand new server from this form. Thin wrapper over the shared
    /// `start_server_from_action` executor — no logic is duplicated.
    pub async fn create(
        self,
        state: &AppState,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<ServerId> {
        let instruction = self.instruction.clone().unwrap_or_else(|| {
            format!(
                "You are a {} server. Handle requests appropriately.",
                self.protocol
            )
        });
        crate::cli::server_startup::start_server_from_action(
            state,
            self.mac_address,
            self.interface,
            self.host,
            self.port,
            &self.protocol,
            self.send_first,
            self.initial_memory,
            instruction,
            self.startup_params,
            self.event_handlers,
            self.scheduled_tasks,
            self.feedback_instructions,
            status_tx,
        )
        .await
    }
}

impl ClientForm {
    /// Create a brand new client from this form. Thin wrapper over the shared
    /// `start_client_from_action` executor.
    pub async fn create(
        self,
        state: &AppState,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<ClientId> {
        let _ = status_tx; // create logs via the client loop's own channel
        let remote_addr = self
            .remote_addr
            .clone()
            .ok_or_else(|| anyhow::anyhow!("remote_addr is required to open a client"))?;
        let instruction = self.instruction.clone().unwrap_or_else(|| {
            format!(
                "You are a {} client connected to {}. Handle responses appropriately.",
                self.protocol, remote_addr
            )
        });
        crate::cli::client_startup::start_client_from_action(
            state,
            &self.protocol,
            &remote_addr,
            instruction,
            self.startup_params,
            self.initial_memory,
            self.event_handlers,
            self.scheduled_tasks,
            self.feedback_instructions,
            llm_client,
        )
        .await
    }
}

/// Merge a partial `overlay` object into `base`, overlay keys winning. Returns
/// the merged object. Used so a partial `startup_params` update keeps the params
/// it does not mention.
fn merge_params(base: Option<&Value>, overlay: &Value) -> Value {
    let mut out = base
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    if let Some(map) = overlay.as_object() {
        for (k, v) in map {
            out.insert(k.clone(), v.clone());
        }
    }
    Value::Object(out)
}

/// Fields on a [`ServerForm`] whose change requires a rebind (a clean stop+start
/// rather than an in-place mutation).
fn server_needs_restart(form: &ServerForm) -> bool {
    form.port.is_some()
        || form.host.is_some()
        || form.interface.is_some()
        || form.mac_address.is_some()
        || form.startup_params.is_some()
}

/// Update a running server by id, applying only the fields the caller set.
///
/// - Unknown id → clean error, nothing mutated.
/// - Protocol mismatch → error (protocol is immutable; recreate to change it).
/// - `startup_params` / `event_handlers` are validated **before** the running
///   instance is touched, so a bad update leaves the old instance working.
/// - Hot fields (instruction, memory, event handlers, feedback instructions,
///   scheduled tasks) are applied in place, preserving live connections.
/// - Binding fields (port, host, interface, mac) or a `startup_params` change
///   force a clean stop+start; this drops connections and is reported as such.
pub async fn update_server(
    state: &AppState,
    server_id: ServerId,
    form: ServerForm,
    status_tx: mpsc::UnboundedSender<String>,
) -> Result<UpdateOutcome> {
    let current = state
        .get_server(server_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Server #{} not found", server_id.as_u32()))?;

    // Protocol is immutable on update.
    if !form.protocol.is_empty() && !form.protocol.eq_ignore_ascii_case(&current.protocol_name) {
        return Err(anyhow::anyhow!(
            "Cannot change protocol of server #{} from '{}' to '{}'. Stop it and open a new one instead.",
            server_id.as_u32(),
            current.protocol_name,
            form.protocol
        ));
    }

    // Look the protocol up in the registry (case-insensitive via `resolve`) so we
    // can read its declared startup-parameter schema for validation.
    let protocol_impl = crate::protocol::server_registry::registry()
        .resolve(&current.protocol_name)
        .map_err(|_| {
            anyhow::anyhow!("Server protocol '{}' not available", current.protocol_name)
        })?;

    // === Validate BEFORE mutating ===
    // Compute the merged startup params that would take effect, and validate the
    // whole thing against the schema. A bad key is reported (naming the key) and
    // the running server is left untouched.
    let merged_params: Option<Value> = match &form.startup_params {
        Some(overlay) => Some(merge_params(current.startup_params.as_ref(), overlay)),
        None => current.startup_params.clone(),
    };
    if let Some(params) = &merged_params {
        let schema = protocol_impl.get_startup_parameters();
        validate_params_declared(params, &schema, &current.protocol_name)?;
    }

    // Validate event handlers before mutating, too.
    if let Some(handlers) = &form.event_handlers {
        crate::events::handler::EventHandler::parse_event_handlers(handlers.clone())
            .map_err(|e| anyhow::anyhow!("Invalid event_handlers: {}", e))?;
    }

    if server_needs_restart(&form) {
        // === Restart path: clean stop + start with merged config ===
        let mut changed = Vec::new();
        if form.port.is_some() {
            changed.push("port");
        }
        if form.host.is_some() {
            changed.push("host");
        }
        if form.interface.is_some() {
            changed.push("interface");
        }
        if form.mac_address.is_some() {
            changed.push("mac_address");
        }
        if form.startup_params.is_some() {
            changed.push("startup_params");
        }

        // Merge current config with the requested overrides.
        let instruction = form
            .instruction
            .clone()
            .unwrap_or(current.instruction.clone());
        let initial_memory = match &form.initial_memory {
            Some(m) => Some(m.clone()),
            None if !current.memory.is_empty() => Some(current.memory.clone()),
            None => None,
        };
        // Event handlers: use the override, else re-serialize the current config
        // so a rebind does not silently drop deterministic handlers.
        let event_handlers: Option<Vec<Value>> = match &form.event_handlers {
            Some(h) => Some(h.clone()),
            None => current.event_handler_config.as_ref().and_then(|cfg| {
                serde_json::to_value(&cfg.handlers)
                    .ok()
                    .and_then(|v| v.as_array().cloned())
            }),
        };
        let feedback_instructions = form
            .feedback_instructions
            .clone()
            .or(current.feedback_instructions.clone());
        let port = form.port.or(Some(current.port));

        // Release the old socket and its tasks, then start fresh.
        state.remove_server(server_id).await;
        let _ = status_tx.send(format!(
            "[SERVER] Restarting server #{} ({}) to apply {}",
            server_id.as_u32(),
            current.protocol_name,
            changed.join(", ")
        ));

        let new_id = crate::cli::server_startup::start_server_from_action(
            state,
            form.mac_address,
            form.interface,
            form.host,
            port,
            &current.protocol_name,
            form.send_first,
            initial_memory,
            instruction,
            merged_params,
            event_handlers,
            form.scheduled_tasks,
            feedback_instructions,
            status_tx.clone(),
        )
        .await?;

        let summary = format!(
            "Server #{} restarted as #{} (changed: {}); live connections were dropped.",
            server_id.as_u32(),
            new_id.as_u32(),
            changed.join(", ")
        );
        let _ = status_tx.send(format!("[SERVER] {}", summary));
        let _ = status_tx.send("__UPDATE_UI__".to_string());
        return Ok(UpdateOutcome {
            id: new_id.as_u32(),
            restarted: true,
            summary,
        });
    }

    // === Hot path: mutate in place, connections preserved ===
    let mut changed = Vec::new();

    if let Some(instruction) = &form.instruction {
        state.set_instruction(server_id, instruction.clone()).await;
        changed.push("instruction");
    }
    if let Some(memory) = &form.initial_memory {
        state.set_memory(server_id, memory.clone()).await;
        changed.push("memory");
    }
    if let Some(handlers) = &form.event_handlers {
        // Already validated above.
        let config = crate::events::handler::EventHandler::parse_event_handlers(handlers.clone())
            .map_err(|e| anyhow::anyhow!("Invalid event_handlers: {}", e))?;
        state
            .with_server_mut(server_id, |s| {
                s.event_handler_config = Some(config);
            })
            .await;
        changed.push("event_handlers");
    }
    if let Some(fb) = &form.feedback_instructions {
        state
            .with_server_mut(server_id, |s| {
                s.feedback_instructions = Some(fb.clone());
            })
            .await;
        changed.push("feedback_instructions");
    }
    if let Some(tasks) = form.scheduled_tasks {
        let n = tasks.len();
        add_server_tasks(state, server_id, tasks).await;
        if n > 0 {
            changed.push("scheduled_tasks");
        }
    }

    let summary = if changed.is_empty() {
        format!("Server #{}: nothing to update.", server_id.as_u32())
    } else {
        format!(
            "Server #{} updated in place ({}); connections preserved.",
            server_id.as_u32(),
            changed.join(", ")
        )
    };
    let _ = status_tx.send(format!("[SERVER] {}", summary));
    let _ = status_tx.send("__UPDATE_UI__".to_string());

    Ok(UpdateOutcome {
        id: server_id.as_u32(),
        restarted: false,
        summary,
    })
}

/// Fields on a [`ClientForm`] whose change requires a reconnect.
fn client_needs_restart(form: &ClientForm) -> bool {
    form.remote_addr.is_some() || form.startup_params.is_some()
}

/// Update a running client by id. Mirror of [`update_server`]: unknown id and
/// bad params error cleanly without touching the instance; a `remote_addr` or
/// `startup_params` change reconnects, everything else is applied in place.
pub async fn update_client(
    state: &AppState,
    client_id: ClientId,
    form: ClientForm,
    llm_client: OllamaClient,
    status_tx: mpsc::UnboundedSender<String>,
) -> Result<UpdateOutcome> {
    let current = state
        .get_client(client_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Client #{} not found", client_id.as_u32()))?;

    if !form.protocol.is_empty() && !form.protocol.eq_ignore_ascii_case(&current.protocol_name) {
        return Err(anyhow::anyhow!(
            "Cannot change protocol of client #{} from '{}' to '{}'. Stop it and open a new one instead.",
            client_id.as_u32(),
            current.protocol_name,
            form.protocol
        ));
    }

    // Look the client protocol up (case-insensitive via `resolve`) for its
    // declared startup-parameter schema.
    let protocol_impl = crate::protocol::CLIENT_REGISTRY
        .resolve(&current.protocol_name)
        .map_err(|_| {
            anyhow::anyhow!("Client protocol '{}' not available", current.protocol_name)
        })?;

    // Validate merged params before mutating.
    let merged_params: Option<Value> = match &form.startup_params {
        Some(overlay) => Some(merge_params(current.startup_params.as_ref(), overlay)),
        None => current.startup_params.clone(),
    };
    if let Some(params) = &merged_params {
        let schema = protocol_impl.get_startup_parameters();
        validate_params_declared(params, &schema, &current.protocol_name)?;
    }

    // Validate event handlers before mutating (clients parse each handler value).
    if let Some(handlers) = &form.event_handlers {
        validate_client_handlers(handlers)?;
    }

    if client_needs_restart(&form) {
        let mut changed = Vec::new();
        if form.remote_addr.is_some() {
            changed.push("remote_addr");
        }
        if form.startup_params.is_some() {
            changed.push("startup_params");
        }

        let remote_addr = form
            .remote_addr
            .clone()
            .unwrap_or(current.remote_addr.clone());
        let instruction = form
            .instruction
            .clone()
            .unwrap_or(current.instruction.clone());
        let initial_memory = match &form.initial_memory {
            Some(m) => Some(m.clone()),
            None if !current.memory.is_empty() => Some(current.memory.clone()),
            None => None,
        };
        let event_handlers: Option<Vec<Value>> = match &form.event_handlers {
            Some(h) => Some(h.clone()),
            None => current.event_handler_config.as_ref().and_then(|cfg| {
                serde_json::to_value(&cfg.handlers)
                    .ok()
                    .and_then(|v| v.as_array().cloned())
            }),
        };
        let feedback_instructions = form
            .feedback_instructions
            .clone()
            .or(current.feedback_instructions.clone());

        // Release the old client (aborts its tasks / socket) then reconnect.
        state.remove_client(client_id).await;
        let _ = status_tx.send(format!(
            "[CLIENT] Reconnecting client #{} ({}) to apply {}",
            client_id.as_u32(),
            current.protocol_name,
            changed.join(", ")
        ));

        let new_id = crate::cli::client_startup::start_client_from_action(
            state,
            &current.protocol_name,
            &remote_addr,
            instruction,
            merged_params,
            initial_memory,
            event_handlers,
            form.scheduled_tasks,
            feedback_instructions,
            llm_client,
        )
        .await?;

        let summary = format!(
            "Client #{} reconnected as #{} (changed: {}).",
            client_id.as_u32(),
            new_id.as_u32(),
            changed.join(", ")
        );
        let _ = status_tx.send(format!("[CLIENT] {}", summary));
        let _ = status_tx.send("__UPDATE_UI__".to_string());
        return Ok(UpdateOutcome {
            id: new_id.as_u32(),
            restarted: true,
            summary,
        });
    }

    // Hot path.
    let mut changed = Vec::new();
    if let Some(instruction) = &form.instruction {
        state
            .set_instruction_for_client(client_id, instruction.clone())
            .await;
        changed.push("instruction");
    }
    if let Some(memory) = &form.initial_memory {
        state.set_memory_for_client(client_id, memory.clone()).await;
        changed.push("memory");
    }
    if let Some(handlers) = &form.event_handlers {
        let config = parse_client_handlers(handlers)?;
        state
            .with_client_mut(client_id, |c| {
                c.event_handler_config = Some(config);
            })
            .await;
        changed.push("event_handlers");
    }
    if let Some(fb) = &form.feedback_instructions {
        state
            .with_client_mut(client_id, |c| {
                c.feedback_instructions = Some(fb.clone());
            })
            .await;
        changed.push("feedback_instructions");
    }
    if let Some(tasks) = form.scheduled_tasks {
        let n = tasks.len();
        add_client_tasks(state, client_id, tasks).await;
        if n > 0 {
            changed.push("scheduled_tasks");
        }
    }

    let summary = if changed.is_empty() {
        format!("Client #{}: nothing to update.", client_id.as_u32())
    } else {
        format!(
            "Client #{} updated in place ({}).",
            client_id.as_u32(),
            changed.join(", ")
        )
    };
    let _ = status_tx.send(format!("[CLIENT] {}", summary));
    let _ = status_tx.send("__UPDATE_UI__".to_string());

    Ok(UpdateOutcome {
        id: client_id.as_u32(),
        restarted: false,
        summary,
    })
}

/// Validate client event handler JSON without mutating anything.
fn validate_client_handlers(handlers: &[Value]) -> Result<()> {
    parse_client_handlers(handlers).map(|_| ())
}

/// Parse client event handler JSON into an [`EventHandlerConfig`].
///
/// Clients deserialize each handler value directly (as `start_client_from_action`
/// does); an entry that fails to parse is a hard error rather than silently
/// dropped, so a bad update is reported instead of applied partially.
fn parse_client_handlers(handlers: &[Value]) -> Result<crate::scripting::EventHandlerConfig> {
    use crate::scripting::{EventHandler, EventHandlerConfig};
    let mut parsed = Vec::with_capacity(handlers.len());
    for (i, h) in handlers.iter().enumerate() {
        let handler: EventHandler = serde_json::from_value(h.clone())
            .map_err(|e| anyhow::anyhow!("Invalid event_handlers[{}]: {}", i, e))?;
        parsed.push(handler);
    }
    Ok(EventHandlerConfig { handlers: parsed })
}

/// Add scheduled tasks scoped to a server. Mirrors the task construction in
/// `start_server_from_action` so both create and update produce identical tasks.
async fn add_server_tasks(state: &AppState, server_id: ServerId, tasks: Vec<ServerTaskDefinition>) {
    use crate::state::task::TaskScope;
    for task_def in tasks {
        let task = build_task(TaskScope::Server(server_id), task_def);
        state.add_task(task).await;
    }
}

/// Add scheduled tasks scoped to a client.
async fn add_client_tasks(state: &AppState, client_id: ClientId, tasks: Vec<ServerTaskDefinition>) {
    use crate::state::task::TaskScope;
    for task_def in tasks {
        let task = build_task(TaskScope::Client(client_id), task_def);
        state.add_task(task).await;
    }
}

/// Build a [`ScheduledTask`] from a definition and scope — the same shape both
/// startup executors use.
fn build_task(
    scope: crate::state::task::TaskScope,
    task_def: ServerTaskDefinition,
) -> crate::state::task::ScheduledTask {
    use crate::state::task::{ScheduledTask, TaskId, TaskStatus, TaskType};
    use std::time::{Duration, Instant};

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
    let delay = if task_def.recurring {
        Duration::from_secs(0)
    } else {
        Duration::from_secs(task_def.delay_secs.unwrap_or(0))
    };
    ScheduledTask {
        id: TaskId::new(rand::random()),
        name: task_def.task_id,
        scope,
        task_type,
        instruction: task_def.instruction,
        context: task_def.context,
        status: TaskStatus::Scheduled,
        created_at: Instant::now(),
        next_execution: Instant::now() + delay,
        last_error: None,
        failure_count: 0,
    }
}
