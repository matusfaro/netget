//! NFS protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::log_template::LogTemplate;
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// NFS protocol action handler
pub struct NfsProtocol;

impl NfsProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Protocol for NfsProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![mount_filesystem_action(), unmount_filesystem_action()]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            // Response actions - LLM returns these with structured data
            nfs_lookup_response_action(),
            nfs_read_response_action(),
            nfs_write_response_action(),
            nfs_getattr_response_action(),
            nfs_create_response_action(),
            nfs_remove_response_action(),
            nfs_mkdir_response_action(),
            nfs_readdir_response_action(),
            nfs_rename_response_action(),
            nfs_setattr_response_action(),
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "NFS"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_nfs_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>NFS"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["nfs", "file server"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("nfsserve v0.6 NFSv3 server library")
            .llm_control("All filesystem operations (lookup, read, write, mkdir)")
            .e2e_testing("mount / nfs-client")
            .notes("NFSv3 only, virtual filesystem, no persistence")
            .build()
    }

    fn description(&self) -> &'static str {
        "NFS file server"
    }

    fn example_prompt(&self) -> &'static str {
        "Start an NFS file server on port 2049"
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
                "port": 2049,
                "base_stack": "nfs",
                "instruction": "NFS file server. On file reads, return content based on the path. On writes, acknowledge with updated attributes. Provide directory listings with sample files."
            }),
            // Script mode
            json!({
                "type": "open_server",
                "port": 2049,
                "base_stack": "nfs",
                "event_handlers": [{
                    "event_pattern": "nfs_operation",
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
                "port": 2049,
                "base_stack": "nfs",
                "event_handlers": [{
                    "event_pattern": "nfs_operation",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "nfs_getattr_response",
                            "file_type": "regular",
                            "mode": 420,
                            "size": 1024,
                            "uid": 1000,
                            "gid": 1000
                        }]
                    }
                }]
            }),
        )
    }
}

impl Server for NfsProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::nfs::NfsServer;
            NfsServer::spawn_with_llm_actions(
                ctx.legacy_listen_addr(),
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

        match action_type {
            "mount_filesystem" => self.execute_mount_filesystem(action),
            "unmount_filesystem" => self.execute_unmount_filesystem(action),
            "nfs_lookup_response" => self.execute_nfs_lookup_response(action),
            "nfs_read_response" => self.execute_nfs_read_response(action),
            "nfs_write_response" => self.execute_nfs_write_response(action),
            "nfs_getattr_response" => self.execute_nfs_getattr_response(action),
            "nfs_create_response" => self.execute_nfs_create_response(action),
            "nfs_remove_response" => self.execute_nfs_remove_response(action),
            "nfs_mkdir_response" => self.execute_nfs_mkdir_response(action),
            "nfs_readdir_response" => self.execute_nfs_readdir_response(action),
            "nfs_rename_response" => self.execute_nfs_rename_response(action),
            "nfs_setattr_response" => self.execute_nfs_setattr_response(action),
            _ => Err(anyhow::anyhow!("Unknown NFS action: {}", action_type)),
        }
    }
}

impl NfsProtocol {
    /// Mount a filesystem export
    fn execute_mount_filesystem(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _path = action
            .get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' parameter")?;

        Ok(ActionResult::NoAction)
    }

    /// Unmount a filesystem
    fn execute_unmount_filesystem(&self, action: serde_json::Value) -> Result<ActionResult> {
        let _path = action
            .get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' parameter")?;

        Ok(ActionResult::NoAction)
    }

    /// NFS LOOKUP response
    fn execute_nfs_lookup_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let response = json!({
            "fileid": action.get("fileid").and_then(|v| v.as_u64()),
            "error": action.get("error").and_then(|v| v.as_str()),
        });
        Ok(ActionResult::Output(serde_json::to_vec(&response)?))
    }

    /// NFS READ response
    fn execute_nfs_read_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let response = json!({
            "data": action.get("data").and_then(|v| v.as_str()).unwrap_or(""),
            "eof": action.get("eof").and_then(|v| v.as_bool()).unwrap_or(true),
            "error": action.get("error").and_then(|v| v.as_str()),
        });
        Ok(ActionResult::Output(serde_json::to_vec(&response)?))
    }

    /// NFS WRITE response
    fn execute_nfs_write_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let response = json!({
            "size": action.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
            "mode": action.get("mode").and_then(|v| v.as_u64()).unwrap_or(0o644),
            "mtime": action.get("mtime").and_then(|v| v.as_u64()),
            "error": action.get("error").and_then(|v| v.as_str()),
        });
        Ok(ActionResult::Output(serde_json::to_vec(&response)?))
    }

    /// NFS GETATTR response
    fn execute_nfs_getattr_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let response = json!({
            "file_type": action.get("file_type").and_then(|v| v.as_str()).unwrap_or("regular"),
            "mode": action.get("mode").and_then(|v| v.as_u64()).unwrap_or(0o644),
            "size": action.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
            "uid": action.get("uid").and_then(|v| v.as_u64()).unwrap_or(0),
            "gid": action.get("gid").and_then(|v| v.as_u64()).unwrap_or(0),
            "atime": action.get("atime").and_then(|v| v.as_u64()),
            "mtime": action.get("mtime").and_then(|v| v.as_u64()),
            "ctime": action.get("ctime").and_then(|v| v.as_u64()),
            "error": action.get("error").and_then(|v| v.as_str()),
        });
        Ok(ActionResult::Output(serde_json::to_vec(&response)?))
    }

    /// NFS CREATE response
    fn execute_nfs_create_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let response = json!({
            "fileid": action.get("fileid").and_then(|v| v.as_u64()),
            "size": action.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
            "mode": action.get("mode").and_then(|v| v.as_u64()).unwrap_or(0o644),
            "error": action.get("error").and_then(|v| v.as_str()),
        });
        Ok(ActionResult::Output(serde_json::to_vec(&response)?))
    }

    /// NFS REMOVE response
    fn execute_nfs_remove_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let response = json!({
            "success": action.get("success").and_then(|v| v.as_bool()).unwrap_or(false),
            "error": action.get("error").and_then(|v| v.as_str()),
        });
        Ok(ActionResult::Output(serde_json::to_vec(&response)?))
    }

    /// NFS MKDIR response
    fn execute_nfs_mkdir_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let response = json!({
            "fileid": action.get("fileid").and_then(|v| v.as_u64()),
            "mode": action.get("mode").and_then(|v| v.as_u64()).unwrap_or(0o755),
            "error": action.get("error").and_then(|v| v.as_str()),
        });
        Ok(ActionResult::Output(serde_json::to_vec(&response)?))
    }

    /// NFS READDIR response
    fn execute_nfs_readdir_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let response = json!({
            "entries": action.get("entries").and_then(|v| v.as_array()).cloned().unwrap_or_default(),
            "eof": action.get("eof").and_then(|v| v.as_bool()).unwrap_or(true),
            "error": action.get("error").and_then(|v| v.as_str()),
        });
        Ok(ActionResult::Output(serde_json::to_vec(&response)?))
    }

    /// NFS RENAME response
    fn execute_nfs_rename_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let response = json!({
            "success": action.get("success").and_then(|v| v.as_bool()).unwrap_or(false),
            "error": action.get("error").and_then(|v| v.as_str()),
        });
        Ok(ActionResult::Output(serde_json::to_vec(&response)?))
    }

    /// NFS SETATTR response
    fn execute_nfs_setattr_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let response = json!({
            "size": action.get("size").and_then(|v| v.as_u64()),
            "mode": action.get("mode").and_then(|v| v.as_u64()),
            "mtime": action.get("mtime").and_then(|v| v.as_u64()),
            "error": action.get("error").and_then(|v| v.as_str()),
        });
        Ok(ActionResult::Output(serde_json::to_vec(&response)?))
    }
}

/// Action definitions
fn mount_filesystem_action() -> ActionDefinition {
    ActionDefinition {
        name: "mount_filesystem".to_string(),
        description: "Mount an NFS filesystem export".to_string(),
        parameters: vec![Parameter {
            name: "path".to_string(),
            type_hint: "string".to_string(),
            description: "Export path to mount".to_string(),
            required: true,
        }],
        example: json!({
            "type": "mount_filesystem",
            "path": "/export/data"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("NFS mount {path}")
                .with_debug("NFS mount_filesystem: path={path}"),
        ),
    }
}

fn unmount_filesystem_action() -> ActionDefinition {
    ActionDefinition {
        name: "unmount_filesystem".to_string(),
        description: "Unmount an NFS filesystem".to_string(),
        parameters: vec![Parameter {
            name: "path".to_string(),
            type_hint: "string".to_string(),
            description: "Export path to unmount".to_string(),
            required: true,
        }],
        example: json!({
            "type": "unmount_filesystem",
            "path": "/export/data"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("NFS unmount {path}")
                .with_debug("NFS unmount_filesystem: path={path}"),
        ),
    }
}

fn nfs_lookup_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "nfs_lookup_response".to_string(),
        description: "Return file ID for NFS LOOKUP operation".to_string(),
        parameters: vec![
            Parameter {
                name: "fileid".to_string(),
                type_hint: "number".to_string(),
                description: "File ID (unique identifier) if file exists".to_string(),
                required: false,
            },
            Parameter {
                name: "error".to_string(),
                type_hint: "string".to_string(),
                description:
                    "NFS error code if operation failed (e.g. 'NFS3ERR_NOENT', 'NFS3ERR_ACCES')"
                        .to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "nfs_lookup_response",
            "fileid": 42
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> NFS lookup fileid={fileid}")
                .with_debug("NFS nfs_lookup_response: fileid={fileid}"),
        ),
    }
}

fn nfs_read_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "nfs_read_response".to_string(),
        description: "Return file data for NFS READ operation".to_string(),
        parameters: vec![
            Parameter {
                name: "data".to_string(),
                type_hint: "string".to_string(),
                description: "File content to return".to_string(),
                required: true,
            },
            Parameter {
                name: "eof".to_string(),
                type_hint: "boolean".to_string(),
                description: "True if end of file reached".to_string(),
                required: true,
            },
            Parameter {
                name: "error".to_string(),
                type_hint: "string".to_string(),
                description: "NFS error code if operation failed".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "nfs_read_response",
            "data": "Hello from NFS!",
            "eof": false
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> NFS read data")
                .with_debug("NFS nfs_read_response: eof={eof}"),
        ),
    }
}

fn nfs_write_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "nfs_write_response".to_string(),
        description: "Return file attributes after NFS WRITE operation".to_string(),
        parameters: vec![
            Parameter {
                name: "size".to_string(),
                type_hint: "number".to_string(),
                description: "New file size after write".to_string(),
                required: true,
            },
            Parameter {
                name: "mode".to_string(),
                type_hint: "number".to_string(),
                description: "File permissions (e.g. 0644)".to_string(),
                required: false,
            },
            Parameter {
                name: "mtime".to_string(),
                type_hint: "number".to_string(),
                description: "Modification time (Unix timestamp)".to_string(),
                required: false,
            },
            Parameter {
                name: "error".to_string(),
                type_hint: "string".to_string(),
                description: "NFS error code if operation failed".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "nfs_write_response",
            "size": 1024,
            "mode": 0o644
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> NFS write size={size}")
                .with_debug("NFS nfs_write_response: size={size}, mode={mode}"),
        ),
    }
}

fn nfs_getattr_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "nfs_getattr_response".to_string(),
        description: "Return file/directory attributes for NFS GETATTR operation".to_string(),
        parameters: vec![
            Parameter {
                name: "file_type".to_string(),
                type_hint: "string".to_string(),
                description: "'regular' for file, 'directory' for dir".to_string(),
                required: true,
            },
            Parameter {
                name: "mode".to_string(),
                type_hint: "number".to_string(),
                description: "Permissions (e.g. 0644 for files, 0755 for dirs)".to_string(),
                required: true,
            },
            Parameter {
                name: "size".to_string(),
                type_hint: "number".to_string(),
                description: "File size in bytes".to_string(),
                required: true,
            },
            Parameter {
                name: "uid".to_string(),
                type_hint: "number".to_string(),
                description: "Owner user ID".to_string(),
                required: false,
            },
            Parameter {
                name: "gid".to_string(),
                type_hint: "number".to_string(),
                description: "Owner group ID".to_string(),
                required: false,
            },
            Parameter {
                name: "atime".to_string(),
                type_hint: "number".to_string(),
                description: "Access time (Unix timestamp)".to_string(),
                required: false,
            },
            Parameter {
                name: "mtime".to_string(),
                type_hint: "number".to_string(),
                description: "Modification time (Unix timestamp)".to_string(),
                required: false,
            },
            Parameter {
                name: "ctime".to_string(),
                type_hint: "number".to_string(),
                description: "Status change time (Unix timestamp)".to_string(),
                required: false,
            },
            Parameter {
                name: "error".to_string(),
                type_hint: "string".to_string(),
                description: "NFS error code if operation failed".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "nfs_getattr_response",
            "file_type": "regular",
            "mode": 0o644,
            "size": 1024,
            "uid": 1000,
            "gid": 1000
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> NFS attr {file_type} size={size}")
                .with_debug("NFS nfs_getattr_response: type={file_type}, size={size}, mode={mode}"),
        ),
    }
}

fn nfs_create_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "nfs_create_response".to_string(),
        description: "Return new file ID and attributes for NFS CREATE operation".to_string(),
        parameters: vec![
            Parameter {
                name: "fileid".to_string(),
                type_hint: "number".to_string(),
                description: "New file ID".to_string(),
                required: true,
            },
            Parameter {
                name: "size".to_string(),
                type_hint: "number".to_string(),
                description: "Initial file size (usually 0)".to_string(),
                required: false,
            },
            Parameter {
                name: "mode".to_string(),
                type_hint: "number".to_string(),
                description: "File permissions".to_string(),
                required: false,
            },
            Parameter {
                name: "error".to_string(),
                type_hint: "string".to_string(),
                description: "NFS error code if operation failed".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "nfs_create_response",
            "fileid": 123,
            "size": 0,
            "mode": 0o644
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> NFS create fileid={fileid}")
                .with_debug("NFS nfs_create_response: fileid={fileid}, mode={mode}"),
        ),
    }
}

fn nfs_remove_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "nfs_remove_response".to_string(),
        description: "Return success/error for NFS REMOVE operation".to_string(),
        parameters: vec![
            Parameter {
                name: "success".to_string(),
                type_hint: "boolean".to_string(),
                description: "True if file was removed successfully".to_string(),
                required: true,
            },
            Parameter {
                name: "error".to_string(),
                type_hint: "string".to_string(),
                description: "NFS error code if operation failed".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "nfs_remove_response",
            "success": true
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> NFS remove success={success}")
                .with_debug("NFS nfs_remove_response: success={success}"),
        ),
    }
}

fn nfs_mkdir_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "nfs_mkdir_response".to_string(),
        description: "Return new directory ID for NFS MKDIR operation".to_string(),
        parameters: vec![
            Parameter {
                name: "fileid".to_string(),
                type_hint: "number".to_string(),
                description: "New directory ID".to_string(),
                required: true,
            },
            Parameter {
                name: "mode".to_string(),
                type_hint: "number".to_string(),
                description: "Directory permissions (default 0755)".to_string(),
                required: false,
            },
            Parameter {
                name: "error".to_string(),
                type_hint: "string".to_string(),
                description: "NFS error code if operation failed".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "nfs_mkdir_response",
            "fileid": 456,
            "mode": 0o755
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> NFS mkdir fileid={fileid}")
                .with_debug("NFS nfs_mkdir_response: fileid={fileid}, mode={mode}"),
        ),
    }
}

fn nfs_readdir_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "nfs_readdir_response".to_string(),
        description: "Return directory listing for NFS READDIR operation".to_string(),
        parameters: vec![
            Parameter {
                name: "entries".to_string(),
                type_hint: "array".to_string(),
                description:
                    "Array of directory entries [{\"name\": \"file.txt\", \"fileid\": 42}, ...]"
                        .to_string(),
                required: true,
            },
            Parameter {
                name: "eof".to_string(),
                type_hint: "boolean".to_string(),
                description: "True if no more entries".to_string(),
                required: true,
            },
            Parameter {
                name: "error".to_string(),
                type_hint: "string".to_string(),
                description: "NFS error code if operation failed".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "nfs_readdir_response",
            "entries": [
                {"name": "file.txt", "fileid": 42},
                {"name": "subdir", "fileid": 43}
            ],
            "eof": true
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> NFS readdir {entries_len} entries")
                .with_debug("NFS nfs_readdir_response: {entries_len} entries, eof={eof}"),
        ),
    }
}

fn nfs_rename_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "nfs_rename_response".to_string(),
        description: "Return success/error for NFS RENAME operation".to_string(),
        parameters: vec![
            Parameter {
                name: "success".to_string(),
                type_hint: "boolean".to_string(),
                description: "True if file was renamed successfully".to_string(),
                required: true,
            },
            Parameter {
                name: "error".to_string(),
                type_hint: "string".to_string(),
                description: "NFS error code if operation failed".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "nfs_rename_response",
            "success": true
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> NFS rename success={success}")
                .with_debug("NFS nfs_rename_response: success={success}"),
        ),
    }
}

fn nfs_setattr_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "nfs_setattr_response".to_string(),
        description: "Return updated attributes for NFS SETATTR operation".to_string(),
        parameters: vec![
            Parameter {
                name: "size".to_string(),
                type_hint: "number".to_string(),
                description: "New file size".to_string(),
                required: false,
            },
            Parameter {
                name: "mode".to_string(),
                type_hint: "number".to_string(),
                description: "New permissions".to_string(),
                required: false,
            },
            Parameter {
                name: "mtime".to_string(),
                type_hint: "number".to_string(),
                description: "New modification time".to_string(),
                required: false,
            },
            Parameter {
                name: "error".to_string(),
                type_hint: "string".to_string(),
                description: "NFS error code if operation failed".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "nfs_setattr_response",
            "mode": 0o600
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> NFS setattr")
                .with_debug("NFS nfs_setattr_response: mode={mode}"),
        ),
    }
}

// ============================================================================
// NFS Event Type Constants
// ============================================================================

/// NFS operation event - triggered when NFS client requests a filesystem operation
pub static NFS_OPERATION_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("nfs_operation", "NFS client requested a filesystem operation", json!({"type": "placeholder", "event_id": "nfs_operation"}))
    .with_parameters(vec![
        Parameter {
            name: "operation".to_string(),
            type_hint: "string".to_string(),
            description: "The NFS operation type (lookup, getattr, setattr, read, write, create, mkdir, remove, rename, readdir, symlink, readlink)".to_string(),
            required: true,
        },
        Parameter {
            name: "params".to_string(),
            type_hint: "object".to_string(),
            description: "Operation-specific parameters (path, fileid, offset, size, etc.)".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        // Include all NFS response actions
        // The LLM will choose the appropriate response based on the operation type
    ])
    .with_log_template(
        LogTemplate::new()
            .with_info("NFS {operation}")
            .with_debug("NFS {operation}: {params}")
            .with_trace("NFS: {json_pretty(.)}"),
    )
});

/// Get NFS event types
pub fn get_nfs_event_types() -> Vec<EventType> {
    vec![NFS_OPERATION_EVENT.clone()]
}
