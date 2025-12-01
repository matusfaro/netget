//! SMB protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// SMB protocol action handler
pub struct SmbProtocol;

impl Default for SmbProtocol {
    fn default() -> Self {
        Self
    }
}

impl SmbProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for SmbProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![disconnect_client_action()]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            smb_auth_success_action(),
            smb_auth_deny_action(),
            smb_list_directory_action(),
            smb_read_file_action(),
            smb_write_file_action(),
            smb_get_file_info_action(),
            smb_create_file_action(),
            smb_delete_file_action(),
            smb_create_directory_action(),
            smb_delete_directory_action(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "SMB"
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>SMB"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["smb", "cifs"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Manual SMB2 protocol (0x0210 dialect)")
            .llm_control("Filesystem operations, authentication, directory listings")
            .e2e_testing("smbclient / Windows Explorer")
            .notes("SMB 2.1 only, guest auth only, no signing/encryption")
            .build()
    }
    fn description(&self) -> &'static str {
        "SMB/CIFS file server"
    }
    fn example_prompt(&self) -> &'static str {
        "Start an SMB/CIFS file server on port 8445"
    }
    fn group_name(&self) -> &'static str {
        "Web & File"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        StartupExamples::new(
            // LLM mode
            json!({
                "type": "open_server",
                "port": 445,
                "base_stack": "smb",
                "instruction": "SMB file server. Accept all guest connections. Provide /documents directory with sample files. Return file content on reads."
            }),
            // Script mode
            json!({
                "type": "open_server",
                "port": 445,
                "base_stack": "smb",
                "event_handlers": [{
                    "event_pattern": "smb_operation",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<protocol_handler>"
                    }
                }]
            }),
            // Static mode
            json!({
                "type": "open_server",
                "port": 445,
                "base_stack": "smb",
                "event_handlers": [{
                    "event_pattern": "smb_operation",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "smb_auth_success",
                            "username": "guest"
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for SmbProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::smb::SmbServer;
            SmbServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            )
            .await
        })
    }
    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        // Return Custom result with the action data for SMB server to handle
        Ok(ActionResult::Custom {
            name: action_type.to_string(),
            data: action,
        })
    }
}

// Event type for SMB operations
pub static SMB_OPERATION_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "smb_operation",
        "SMB client requested a filesystem operation",
        json!({
            "type": "smb_read_file",
            "path": "/documents/file.txt",
            "content": "Sample file content"
        })
    )
    .with_parameters(vec![
        Parameter {
            name: "operation".to_string(),
            type_hint: "string".to_string(),
            description: "The SMB operation type".to_string(),
            required: true,
        },
        Parameter {
            name: "path".to_string(),
            type_hint: "string".to_string(),
            description: "The file or directory path being accessed".to_string(),
            required: false,
        },
    ])
});

// Action definitions

fn disconnect_client_action() -> ActionDefinition {
    ActionDefinition {
        name: "disconnect_client".to_string(),
        description: "Disconnect an SMB client".to_string(),
        parameters: vec![Parameter {
            name: "client".to_string(),
            type_hint: "string".to_string(),
            description: "Client address to disconnect".to_string(),
            required: true,
        }],
        example: json!({
            "type": "disconnect_client",
            "client": "192.168.1.100:54321"
        }),
    }
}

fn smb_list_directory_action() -> ActionDefinition {
    ActionDefinition {
        name: "smb_list_directory".to_string(),
        description: "List files in a directory".to_string(),
        parameters: vec![
            Parameter {
                name: "path".to_string(),
                type_hint: "string".to_string(),
                description: "Directory path to list".to_string(),
                required: true,
            },
            Parameter {
                name: "files".to_string(),
                type_hint: "array".to_string(),
                description: "Array of file objects with name, size, is_directory, modified_time"
                    .to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "smb_list_directory",
            "path": "/documents",
            "files": [
                {
                    "name": "report.pdf",
                    "size": 524288,
                    "is_directory": false,
                    "modified_time": "2025-01-15T10:30:00Z"
                }
            ]
        }),
    }
}

fn smb_read_file_action() -> ActionDefinition {
    ActionDefinition {
        name: "smb_read_file".to_string(),
        description: "Read file contents".to_string(),
        parameters: vec![
            Parameter {
                name: "path".to_string(),
                type_hint: "string".to_string(),
                description: "File path to read".to_string(),
                required: true,
            },
            Parameter {
                name: "content".to_string(),
                type_hint: "string".to_string(),
                description: "File content (base64 encoded for binary)".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "smb_read_file",
            "path": "/documents/file.txt",
            "content": "Hello, World!"
        }),
    }
}

fn smb_write_file_action() -> ActionDefinition {
    ActionDefinition {
        name: "smb_write_file".to_string(),
        description: "Write to a file".to_string(),
        parameters: vec![
            Parameter {
                name: "path".to_string(),
                type_hint: "string".to_string(),
                description: "File path to write".to_string(),
                required: true,
            },
            Parameter {
                name: "data".to_string(),
                type_hint: "string".to_string(),
                description: "Data to write (base64 for binary)".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "smb_write_file",
            "path": "/documents/file.txt",
            "data": "New content"
        }),
    }
}

fn smb_get_file_info_action() -> ActionDefinition {
    ActionDefinition {
        name: "smb_get_file_info".to_string(),
        description: "Get file metadata".to_string(),
        parameters: vec![
            Parameter {
                name: "path".to_string(),
                type_hint: "string".to_string(),
                description: "File path".to_string(),
                required: true,
            },
            Parameter {
                name: "size".to_string(),
                type_hint: "number".to_string(),
                description: "File size in bytes".to_string(),
                required: true,
            },
            Parameter {
                name: "is_directory".to_string(),
                type_hint: "boolean".to_string(),
                description: "Whether path is a directory".to_string(),
                required: true,
            },
            Parameter {
                name: "modified_time".to_string(),
                type_hint: "string".to_string(),
                description: "Last modified time (ISO 8601)".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "smb_get_file_info",
            "path": "/documents/file.txt",
            "size": 1024,
            "is_directory": false,
            "modified_time": "2025-01-15T10:30:00Z"
        }),
    }
}

fn smb_create_file_action() -> ActionDefinition {
    ActionDefinition {
        name: "smb_create_file".to_string(),
        description: "Create a new file".to_string(),
        parameters: vec![Parameter {
            name: "path".to_string(),
            type_hint: "string".to_string(),
            description: "File path to create".to_string(),
            required: true,
        }],
        example: json!({
            "type": "smb_create_file",
            "path": "/documents/newfile.txt"
        }),
    }
}

fn smb_delete_file_action() -> ActionDefinition {
    ActionDefinition {
        name: "smb_delete_file".to_string(),
        description: "Delete a file".to_string(),
        parameters: vec![Parameter {
            name: "path".to_string(),
            type_hint: "string".to_string(),
            description: "File path to delete".to_string(),
            required: true,
        }],
        example: json!({
            "type": "smb_delete_file",
            "path": "/documents/oldfile.txt"
        }),
    }
}

fn smb_create_directory_action() -> ActionDefinition {
    ActionDefinition {
        name: "smb_create_directory".to_string(),
        description: "Create a new directory".to_string(),
        parameters: vec![Parameter {
            name: "path".to_string(),
            type_hint: "string".to_string(),
            description: "Directory path to create".to_string(),
            required: true,
        }],
        example: json!({
            "type": "smb_create_directory",
            "path": "/documents/newdir"
        }),
    }
}

fn smb_delete_directory_action() -> ActionDefinition {
    ActionDefinition {
        name: "smb_delete_directory".to_string(),
        description: "Delete a directory".to_string(),
        parameters: vec![Parameter {
            name: "path".to_string(),
            type_hint: "string".to_string(),
            description: "Directory path to delete".to_string(),
            required: true,
        }],
        example: json!({
            "type": "smb_delete_directory",
            "path": "/documents/olddir"
        }),
    }
}

fn smb_auth_success_action() -> ActionDefinition {
    ActionDefinition {
        name: "smb_auth_success".to_string(),
        description: "Allow SMB authentication for the user (respond to session_setup event)"
            .to_string(),
        parameters: vec![
            Parameter {
                name: "username".to_string(),
                type_hint: "string".to_string(),
                description: "Username that was authenticated".to_string(),
                required: true,
            },
            Parameter {
                name: "message".to_string(),
                type_hint: "string".to_string(),
                description: "Optional message explaining why auth was allowed".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "smb_auth_success",
            "username": "alice",
            "message": "User alice authenticated successfully"
        }),
    }
}

fn smb_auth_deny_action() -> ActionDefinition {
    ActionDefinition {
        name: "smb_auth_deny".to_string(),
        description: "Deny SMB authentication for the user (respond to session_setup event)"
            .to_string(),
        parameters: vec![
            Parameter {
                name: "username".to_string(),
                type_hint: "string".to_string(),
                description: "Username that was denied".to_string(),
                required: true,
            },
            Parameter {
                name: "reason".to_string(),
                type_hint: "string".to_string(),
                description: "Reason for denying authentication".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "smb_auth_deny",
            "username": "hacker",
            "reason": "User not authorized"
        }),
    }
}
