//! NFS client protocol actions implementation

use crate::llm::actions::{
    client_trait::{Client, ClientActionResult},
    protocol_trait::Protocol,
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// NFS client connected event
pub static NFS_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "nfs_connected",
        "NFS client successfully mounted NFS export"
    )
    .with_parameters(vec![
        Parameter {
            name: "export_path".to_string(),
            type_hint: "string".to_string(),
            description: "NFS export path that was mounted".to_string(),
            required: true,
        },
        Parameter {
            name: "root_fh".to_string(),
            type_hint: "string".to_string(),
            description: "Root file handle (hex-encoded)".to_string(),
            required: true,
        },
    ])
});

/// NFS client file operation result event
pub static NFS_CLIENT_OPERATION_RESULT_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "nfs_operation_result",
        "Result of an NFS file operation"
    )
    .with_parameters(vec![
        Parameter {
            name: "operation".to_string(),
            type_hint: "string".to_string(),
            description: "The operation performed (lookup, read, write, etc.)".to_string(),
            required: true,
        },
        Parameter {
            name: "result".to_string(),
            type_hint: "object".to_string(),
            description: "Operation result data".to_string(),
            required: true,
        },
    ])
});

/// NFS client protocol action handler
pub struct NfsClientProtocol;

impl NfsClientProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Protocol for NfsClientProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "nfs_lookup".to_string(),
                description: "Look up a file or directory by path".to_string(),
                parameters: vec![
                    Parameter {
                        name: "path".to_string(),
                        type_hint: "string".to_string(),
                        description: "Path to file or directory (relative to root)".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "nfs_lookup",
                    "path": "/documents/readme.txt"
                }),
            },
            ActionDefinition {
                name: "nfs_read_file".to_string(),
                description: "Read contents of a file".to_string(),
                parameters: vec![
                    Parameter {
                        name: "path".to_string(),
                        type_hint: "string".to_string(),
                        description: "Path to file to read".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "offset".to_string(),
                        type_hint: "number".to_string(),
                        description: "Byte offset to start reading from (default: 0)".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "count".to_string(),
                        type_hint: "number".to_string(),
                        description: "Number of bytes to read (default: 4096)".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "nfs_read_file",
                    "path": "/readme.txt",
                    "offset": 0,
                    "count": 4096
                }),
            },
            ActionDefinition {
                name: "nfs_write_file".to_string(),
                description: "Write data to a file".to_string(),
                parameters: vec![
                    Parameter {
                        name: "path".to_string(),
                        type_hint: "string".to_string(),
                        description: "Path to file to write".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "data".to_string(),
                        type_hint: "string".to_string(),
                        description: "Data to write".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "offset".to_string(),
                        type_hint: "number".to_string(),
                        description: "Byte offset to start writing at (default: 0)".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "nfs_write_file",
                    "path": "/data.txt",
                    "data": "Hello, World!",
                    "offset": 0
                }),
            },
            ActionDefinition {
                name: "nfs_list_dir".to_string(),
                description: "List contents of a directory".to_string(),
                parameters: vec![
                    Parameter {
                        name: "path".to_string(),
                        type_hint: "string".to_string(),
                        description: "Path to directory to list (default: /)".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "nfs_list_dir",
                    "path": "/documents"
                }),
            },
            ActionDefinition {
                name: "nfs_get_attr".to_string(),
                description: "Get file or directory attributes".to_string(),
                parameters: vec![
                    Parameter {
                        name: "path".to_string(),
                        type_hint: "string".to_string(),
                        description: "Path to file or directory".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "nfs_get_attr",
                    "path": "/readme.txt"
                }),
            },
            ActionDefinition {
                name: "nfs_create_file".to_string(),
                description: "Create a new file".to_string(),
                parameters: vec![
                    Parameter {
                        name: "path".to_string(),
                        type_hint: "string".to_string(),
                        description: "Path to new file".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "mode".to_string(),
                        type_hint: "number".to_string(),
                        description: "File permissions in octal (default: 0644)".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "nfs_create_file",
                    "path": "/newfile.txt",
                    "mode": 0o644
                }),
            },
            ActionDefinition {
                name: "nfs_mkdir".to_string(),
                description: "Create a new directory".to_string(),
                parameters: vec![
                    Parameter {
                        name: "path".to_string(),
                        type_hint: "string".to_string(),
                        description: "Path to new directory".to_string(),
                        required: true,
                    },
                    Parameter {
                        name: "mode".to_string(),
                        type_hint: "number".to_string(),
                        description: "Directory permissions in octal (default: 0755)".to_string(),
                        required: false,
                    },
                ],
                example: json!({
                    "type": "nfs_mkdir",
                    "path": "/newdir",
                    "mode": 0o755
                }),
            },
            ActionDefinition {
                name: "nfs_remove".to_string(),
                description: "Remove a file".to_string(),
                parameters: vec![
                    Parameter {
                        name: "path".to_string(),
                        type_hint: "string".to_string(),
                        description: "Path to file to remove".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "nfs_remove",
                    "path": "/oldfile.txt"
                }),
            },
            ActionDefinition {
                name: "nfs_rmdir".to_string(),
                description: "Remove a directory".to_string(),
                parameters: vec![
                    Parameter {
                        name: "path".to_string(),
                        type_hint: "string".to_string(),
                        description: "Path to directory to remove".to_string(),
                        required: true,
                    },
                ],
                example: json!({
                    "type": "nfs_rmdir",
                    "path": "/olddir"
                }),
            },
            ActionDefinition {
                name: "disconnect".to_string(),
                description: "Disconnect from the NFS server".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "disconnect"
                }),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "wait_for_more".to_string(),
                description: "Wait for more operations before responding".to_string(),
                parameters: vec![],
                example: json!({
                    "type": "wait_for_more"
                }),
            },
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "NFS"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            EventType {
                id: "nfs_connected".to_string(),
                description: "Triggered when NFS client mounts export".to_string(),
                actions: vec![],
                parameters: vec![],
            },
            EventType {
                id: "nfs_operation_result".to_string(),
                description: "Triggered when NFS operation completes".to_string(),
                actions: vec![],
                parameters: vec![],
            },
        ]
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>RPC>NFS"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["nfs", "nfs client", "connect to nfs"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("nfs3_client crate for NFSv3 protocol")
            .llm_control("Full control over file operations (read, write, create, delete)")
            .e2e_testing("NetGet NFS server as test target")
            .build()
    }

    fn description(&self) -> &'static str {
        "NFS client for mounting and accessing network file systems"
    }

    fn example_prompt(&self) -> &'static str {
        "Connect to NFS server at 192.168.1.100:/export/data and read /readme.txt"
    }

    fn group_name(&self) -> &'static str {
        "File Sharing"
    }
}

impl Client for NfsClientProtocol {
    fn connect(
        &self,
        ctx: crate::protocol::ConnectContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::client::nfs::NfsClient;
            NfsClient::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.client_id,
            )
            .await
        })
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "nfs_lookup" | "nfs_read_file" | "nfs_write_file" | "nfs_list_dir" |
            "nfs_get_attr" | "nfs_create_file" | "nfs_mkdir" | "nfs_remove" | "nfs_rmdir" => {
                // These operations are handled asynchronously in the main loop
                Ok(ClientActionResult::Custom {
                    name: action_type.to_string(),
                    data: action,
                })
            }
            "disconnect" => Ok(ClientActionResult::Disconnect),
            "wait_for_more" => Ok(ClientActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!("Unknown NFS client action: {}", action_type)),
        }
    }
}
