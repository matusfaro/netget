//! Common actions available in all prompts
//!
//! This module defines actions that are available in both user input
//! and network event prompts (show_message, memory operations, etc.).

use super::protocol_trait::ProtocolActions;
use super::{ActionDefinition, Parameter};
use crate::protocol::BaseStack;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

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
pub fn open_server_action() -> ActionDefinition {
    ActionDefinition {
        name: "open_server".to_string(),
        description: "Start a new server".to_string(),
        parameters: vec![
            Parameter {
                name: "port".to_string(),
                type_hint: "number".to_string(),
                description: "Port number to listen on".to_string(),
                required: true,
            },
            Parameter {
                name: "base_stack".to_string(),
                type_hint: "string".to_string(),
                description: "Stack: tcp, http, udp, snmp, dns, dhcp, ntp, ssh, irc".to_string(),
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
                name: "script_language".to_string(),
                type_hint: "string".to_string(),
                description: "Optional: Use 'python' or 'javascript' to handle deterministic responses via script instead of LLM.".to_string(),
                required: false,
            },
            Parameter {
                name: "script_path".to_string(),
                type_hint: "string".to_string(),
                description: "Optional: Path to script file (alternative to script_inline).".to_string(),
                required: false,
            },
            Parameter {
                name: "script_inline".to_string(),
                type_hint: "string".to_string(),
                description: "Optional: Inline script code (alternative to script_path).".to_string(),
                required: false,
            },
            Parameter {
                name: "script_handles".to_string(),
                type_hint: "array".to_string(),
                description: "Optional: Context types the script handles, e.g. [\"ssh_auth\", \"ssh_banner\"] or [\"all\"]. Defaults to [\"all\"].".to_string(),
                required: false,
            },
        ],
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
pub fn update_script_action() -> ActionDefinition {
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
                name: "script_language".to_string(),
                type_hint: "string".to_string(),
                description: "Script language: 'python' or 'javascript' (required for 'set' operation)".to_string(),
                required: false,
            },
            Parameter {
                name: "script_path".to_string(),
                type_hint: "string".to_string(),
                description: "Path to script file (alternative to script_inline, required for 'set')".to_string(),
                required: false,
            },
            Parameter {
                name: "script_inline".to_string(),
                type_hint: "string".to_string(),
                description: "Inline script code (alternative to script_path, required for 'set')".to_string(),
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
            "script_language": "python",
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

/// Get all common action definitions
///
/// Actions are organized logically:
/// 1. Server Management - Create/destroy servers
/// 2. Server Configuration - Configure running servers
/// 3. System/Utility - Model changes, messages, logging
pub fn get_all_common_actions() -> Vec<ActionDefinition> {
    vec![
        // === Server Management ===
        get_open_server_action_with_params(),
        close_server_action(),
        // === Server Configuration ===
        update_instruction_action(),
        update_script_action(),
        set_memory_action(),
        append_memory_action(),
        // === System/Utility ===
        change_model_action(),
        show_message_action(),
        append_to_log_action(),
    ]
}

/// Get common actions for user input (all common actions with enhanced open_server)
pub fn get_user_input_common_actions() -> Vec<ActionDefinition> {
    get_all_common_actions()
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

/// Create a protocol instance for getting startup parameters
/// Returns None if the protocol doesn't support the ProtocolActions trait or isn't compiled in
fn get_protocol_for_stack(stack: BaseStack) -> Option<Box<dyn ProtocolActions>> {
    match stack {
        #[cfg(feature = "tcp")]
        BaseStack::Tcp => {
            use crate::server::TcpProtocol;
            Some(Box::new(TcpProtocol::new()))
        }
        #[cfg(feature = "http")]
        BaseStack::Http => {
            use crate::server::HttpProtocol;
            Some(Box::new(HttpProtocol::new()))
        }
        #[cfg(feature = "udp")]
        BaseStack::Udp => {
            use crate::server::UdpProtocol;
            Some(Box::new(UdpProtocol::new()))
        }
        #[cfg(feature = "dns")]
        BaseStack::Dns => {
            use crate::server::DnsProtocol;
            Some(Box::new(DnsProtocol::new()))
        }
        #[cfg(feature = "dhcp")]
        BaseStack::Dhcp => {
            use crate::server::DhcpProtocol;
            Some(Box::new(DhcpProtocol::new()))
        }
        #[cfg(feature = "ntp")]
        BaseStack::Ntp => {
            use crate::server::NtpProtocol;
            Some(Box::new(NtpProtocol::new()))
        }
        #[cfg(feature = "snmp")]
        BaseStack::Snmp => {
            use crate::server::SnmpProtocol;
            Some(Box::new(SnmpProtocol::new()))
        }
        #[cfg(feature = "ssh")]
        BaseStack::Ssh => {
            use crate::server::SshProtocol;
            Some(Box::new(SshProtocol::new()))
        }
        #[cfg(feature = "irc")]
        BaseStack::Irc => {
            use crate::server::IrcProtocol;
            Some(Box::new(IrcProtocol::new()))
        }
        #[cfg(feature = "telnet")]
        BaseStack::Telnet => {
            use crate::server::TelnetProtocol;
            Some(Box::new(TelnetProtocol::new()))
        }
        #[cfg(feature = "smtp")]
        BaseStack::Smtp => {
            use crate::server::SmtpProtocol;
            Some(Box::new(SmtpProtocol::new()))
        }
        #[cfg(feature = "mdns")]
        BaseStack::Mdns => {
            use crate::server::MdnsProtocol;
            Some(Box::new(MdnsProtocol::new()))
        }
        #[cfg(feature = "ipp")]
        BaseStack::Ipp => {
            use crate::server::IppProtocol;
            Some(Box::new(IppProtocol::new()))
        }
        #[cfg(feature = "mysql")]
        BaseStack::Mysql => {
            // MySQL protocol requires constructor args, so we can't instantiate here
            // Startup parameters not supported yet for MySQL
            None
        }
        #[cfg(feature = "postgresql")]
        BaseStack::Postgresql => {
            // PostgreSQL protocol requires constructor args, so we can't instantiate here
            // Startup parameters not supported yet for PostgreSQL
            None
        }
        #[cfg(feature = "redis")]
        BaseStack::Redis => {
            // Redis protocol requires constructor args, so we can't instantiate here
            // Startup parameters not supported yet for Redis
            None
        }
        #[cfg(feature = "proxy")]
        BaseStack::Proxy => {
            use crate::server::ProxyProtocol;
            Some(Box::new(ProxyProtocol::new()))
        }
        #[cfg(feature = "webdav")]
        BaseStack::WebDav => {
            use crate::server::WebDavProtocol;
            Some(Box::new(WebDavProtocol::new()))
        }
        #[cfg(feature = "nfs")]
        BaseStack::Nfs => {
            use crate::server::NfsProtocol;
            Some(Box::new(NfsProtocol::new()))
        }
        _ => None,
    }
}

/// Get all BaseStack enum values (useful for iteration)
fn all_base_stacks() -> Vec<BaseStack> {
    vec![
        BaseStack::Tcp,
        BaseStack::Http,
        BaseStack::DataLink,
        BaseStack::Udp,
        BaseStack::Dns,
        BaseStack::Dhcp,
        BaseStack::Ntp,
        BaseStack::Snmp,
        BaseStack::Ssh,
        BaseStack::Irc,
        BaseStack::Telnet,
        BaseStack::Smtp,
        BaseStack::Mdns,
        BaseStack::Mysql,
        BaseStack::Ipp,
        BaseStack::Postgresql,
        BaseStack::Redis,
        BaseStack::Proxy,
        BaseStack::WebDav,
        BaseStack::Nfs,
    ]
}

/// Generate comprehensive base stack documentation with startup parameters
/// Returns formatted text listing all available stacks and their configuration options
pub fn generate_base_stack_documentation() -> String {
    let mut doc = String::from("## Available Base Stacks\n\n");
    doc.push_str("Each protocol stack has a specific name to use in the 'base_stack' field:\n\n");

    for stack in all_base_stacks() {
        // Get the stack name/identifier
        let stack_str = stack.to_string();
        doc.push_str(&format!("### {}\n", stack_str));
        doc.push_str(&format!("Stack name: \"{}\"\n", stack.name()));

        // Add startup parameters if available
        if let Some(protocol) = get_protocol_for_stack(stack) {
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
                doc.push_str("No startup parameters.\n");
            }
        } else {
            doc.push_str("No startup parameters.\n");
        }
        doc.push('\n');
    }

    doc
}

/// Get open_server action with example showing startup_params usage
///
/// Startup parameter documentation is provided in the base stack documentation section,
/// not inline here, to avoid redundancy and reduce token usage.
pub fn get_open_server_action_with_params() -> ActionDefinition {
    let mut base_action = open_server_action();

    // Use example that shows startup_params usage (proxy is a good example as it has params)
    base_action.example = json!({
        "type": "open_server",
        "port": 8080,
        "base_stack": "proxy",
        "instruction": "Act as HTTP proxy",
        "startup_params": {
            "certificate_mode": "generate",
            "request_filter_mode": "match_only"
        }
    });

    base_action
}
