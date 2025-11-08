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
        // Event handler configuration
        #[serde(default)]
        event_handlers: Option<Vec<serde_json::Value>>,
        // Scheduled tasks to create with this server
        #[serde(default)]
        scheduled_tasks: Option<Vec<ServerTaskDefinition>>,
    },

    /// Close a specific server
    CloseServer {
        server_id: u32,
    },

    /// Close all servers
    CloseAllServers,

    /// Open a new client connection
    OpenClient {
        protocol: String,
        remote_addr: String,
        instruction: String,
        #[serde(default)]
        startup_params: Option<serde_json::Value>,
        #[serde(default)]
        initial_memory: Option<String>,
        // Event handler configuration
        #[serde(default)]
        event_handlers: Option<Vec<serde_json::Value>>,
        // Scheduled tasks to create with this client
        #[serde(default)]
        scheduled_tasks: Option<Vec<ServerTaskDefinition>>,
    },

    /// Close a specific client
    CloseClient {
        client_id: u32,
    },

    /// Close all clients
    CloseAllClients,

    /// Reconnect a disconnected client
    ReconnectClient {
        client_id: u32,
    },

    /// Update the client instruction
    UpdateClientInstruction {
        client_id: u32,
        instruction: String,
    },

    /// Update the server instruction (combines with existing)
    UpdateInstruction { instruction: String },

    /// Change the LLM model
    ChangeModel { model: String },

    /// Replace global memory completely
    SetMemory { value: String },

    /// Append to global memory
    AppendMemory { value: String },

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
        #[serde(default)]
        connection_id: Option<String>,
        #[serde(default)]
        client_id: Option<u32>,
        instruction: String,
        #[serde(default)]
        context: Option<serde_json::Value>,
        // Script configuration fields
        #[serde(default)]
        script_runtime: Option<String>,
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
pub fn open_server_action(
    _selected_mode: crate::state::app_state::ScriptingMode,
    env: &crate::scripting::ScriptingEnvironment,
    is_enabled: bool,
) -> ActionDefinition {
    let name = "open_server".to_string();
    let mut description = "Start a new server.".to_string();

    if !is_enabled {
        description.push_str(" ⚠️ DISABLED: You must call read_base_stack_docs tool call first to enable this action. This tool provides detailed protocol documentation and startup parameters required for server configuration.");
        return ActionDefinition {
            name,
            description,
            parameters: vec![],
            example: json!({}),
        };
    }

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
                description: format!("Protocol stack to use. Choose the best stack for the task. Available: {}", all_base_stacks(false).join(", ")),
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
                description: "Optional: Array of scheduled tasks to create with this server. Each task will be attached to the server and execute at specified intervals or delays. Tasks are automatically cleaned up when the server stops. Each task has: task_id, recurring (boolean), delay_secs (for one-shot or initial delay), interval_secs (for recurring), max_executions (optional), instruction, context (optional).".to_string(),
                required: false,
            },
        ];

    // Add event_handlers parameter with description based on handler mode
    let handler_mode_guidance = "You can configure different handlers for different events. Each handler specifies an event_pattern (specific event ID or \"*\" for all events) and a handler type (script, static, or llm). Handlers are matched in order - first match wins.";

    let available_runtimes = env.format_available();
    let event_handlers_description = format!(
        "Optional: Array of event handlers to configure how events are processed. {}\\n\\nEach handler has:\\n- event_pattern: Event ID to match (e.g., \\\"tcp_data_received\\\") or \\\"*\\\" for all events\\n- handler: Object with:\\n  - type: \\\"script\\\" (inline code), \\\"static\\\" (predefined actions), or \\\"llm\\\" (dynamic processing)\\n  - For script: language ({}), code (inline script)\\n  - For static: actions (array of action objects)\\n\\nExample script handler: {{\\\"event_pattern\\\": \\\"ssh_auth\\\", \\\"handler\\\": {{\\\"type\\\": \\\"script\\\", \\\"language\\\": \\\"python\\\", \\\"code\\\": \\\"import json,sys;data=json.load(sys.stdin);print(json.dumps({{'actions':[{{'type':'send_data','data':'OK'}}]}}))\\\"}}}}\\n\\nExample static handler: {{\\\"event_pattern\\\": \\\"*\\\", \\\"handler\\\": {{\\\"type\\\": \\\"static\\\", \\\"actions\\\": [{{\\\"type\\\": \\\"send_data\\\", \\\"data\\\": \\\"Welcome\\\"}}]}}}}\\n\\nExample LLM handler: {{\\\"event_pattern\\\": \\\"http_request\\\", \\\"handler\\\": {{\\\"type\\\": \\\"llm\\\"}}}}",
        handler_mode_guidance,
        available_runtimes
    );

    parameters.push(Parameter {
        name: "event_handlers".to_string(),
        type_hint: "array".to_string(),
        description: event_handlers_description,
        required: false,
    });

    let example = json!({
        "type": "open_server",
        "port": 21,
        "base_stack": "tcp",
        "send_first": true,
        "initial_memory": "login_count: 0\nfiles: data.txt,readme.md",
        "instruction": "You are an FTP server. Respond to FTP commands like USER, PASS, LIST, RETR, QUIT with appropriate FTP response codes."
    });

    ActionDefinition {
        name,
        description,
        parameters,
        example,
    }
}

/// Get action definition for close_server
pub fn close_server_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_server".to_string(),
        description: "Stop a specific server by ID.".to_string(),
        parameters: vec![Parameter {
            name: "server_id".to_string(),
            type_hint: "number".to_string(),
            description: "Server ID to close (e.g., 1, 2).".to_string(),
            required: true,
        }],
        example: json!({
            "type": "close_server",
            "server_id": 1
        }),
    }
}

/// Get action definition for close_all_servers
pub fn close_all_servers_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_all_servers".to_string(),
        description: "Stop all running servers.".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_all_servers"
        }),
    }
}

/// Get action definition for open_client
pub fn open_client_action(
    _selected_mode: crate::state::app_state::ScriptingMode,
    env: &crate::scripting::ScriptingEnvironment,
    is_enabled: bool,
) -> ActionDefinition {
    let name = "open_client".to_string();
    let mut description = "Connect to a remote server as a client.".to_string();

    if !is_enabled {
        description.push_str(" ⚠️ DISABLED: You must call read_base_stack_docs tool call first to enable this action. This tool provides detailed protocol documentation and startup parameters required for client configuration.");
        return ActionDefinition {
            name,
            description,
            parameters: vec![],
            example: json!({}),
        };
    }

    let mut parameters = vec![
        Parameter {
            name: "protocol".to_string(),
            type_hint: "string".to_string(),
            description: "Protocol to use for connection (e.g., 'tcp', 'http', 'redis', 'ssh')".to_string(),
            required: true,
        },
        Parameter {
            name: "remote_addr".to_string(),
            type_hint: "string".to_string(),
            description: "Remote server address as 'hostname:port' or 'IP:port' (e.g., 'example.com:80', '192.168.1.1:6379', 'localhost:8080')".to_string(),
            required: true,
        },
        Parameter {
            name: "instruction".to_string(),
            type_hint: "string".to_string(),
            description: "Detailed instructions for controlling the client (how to send data, interpret responses, make decisions)".to_string(),
            required: true,
        },
        Parameter {
            name: "initial_memory".to_string(),
            type_hint: "string".to_string(),
            description: "Optional initial memory as a string. Use for storing persistent context. Example: \"auth_token: abc123\\nrequest_count: 0\"".to_string(),
            required: false,
        },
        Parameter {
            name: "startup_params".to_string(),
            type_hint: "object".to_string(),
            description: "Optional protocol-specific startup parameters. For example, HTTP clients may accept default headers or user agent settings.".to_string(),
            required: false,
        },
        Parameter {
            name: "scheduled_tasks".to_string(),
            type_hint: "array".to_string(),
            description: "Optional: Array of scheduled tasks to create with this client. Each task will be attached to the client and execute at specified intervals or delays. Tasks are automatically cleaned up when the client disconnects.".to_string(),
            required: false,
        },
    ];

    // Add event_handlers parameter
    let handler_mode_guidance = "You can configure different handlers for different client events. Each handler specifies an event_pattern (specific event ID or \"*\" for all events) and a handler type (script, static, or llm). Handlers are matched in order - first match wins.";

    let available_runtimes = env.format_available();
    let event_handlers_description = format!(
        "Optional: Array of event handlers to configure how client events are processed. {}\\n\\nEach handler has:\\n- event_pattern: Event ID to match (e.g., \\\"http_response_received\\\") or \\\"*\\\" for all events\\n- handler: Object with:\\n  - type: \\\"script\\\" (inline code), \\\"static\\\" (predefined actions), or \\\"llm\\\" (dynamic processing)\\n  - For script: language ({}), code (inline script)\\n  - For static: actions (array of action objects)\\n\\nExample script handler: {{\\\"event_pattern\\\": \\\"redis_response_received\\\", \\\"handler\\\": {{\\\"type\\\": \\\"script\\\", \\\"language\\\": \\\"python\\\", \\\"code\\\": \\\"import json,sys;data=json.load(sys.stdin);print(json.dumps({{'actions':[{{'type':'execute_redis_command','command':'PING'}}]}}))\\\"}}}}\\n\\nExample static handler: {{\\\"event_pattern\\\": \\\"*\\\", \\\"handler\\\": {{\\\"type\\\": \\\"static\\\", \\\"actions\\\": [{{\\\"type\\\": \\\"send_http_request\\\", \\\"method\\\": \\\"GET\\\", \\\"path\\\": \\\"/\\\"}}]}}}}",
        handler_mode_guidance,
        available_runtimes
    );

    parameters.push(Parameter {
        name: "event_handlers".to_string(),
        type_hint: "array".to_string(),
        description: event_handlers_description,
        required: false,
    });

    let example = json!({
        "type": "open_client",
        "protocol": "http",
        "remote_addr": "example.com:80",
        "instruction": "Send a GET request to /api/status and log the response code."
    });

    ActionDefinition {
        name,
        description,
        parameters,
        example,
    }
}

/// Get action definition for close_client
pub fn close_client_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_client".to_string(),
        description: "Disconnect a specific client by ID.".to_string(),
        parameters: vec![Parameter {
            name: "client_id".to_string(),
            type_hint: "number".to_string(),
            description: "Client ID to close (e.g., 1, 2).".to_string(),
            required: true,
        }],
        example: json!({
            "type": "close_client",
            "client_id": 1
        }),
    }
}

/// Get action definition for close_all_clients
pub fn close_all_clients_action() -> ActionDefinition {
    ActionDefinition {
        name: "close_all_clients".to_string(),
        description: "Disconnect all active clients.".to_string(),
        parameters: vec![],
        example: json!({
            "type": "close_all_clients"
        }),
    }
}

/// Get action definition for reconnect_client
pub fn reconnect_client_action() -> ActionDefinition {
    ActionDefinition {
        name: "reconnect_client".to_string(),
        description: "Reconnect a disconnected client to its remote server.".to_string(),
        parameters: vec![Parameter {
            name: "client_id".to_string(),
            type_hint: "number".to_string(),
            description: "Client ID to reconnect (e.g., 1, 2).".to_string(),
            required: true,
        }],
        example: json!({
            "type": "reconnect_client",
            "client_id": 1
        }),
    }
}

/// Get action definition for update_client_instruction
pub fn update_client_instruction_action() -> ActionDefinition {
    ActionDefinition {
        name: "update_client_instruction".to_string(),
        description: "Update the instruction for a specific client (replaces existing instruction).".to_string(),
        parameters: vec![
            Parameter {
                name: "client_id".to_string(),
                type_hint: "number".to_string(),
                description: "Client ID to update (e.g., 1, 2).".to_string(),
                required: true,
            },
            Parameter {
                name: "instruction".to_string(),
                type_hint: "string".to_string(),
                description: "New instruction for the client.".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "update_client_instruction",
            "client_id": 1,
            "instruction": "Switch to POST requests with JSON payload"
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
pub fn schedule_task_action(
    _selected_mode: crate::state::app_state::ScriptingMode,
    env: &crate::scripting::ScriptingEnvironment,
) -> ActionDefinition {
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
            name: "connection_id".to_string(),
            type_hint: "string".to_string(),
            description: "Optional: Connection ID (e.g., 'conn-123') to scope this task to a specific connection. Requires server_id to be specified. Task will be automatically cleaned up when the connection closes. Useful for connection-specific timeouts, session cleanup, or per-connection monitoring.".to_string(),
            required: false,
        },
        Parameter {
            name: "client_id".to_string(),
            type_hint: "number".to_string(),
            description: "Optional: Client ID to scope this task to. If provided, task uses client's instruction and protocol actions. Task will be automatically cleaned up when the client disconnects. Useful for client-specific timeouts, reconnection logic, or per-client monitoring.".to_string(),
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

    // Add script parameters based on scripting mode
    match selected_mode {
        crate::state::app_state::ScriptingMode::On => {
            // ON mode: LLM chooses runtime from available options
            let available_runtimes = env.format_available();
            parameters.extend(vec![
                Parameter {
                    name: "script_runtime".to_string(),
                    type_hint: "string".to_string(),
                    description: format!(
                        "Required when script_inline is provided: Choose runtime for script execution. Available: {}",
                        available_runtimes
                    ),
                    required: false,
                },
                Parameter {
                    name: "script_inline".to_string(),
                    type_hint: "string".to_string(),
                    description: "Optional: Inline script code to handle task execution instead of LLM. Must match the script_runtime language. If provided, script_runtime MUST also be specified.".to_string(),
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
        crate::state::app_state::ScriptingMode::Off => {
            // OFF mode: no script parameters
        }
        crate::state::app_state::ScriptingMode::Python
        | crate::state::app_state::ScriptingMode::JavaScript
        | crate::state::app_state::ScriptingMode::Go
        | crate::state::app_state::ScriptingMode::Perl => {
            let lang = selected_mode.as_str();
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
/// 2. Client Management - Create/destroy/control clients
/// 3. Server Configuration - Configure running servers
/// 4. Task Management - Schedule/cancel tasks
/// 5. System/Utility - Model changes, messages, logging
pub fn get_all_common_actions(
    _selected_mode: crate::state::app_state::ScriptingMode,
    env: &crate::scripting::ScriptingEnvironment,
    is_open_server_enabled: bool,
    is_open_client_enabled: bool,
) -> Vec<ActionDefinition> {
    let actions = vec![
        // === Server Management ===
        open_server_action(selected_mode, env, is_open_server_enabled),
        close_server_action(),
        close_all_servers_action(),
        // === Client Management ===
        open_client_action(selected_mode, env, is_open_client_enabled),
        close_client_action(),
        close_all_clients_action(),
        reconnect_client_action(),
        update_client_instruction_action(),
        // === Server Configuration ===
        update_instruction_action(),
        set_memory_action(),
        append_memory_action(),
        // === Task Management ===
        schedule_task_action(selected_mode, env),
        cancel_task_action(),
        list_tasks_action(),
        // === System/Utility ===
        change_model_action(),
        show_message_action(),
        append_to_log_action(),
    ];

    actions
}

/// Get common actions for user input (all common actions with enhanced open_server and open_client)
pub fn get_user_input_common_actions(
    _selected_mode: crate::state::app_state::ScriptingMode,
    env: &crate::scripting::ScriptingEnvironment,
    is_open_server_enabled: bool,
    is_open_client_enabled: bool,
) -> Vec<ActionDefinition> {
    get_all_common_actions(selected_mode, env, is_open_server_enabled, is_open_client_enabled)
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
    let registry = crate::protocol::server_registry::registry();

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
    let registry = crate::protocol::server_registry::registry();
    let mut groups: std::collections::HashMap<
        &'static str,
        Vec<(String, std::sync::Arc<dyn crate::llm::actions::Server>)>,
    > = std::collections::HashMap::new();

    for protocol_name in all_base_stacks(include_disabled) {
        if let Some(protocol) = registry.get(&protocol_name) {
            let group = protocol.group_name();
            groups
                .entry(group)
                .or_insert_with(Vec::new)
                .push((protocol_name.clone(), protocol));
        }
    }

    // Sort groups alphabetically
    let mut sorted_group_names: Vec<&'static str> = groups.keys().copied().collect();
    sorted_group_names.sort();

    for group_name in sorted_group_names {
        if let Some(protocols) = groups.get_mut(group_name) {
            if protocols.is_empty() {
                continue;
            }

            // Output group header
            doc.push_str(&format!("### {}\n", group_name));

            // Sort protocols alphabetically within group
            protocols.sort_by(|a, b| a.0.cmp(&b.0));

            // Output each protocol with keywords on one line
            for (protocol_name, protocol) in protocols {
                let keywords = protocol.keywords();
                if !keywords.is_empty() {
                    doc.push_str(&format!("{} ({})\n", protocol_name, keywords.join(", ")));
                } else {
                    doc.push_str(&format!("{}\n", protocol_name));
                }
            }
            doc.push('\n');
        }
    }

    doc
}

/// Generate documentation for a single protocol
///
/// This is used by the read_base_stack_docs tool to provide detailed information
/// about a specific protocol on demand. Includes both server and client capabilities
/// if available.
///
/// # Arguments
/// * `protocol_name` - Name of the protocol (e.g., "http", "ssh", "tor")
///
/// # Returns
/// * `Ok(String)` - Documentation for the protocol (server and/or client)
/// * `Err(_)` - If protocol not found in either registry
pub fn generate_single_protocol_documentation(protocol_name: &str) -> anyhow::Result<String> {
    let server_registry = crate::protocol::server_registry::registry();
    let client_registry = &crate::protocol::client_registry::CLIENT_REGISTRY;

    // Protocol names are stored in uppercase (e.g., "HTTP", "SSH", "TCP")
    // Normalize input to lowercase for client registry and uppercase for server
    let normalized_name_upper = protocol_name.to_uppercase();
    let normalized_name_lower = protocol_name.to_lowercase();

    let server_protocol = server_registry.get(&normalized_name_upper);
    let client_protocol = client_registry.get(&normalized_name_lower);

    // Error if neither found
    if server_protocol.is_none() && client_protocol.is_none() {
        return Err(anyhow::anyhow!(
            "Protocol '{}' not found in server or client registry",
            protocol_name
        ));
    }

    let mut doc = String::new();

    // Protocol header
    doc.push_str(&format!(
        "# {} Protocol Documentation\n\n",
        protocol_name.to_uppercase()
    ));

    // Show which modes are available
    if server_protocol.is_some() && client_protocol.is_some() {
        doc.push_str("**Available as:** Server and Client\n\n");
    } else if server_protocol.is_some() {
        doc.push_str("**Available as:** Server only\n\n");
    } else {
        doc.push_str("**Available as:** Client only\n\n");
    }

    // Server documentation
    if let Some(protocol) = server_protocol {
        doc.push_str("## Server Mode\n\n");

        // Full stack name
        doc.push_str(&format!("**Full name:** {}\n\n", protocol.stack_name()));

        // Group
        doc.push_str(&format!("**Category:** {}\n\n", protocol.group_name()));

        // Description
        doc.push_str(&format!("**Description:** {}\n\n", protocol.description()));

        // Example prompt
        doc.push_str(&format!(
            "**Example usage:** \"{}\"\n\n",
            protocol.example_prompt()
        ));

        // Keywords
        let keywords = protocol.keywords();
        if !keywords.is_empty() {
            doc.push_str(&format!("**Keywords:** {}\n\n", keywords.join(", ")));
        }

        // Startup parameters
        let params = protocol.get_startup_parameters();
        if !params.is_empty() {
            doc.push_str("### Startup Parameters for `open_server`\n\n");
            doc.push_str("These parameters can be included in the `startup_params` field when calling `open_server`:\n\n");
            for param in params {
                doc.push_str(&format!(
                    "- **{}** ({}): {}\n",
                    param.name,
                    if param.required {
                        "required"
                    } else {
                        "optional"
                    },
                    param.description
                ));
                doc.push_str(&format!("  - Type: {}\n", param.type_hint));
                doc.push_str(&format!(
                    "  - Example: {}\n",
                    serde_json::to_string(&param.example).unwrap_or_default()
                ));
                doc.push('\n');
            }
        } else {
            doc.push_str("### Startup Parameters for `open_server`\n\nThis server does not require any startup parameters.\n\n");
        }

        // Metadata (state)
        let metadata = protocol.metadata();
        doc.push_str(&format!("**Development state:** {:?}\n", metadata.state));
        if let Some(notes) = metadata.notes {
            doc.push_str(&format!("**Notes:** {}\n", notes));
        }
        doc.push('\n');
    }

    // Client documentation
    if let Some(client) = client_protocol {
        doc.push_str("## Client Mode\n\n");

        // Full stack name
        doc.push_str(&format!("**Full name:** {}\n\n", client.stack_name()));

        // Group
        doc.push_str(&format!("**Category:** {}\n\n", client.group_name()));

        // Description
        doc.push_str(&format!("**Description:** {}\n\n", client.description()));

        // Example prompt
        doc.push_str(&format!(
            "**Example usage:** \"{}\"\n\n",
            client.example_prompt()
        ));

        // Keywords
        let keywords = client.keywords();
        if !keywords.is_empty() {
            doc.push_str(&format!("**Keywords:** {}\n\n", keywords.join(", ")));
        }

        // Startup parameters
        let params = client.get_startup_parameters();
        if !params.is_empty() {
            doc.push_str("### Startup Parameters for `open_client`\n\n");
            doc.push_str("These parameters can be included in the `startup_params` field when calling `open_client`:\n\n");
            for param in params {
                doc.push_str(&format!(
                    "- **{}** ({}): {}\n",
                    param.name,
                    if param.required {
                        "required"
                    } else {
                        "optional"
                    },
                    param.description
                ));
                doc.push_str(&format!("  - Type: {}\n", param.type_hint));
                doc.push_str(&format!(
                    "  - Example: {}\n",
                    serde_json::to_string(&param.example).unwrap_or_default()
                ));
                doc.push('\n');
            }
        } else {
            doc.push_str("### Startup Parameters for `open_client`\n\nThis client does not require any startup parameters.\n\n");
        }

        // Metadata (state)
        let metadata = client.metadata();
        doc.push_str(&format!("**Development state:** {:?}\n", metadata.state));
        if let Some(notes) = metadata.notes {
            doc.push_str(&format!("**Notes:** {}\n", notes));
        }
    }

    Ok(doc)
}
