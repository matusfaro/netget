//! Common actions available in all prompts
//!
//! This module defines actions that are available in both user input
//! and network event prompts (show_message, memory operations, etc.).

use super::{ActionDefinition, Parameter};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Task definition for attaching to a server at creation time
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerTaskDefinition {
    pub task_id: String,
    pub recurring: bool,
    #[serde(default)]
    pub delay_secs: Option<u64>,
    #[serde(default)]
    pub interval_secs: Option<u64>,
    #[serde(default)]
    pub max_executions: Option<u64>,
    pub instruction: String,
    #[serde(default)]
    pub context: Option<serde_json::Value>,
    // Script configuration fields
    #[serde(default)]
    pub script_language: Option<String>,
    #[serde(default)]
    pub script_path: Option<String>,
    #[serde(default)]
    pub script_inline: Option<String>,
    #[serde(default)]
    pub script_handles: Option<Vec<String>>,
}

/// Common actions available in all contexts
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommonAction {
    /// Display a message to the user
    ShowMessage { message: String },

    /// Open a new server
    OpenServer {
        port: u16,
        base_stack: String,
        #[serde(default)]
        send_first: bool,
        #[serde(default)]
        initial_memory: Option<String>,
        instruction: String,
        #[serde(default)]
        startup_params: Option<serde_json::Value>,
        // Script configuration fields
        #[serde(default)]
        script_language: Option<String>,
        #[serde(default)]
        script_path: Option<String>,
        #[serde(default)]
        script_inline: Option<String>,
        #[serde(default)]
        script_handles: Option<Vec<String>>,
        // Scheduled tasks to create with this server
        #[serde(default)]
        scheduled_tasks: Option<Vec<ServerTaskDefinition>>,
    },

    /// Close a server (closes all if server_id not specified)
    CloseServer {
        #[serde(default)]
        server_id: Option<u32>,
    },

    /// Update the server instruction (combines with existing)
    UpdateInstruction { instruction: String },

    /// Change the LLM model
    ChangeModel { model: String },

    /// Replace global memory completely
    SetMemory { value: String },

    /// Append to global memory
    AppendMemory { value: String },

    /// Update script configuration for a running server
    UpdateScript {
        #[serde(default)]
        server_id: Option<u32>,
        operation: String,
        #[serde(default)]
        script_language: Option<String>,
        #[serde(default)]
        script_path: Option<String>,
        #[serde(default)]
        script_inline: Option<String>,
        #[serde(default)]
        script_handles: Option<Vec<String>>,
    },

    /// Append content to a log file
    AppendToLog {
        output_name: String,
        content: String,
    },

    /// Schedule a task (one-shot or recurring)
    ScheduleTask {
        task_id: String,
        recurring: bool,
        #[serde(default)]
        delay_secs: Option<u64>,
        #[serde(default)]
        interval_secs: Option<u64>,
        #[serde(default)]
        max_executions: Option<u64>,
        #[serde(default)]
        server_id: Option<u32>,
        instruction: String,
        #[serde(default)]
        context: Option<serde_json::Value>,
        // Script configuration fields
        #[serde(default)]
        script_language: Option<String>,
        #[serde(default)]
        script_path: Option<String>,
        #[serde(default)]
        script_inline: Option<String>,
        #[serde(default)]
        script_handles: Option<Vec<String>>,
    },

    /// Cancel a scheduled task
    CancelTask { task_id: String },

    /// List all scheduled tasks
    ListTasks,
}

impl CommonAction {
    /// Parse from JSON value
    pub fn from_json(value: &serde_json::Value) -> Result<Self> {
        serde_json::from_value(value.clone()).context("Failed to parse common action")
    }
}

/// Get action definition for show_message
pub fn show_message_action() -> ActionDefinition {
    ActionDefinition {
        name: "show_message".to_string(),
        description: "Display a message to the user controlling NetGet".to_string(),
        parameters: vec![Parameter {
            name: "message".to_string(),
            type_hint: "string".to_string(),
            description: "Message to display".to_string(),
            required: true,
        }],
        example: json!({
            "type": "show_message",
            "message": "Server started successfully on port 8080"
        }),
    }
}

/// Get action definition for open_server
pub fn open_server_action(selected_mode: crate::state::app_state::ScriptingMode) -> ActionDefinition {
    let mut parameters = vec![
        Parameter {
            name: "port".to_string(),
            type_hint: "number".to_string(),
            description: "Port number to listen on".to_string(),
            required: true,
        },
        Parameter {
            name: "base_stack".to_string(),
            type_hint: "string".to_string(),
            description: format!("Protocol stack to use. IMPORTANT: Match the protocol name from the user prompt exactly (e.g., if prompt says 'gRPC server', use 'gRPC', not 'HTTP'). Available: {}", all_base_stacks(false).join(", ")),
            required: true,
        },
        Parameter {
            name: "send_first".to_string(),
            type_hint: "boolean".to_string(),
            description: "True if server sends data first (FTP, SMTP), false if it waits for client (HTTP)".to_string(),
            required: false,
        },
        Parameter {
            name: "initial_memory".to_string(),
            type_hint: "string".to_string(),
            description: "Optional initial memory as a string. Use for storing persistent context across connections. Example: \"user_count: 0\"".to_string(),
            required: false,
        },
        Parameter {
            name: "instruction".to_string(),
            type_hint: "string".to_string(),
            description: "Detailed instructions for handling network events".to_string(),
            required: true,
        },
        Parameter {
            name: "startup_params".to_string(),
            type_hint: "object".to_string(),
            description: "Optional protocol-specific startup parameters. See protocol documentation for available parameters.".to_string(),
            required: false,
        },
        Parameter {
            name: "scheduled_tasks".to_string(),
            type_hint: "array".to_string(),
            description: "Optional: Array of scheduled tasks to create with this server. Each task will be attached to the server and execute at specified intervals or delays. Tasks are automatically cleaned up when the server stops. Each task has: task_id, recurring (boolean), delay_secs (for one-shot or initial delay), interval_secs (for recurring), max_executions (optional), instruction, context (optional), and optional script fields (script_inline, script_handles).".to_string(),
            required: false,
        },
    ];

    // Add script parameters if scripting is enabled
    if selected_mode != crate::state::app_state::ScriptingMode::Llm {
        let lang = selected_mode.as_str().to_lowercase();
        parameters.extend(vec![
            Parameter {
                name: "script_inline".to_string(),
                type_hint: "string".to_string(),
                description: format!(
                    "Optional: Inline {} script code to handle deterministic responses instead of LLM. If provided, the script will be executed for network events.",
                    lang
                ),
                required: false,
            },
            Parameter {
                name: "script_handles".to_string(),
                type_hint: "array".to_string(),
                description: "Optional: Context types the script handles, e.g. [\"ssh_auth\", \"ssh_banner\"] or [\"all\"]. Defaults to [\"all\"].".to_string(),
                required: false,
            },
        ]);
    }

    ActionDefinition {
        name: "open_server".to_string(),
        description: "Start a new server".to_string(),
        parameters,
        example: json!({
            "type": "open_server",
            "port": 21,
            "base_stack": "tcp",
            "send_first": true,
            "initial_memory": "login_count: 0\nfiles: data.txt,readme.md",
            "instruction": "You are an FTP server. Respond to FTP commands like USER, PASS, LIST, RETR, QUIT with appropriate FTP response codes."
        }),
    }
}

/// Get action definition for close_server
pub fn close_server_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_server".to_string(),
        description: "Stop the current server".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_server"
        }),
    }
}

/// Get action definition for update_instruction
pub fn update_instruction_action() -> ActionDefinition {
    ActionDefinition {
        name: "update_instruction".to_string(),
        description: "Update the current server instruction (combines with existing instruction)"
            .to_string(),
        parameters: vec![Parameter {
            name: "instruction".to_string(),
            type_hint: "string".to_string(),
            description: "New instruction to add/combine".to_string(),
            required: true,
        }],
        example: json!({
            "type": "update_instruction",
            "instruction": "For all HTTP requests, return status 404 with 'Not Found' message."
        }),
    }
}

/// Get action definition for change_model
pub fn change_model_action() -> ActionDefinition {
    ActionDefinition {
        name: "change_model".to_string(),
        description: "Switch to a different LLM model".to_string(),
        parameters: vec![Parameter {
            name: "model".to_string(),
            type_hint: "string".to_string(),
            description: "Model name (e.g., 'llama3.2:latest')".to_string(),
            required: true,
        }],
        example: json!({
            "type": "change_model",
            "model": "llama3.2:latest"
        }),
    }
}

/// Get action definition for set_memory
pub fn set_memory_action() -> ActionDefinition {
    ActionDefinition {
        name: "set_memory".to_string(),
        description: "Replace the entire global memory with new content. Any existing memory is discarded. Use this to reset or completely rewrite memory state.".to_string(),
        parameters: vec![
            Parameter {
                name: "value".to_string(),
                type_hint: "string".to_string(),
                description: "New memory value as a string. Replaces all existing memory.".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "set_memory",
            "value": "session_id: abc123\nuser_preferences: dark_mode=true\nlast_command: LIST"
        }),
    }
}

/// Get action definition for append_memory
pub fn append_memory_action() -> ActionDefinition {
    ActionDefinition {
        name: "append_memory".to_string(),
        description: "Add new content to the end of global memory. Existing memory is preserved and a newline is automatically added before the new content. Use this to incrementally build up memory state.".to_string(),
        parameters: vec![
            Parameter {
                name: "value".to_string(),
                type_hint: "string".to_string(),
                description: "Text to append as a string. Will be added after existing memory with newline separator.".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "append_memory",
            "value": "connection_count: 5\nlast_file_requested: readme.md"
        }),
    }
}

/// Get action definition for update_script
pub fn update_script_action(selected_mode: crate::state::app_state::ScriptingMode) -> ActionDefinition {
    let lang = selected_mode.as_str().to_lowercase();
    ActionDefinition {
        name: "update_script".to_string(),
        description: "Update or modify script configuration for a running server. Use this to change authentication logic, add/remove context types, or disable scripts entirely.".to_string(),
        parameters: vec![
            Parameter {
                name: "server_id".to_string(),
                type_hint: "number".to_string(),
                description: "Optional: Server ID to update (defaults to first/current server)".to_string(),
                required: false,
            },
            Parameter {
                name: "operation".to_string(),
                type_hint: "string".to_string(),
                description: "Operation: 'set' (replace entire config), 'add_contexts' (add context types), 'remove_contexts' (remove context types), or 'disable' (remove script, use LLM only)".to_string(),
                required: true,
            },
            Parameter {
                name: "script_inline".to_string(),
                type_hint: "string".to_string(),
                description: format!("Inline {} script code (required for 'set' operation)", lang),
                required: false,
            },
            Parameter {
                name: "script_handles".to_string(),
                type_hint: "array".to_string(),
                description: "Context types to handle (for 'set' or 'add_contexts'/'remove_contexts')".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "update_script",
            "server_id": 1,
            "operation": "set",
            "script_inline": "import json\nimport sys\ndata=json.load(sys.stdin)\nprint(json.dumps({'actions':[{'type':'show_message','message':'Updated!'}]}))",
            "script_handles": ["ssh_auth"]
        }),
    }
}

/// Get action definition for append_to_log
pub fn append_to_log_action() -> ActionDefinition {
    ActionDefinition {
        name: "append_to_log".to_string(),
        description: "Append content to a log file. Log files are named 'netget_<output_name>_<timestamp>.log' where timestamp is when the server was started. Each append operation adds the content to the end of the file with a newline. Use this to create access logs, audit trails, or any persistent logging.".to_string(),
        parameters: vec![
            Parameter {
                name: "output_name".to_string(),
                type_hint: "string".to_string(),
                description: "Name of the log output (e.g., 'access_logs'). Used to construct the log filename.".to_string(),
                required: true,
            },
            Parameter {
                name: "content".to_string(),
                type_hint: "string".to_string(),
                description: "Content to append to the log file.".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "append_to_log",
            "output_name": "access_logs",
            "content": "127.0.0.1 - - [29/Oct/2025:12:34:56 +0000] \"GET /index.html HTTP/1.1\" 200 1234"
        }),
    }
}

/// Get action definition for schedule_task
pub fn schedule_task_action(selected_mode: crate::state::app_state::ScriptingMode) -> ActionDefinition {
    let mut parameters = vec![
        Parameter {
            name: "task_id".to_string(),
            type_hint: "string".to_string(),
            description: "Unique identifier for this task (e.g., 'cleanup_logs', 'sse_heartbeat'). Used to reference or cancel the task later.".to_string(),
            required: true,
        },
        Parameter {
            name: "recurring".to_string(),
            type_hint: "boolean".to_string(),
            description: "True for recurring task (executes at intervals), false for one-shot task (executes once after delay).".to_string(),
            required: true,
        },
        Parameter {
            name: "delay_secs".to_string(),
            type_hint: "number".to_string(),
            description: "For one-shot tasks (recurring=false): delay in seconds before executing. For recurring tasks: optional initial delay before first execution (defaults to interval_secs if not provided).".to_string(),
            required: false,
        },
        Parameter {
            name: "interval_secs".to_string(),
            type_hint: "number".to_string(),
            description: "For recurring tasks (recurring=true): interval in seconds between executions. Required when recurring=true.".to_string(),
            required: false,
        },
        Parameter {
            name: "max_executions".to_string(),
            type_hint: "number".to_string(),
            description: "For recurring tasks: maximum number of times to execute. If omitted, task runs indefinitely until cancelled.".to_string(),
            required: false,
        },
        Parameter {
            name: "server_id".to_string(),
            type_hint: "number".to_string(),
            description: "Optional: Server ID to scope this task to. If provided, task uses server's instruction and protocol actions. If omitted, task is global and uses user input actions.".to_string(),
            required: false,
        },
        Parameter {
            name: "instruction".to_string(),
            type_hint: "string".to_string(),
            description: "Instruction/prompt for LLM when task executes. Describes what the task should do.".to_string(),
            required: true,
        },
        Parameter {
            name: "context".to_string(),
            type_hint: "object".to_string(),
            description: "Optional: Additional context data to pass to LLM when task executes (e.g., thresholds, parameters).".to_string(),
            required: false,
        },
    ];

    // Add script parameters if scripting is enabled
    if selected_mode != crate::state::app_state::ScriptingMode::Llm {
        let lang = selected_mode.as_str().to_lowercase();
        parameters.extend(vec![
            Parameter {
                name: "script_inline".to_string(),
                type_hint: "string".to_string(),
                description: format!(
                    "Optional: Inline {} script code to handle task execution instead of LLM. If provided, the script will be executed for each task trigger.",
                    lang
                ),
                required: false,
            },
            Parameter {
                name: "script_handles".to_string(),
                type_hint: "array".to_string(),
                description: "Optional: Event types the script handles (e.g., [\"scheduled_task_cleanup\"]). Defaults to [\"all\"].".to_string(),
                required: false,
            },
        ]);
    }

    ActionDefinition {
        name: "schedule_task".to_string(),
        description: "Schedule a task (one-shot or recurring). The task will call the LLM or execute a script with the provided instruction. One-shot tasks execute once after a delay and are automatically removed. Recurring tasks execute at intervals until cancelled or max_executions is reached. Useful for delayed operations, timeouts, periodic health checks, heartbeats, SSE messages, metrics collection, etc.".to_string(),
        parameters,
        example: json!({
            "type": "schedule_task",
            "task_id": "sse_heartbeat",
            "recurring": true,
            "interval_secs": 30,
            "server_id": 1,
            "instruction": "Send SSE heartbeat to all active connections"
        }),
    }
}

/// Get action definition for cancel_task
pub fn cancel_task_action() -> ActionDefinition {
    ActionDefinition {
        name: "cancel_task".to_string(),
        description: "Cancel a scheduled task by its task_id. Works for both one-shot and recurring tasks. The task is immediately removed and will not execute again.".to_string(),
        parameters: vec![Parameter {
            name: "task_id".to_string(),
            type_hint: "string".to_string(),
            description: "ID of the task to cancel (the task_id used when scheduling).".to_string(),
            required: true,
        }],
        example: json!({
            "type": "cancel_task",
            "task_id": "cleanup_logs"
        }),
    }
}

/// Get action definition for list_tasks
pub fn list_tasks_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_tasks".to_string(),
        description: "List all currently scheduled tasks. Returns information about all one-shot and recurring tasks, including their status, next execution time, and configuration.".to_string(),
        parameters: vec![],
        example: json!({
            "type": "list_tasks"
        }),
    }
}

/// Get all common action definitions
///
/// Actions are organized logically:
/// 1. Server Management - Create/destroy servers
/// 2. Server Configuration - Configure running servers
/// 3. Task Management - Schedule/cancel tasks
/// 4. System/Utility - Model changes, messages, logging
pub fn get_all_common_actions(selected_mode: crate::state::app_state::ScriptingMode) -> Vec<ActionDefinition> {
    let mut actions = vec![
        // === Server Management ===
        get_open_server_action_with_params(selected_mode),
        close_server_action(),
        // === Server Configuration ===
        update_instruction_action(),
        set_memory_action(),
        append_memory_action(),
        // === Task Management ===
        schedule_task_action(selected_mode),
        cancel_task_action(),
        list_tasks_action(),
        // === System/Utility ===
        change_model_action(),
        show_message_action(),
        append_to_log_action(),
    ];

    // Only include update_script if scripting is enabled
    if selected_mode != crate::state::app_state::ScriptingMode::Llm {
        actions.insert(4, update_script_action(selected_mode)); // Insert after update_instruction
    }

    actions
}

/// Get common actions for user input (all common actions with enhanced open_server)
pub fn get_user_input_common_actions(selected_mode: crate::state::app_state::ScriptingMode) -> Vec<ActionDefinition> {
    get_all_common_actions(selected_mode)
}

/// Get common actions for network events (exclude server management actions)
///
/// Network events only get server configuration and utility actions,
/// not server management (can't create/destroy servers from event handlers).
pub fn get_network_event_common_actions() -> Vec<ActionDefinition> {
    vec![
        // === Server Configuration ===
        set_memory_action(),
        append_memory_action(),
        // === System/Utility ===
        show_message_action(),
        append_to_log_action(),
    ]
}


/// Get all protocol names that should be available to the LLM
/// Filters out protocols with ProtocolState::Disabled unless include_disabled is true
/// Dynamically determined from the ProtocolRegistry based on compiled features
fn all_base_stacks(include_disabled: bool) -> Vec<String> {
    let registry = crate::protocol::registry::registry();

    // Get all registered protocols from the registry (only includes compiled features)
    let mut protocols: Vec<String> = registry
        .all_protocols()
        .into_iter()
        .map(|(protocol_name, _protocol)| protocol_name)
        .filter(|protocol_name| {
            if include_disabled {
                // Include all protocols when flag is set
                true
            } else {
                // Only include protocols that are available to LLM (not Disabled)
                registry
                    .metadata(protocol_name)
                    .map(|m| m.is_available_to_llm())
                    .unwrap_or(true)
            }
        })
        .collect();

    // Sort protocols alphabetically for deterministic output
    protocols.sort();
    protocols
}

/// Generate comprehensive base stack documentation with startup parameters
/// Returns formatted text listing all available stacks and their configuration options
pub fn generate_base_stack_documentation(include_disabled: bool) -> String {
    let mut doc = String::from("## Available Base Stacks\n\n");

    // Group protocols by their group_name
    let registry = crate::protocol::registry::registry();
    let mut groups: std::collections::HashMap<&'static str, Vec<(String, std::sync::Arc<dyn crate::llm::actions::Server>)>> = std::collections::HashMap::new();

    for protocol_name in all_base_stacks(include_disabled) {
        if let Some(protocol) = registry.get(&protocol_name) {
            let group = protocol.group_name();
            groups.entry(group).or_insert_with(Vec::new).push((protocol_name.clone(), protocol));
        }
    }

    // Sort groups by predefined order
    let group_order = vec![
        "Core",
        "Application",
        "Database",
        "Web & File",
        "Proxy & Network",
        "AI & API",
        "Other"
    ];

    for group_name in group_order {
        if let Some(protocols) = groups.get_mut(group_name) {
            if protocols.is_empty() {
                continue;
            }

            // Output group header
            doc.push_str(&format!("### {}\n\n", group_name));

            // Sort protocols alphabetically within group
            protocols.sort_by(|a, b| a.0.cmp(&b.0));

            // Output full details for each protocol in the group
            for (protocol_name, protocol) in protocols {
                // Protocol header (short name)
                doc.push_str(&format!("**{}**\n", protocol_name));

                // Full stack name
                let stack_name = protocol.stack_name();
                doc.push_str(&format!("Full name: \"{}\"\n", stack_name));

                // Description
                doc.push_str(&format!("Description: {}\n", protocol.description()));

                // Example prompt
                doc.push_str(&format!("Example: \"{}\"\n", protocol.example_prompt()));

                // Startup parameters
                let params = protocol.get_startup_parameters();
                if !params.is_empty() {
                    doc.push_str("Startup parameters:\n");
                    for param in params {
                        doc.push_str(&format!(
                            "  • {} ({}) - {}\n",
                            param.name,
                            if param.required {
                                "required"
                            } else {
                                "optional"
                            },
                            param.description
                        ));
                        doc.push_str(&format!(
                            "    Example: {}\n",
                            serde_json::to_string(&param.example).unwrap_or_default()
                        ));
                    }
                } else {
                    doc.push_str("Startup parameters: None\n");
                }
                doc.push('\n');
            }
        }
    }

    doc
}

/// Get open_server action with example showing startup_params usage
///
/// Startup parameter documentation is provided in the base stack documentation section,
/// not inline here, to avoid redundancy and reduce token usage.
pub fn get_open_server_action_with_params(selected_mode: crate::state::app_state::ScriptingMode) -> ActionDefinition {
    let mut base_action = open_server_action(selected_mode);

    // Use example that shows startup_params and scheduled_tasks usage
    base_action.example = json!({
        "type": "open_server",
        "port": 8080,
        "base_stack": "http",
        "instruction": "HTTP server with SSE support",
        "startup_params": {},
        "scheduled_tasks": [
            {
                "task_id": "sse_heartbeat",
                "recurring": true,
                "interval_secs": 30,
                "instruction": "Send SSE heartbeat to all active connections"
            },
            {
                "task_id": "cleanup",
                "recurring": false,
                "delay_secs": 3600,
                "instruction": "Clean up idle connections older than 1 hour"
            }
        ]
    });

    base_action
}
