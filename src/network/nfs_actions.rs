//! NFS protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, ProtocolActions},
    ActionDefinition, Parameter,
};
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;

/// NFS protocol action handler
pub struct NfsProtocol;

impl NfsProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl ProtocolActions for NfsProtocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            mount_filesystem_action(),
            unmount_filesystem_action(),
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            lookup_file_action(),
            read_file_action(),
            write_file_action(),
            create_file_action(),
            remove_file_action(),
            get_attributes_action(),
        ]
    }

    fn execute_action(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "mount_filesystem" => self.execute_mount_filesystem(action),
            "unmount_filesystem" => self.execute_unmount_filesystem(action),
            "lookup_file" => self.execute_lookup_file(action),
            "read_file" => self.execute_read_file(action),
            "write_file" => self.execute_write_file(action),
            "create_file" => self.execute_create_file(action),
            "remove_file" => self.execute_remove_file(action),
            "get_attributes" => self.execute_get_attributes(action),
            _ => Err(anyhow::anyhow!("Unknown NFS action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "NFS"
    }
}

impl NfsProtocol {
    /// Mount a filesystem export
    fn execute_mount_filesystem(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let path = action
            .get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' parameter")?;

        Ok(ActionResult::NoAction)
    }

    /// Unmount a filesystem
    fn execute_unmount_filesystem(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let path = action
            .get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' parameter")?;

        Ok(ActionResult::NoAction)
    }

    /// Look up a file or directory
    fn execute_lookup_file(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let path = action
            .get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' parameter")?;

        Ok(ActionResult::NoAction)
    }

    /// Read file contents
    fn execute_read_file(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let path = action
            .get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' parameter")?;

        let offset = action
            .get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let count = action
            .get("count")
            .and_then(|v| v.as_u64())
            .unwrap_or(4096);

        Ok(ActionResult::NoAction)
    }

    /// Write file contents
    fn execute_write_file(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let path = action
            .get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' parameter")?;

        let data = action
            .get("data")
            .and_then(|v| v.as_str())
            .context("Missing 'data' parameter")?;

        let offset = action
            .get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        Ok(ActionResult::NoAction)
    }

    /// Create a new file
    fn execute_create_file(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let path = action
            .get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' parameter")?;

        Ok(ActionResult::NoAction)
    }

    /// Remove a file
    fn execute_remove_file(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let path = action
            .get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' parameter")?;

        Ok(ActionResult::NoAction)
    }

    /// Get file attributes
    fn execute_get_attributes(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let path = action
            .get("path")
            .and_then(|v| v.as_str())
            .context("Missing 'path' parameter")?;

        Ok(ActionResult::NoAction)
    }
}

/// Action definitions

fn mount_filesystem_action() -> ActionDefinition {
    ActionDefinition {
        name: "mount_filesystem".to_string(),
        description: "Mount an NFS filesystem export".to_string(),
        parameters: vec![
            Parameter {
                name: "path".to_string(),
                type_hint: "string".to_string(),
                description: "Export path to mount".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "mount_filesystem",
            "path": "/export/data"
        }),
    }
}

fn unmount_filesystem_action() -> ActionDefinition {
    ActionDefinition {
        name: "unmount_filesystem".to_string(),
        description: "Unmount an NFS filesystem".to_string(),
        parameters: vec![
            Parameter {
                name: "path".to_string(),
                type_hint: "string".to_string(),
                description: "Export path to unmount".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "unmount_filesystem",
            "path": "/export/data"
        }),
    }
}

fn lookup_file_action() -> ActionDefinition {
    ActionDefinition {
        name: "lookup_file".to_string(),
        description: "Look up a file or directory by path".to_string(),
        parameters: vec![
            Parameter {
                name: "path".to_string(),
                type_hint: "string".to_string(),
                description: "Path to look up".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "lookup_file",
            "path": "/export/data/file.txt"
        }),
    }
}

fn read_file_action() -> ActionDefinition {
    ActionDefinition {
        name: "read_file".to_string(),
        description: "Read data from a file".to_string(),
        parameters: vec![
            Parameter {
                name: "path".to_string(),
                type_hint: "string".to_string(),
                description: "Path to file".to_string(),
                required: true,
            },
            Parameter {
                name: "offset".to_string(),
                type_hint: "number".to_string(),
                description: "Offset to start reading from (default: 0)".to_string(),
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
            "type": "read_file",
            "path": "/export/data/file.txt",
            "offset": 0,
            "count": 1024
        }),
    }
}

fn write_file_action() -> ActionDefinition {
    ActionDefinition {
        name: "write_file".to_string(),
        description: "Write data to a file".to_string(),
        parameters: vec![
            Parameter {
                name: "path".to_string(),
                type_hint: "string".to_string(),
                description: "Path to file".to_string(),
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
                description: "Offset to start writing at (default: 0)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "write_file",
            "path": "/export/data/file.txt",
            "data": "Hello NFS!",
            "offset": 0
        }),
    }
}

fn create_file_action() -> ActionDefinition {
    ActionDefinition {
        name: "create_file".to_string(),
        description: "Create a new file".to_string(),
        parameters: vec![
            Parameter {
                name: "path".to_string(),
                type_hint: "string".to_string(),
                description: "Path where file should be created".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "create_file",
            "path": "/export/data/newfile.txt"
        }),
    }
}

fn remove_file_action() -> ActionDefinition {
    ActionDefinition {
        name: "remove_file".to_string(),
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
            "type": "remove_file",
            "path": "/export/data/oldfile.txt"
        }),
    }
}

fn get_attributes_action() -> ActionDefinition {
    ActionDefinition {
        name: "get_attributes".to_string(),
        description: "Get file or directory attributes (size, permissions, etc.)".to_string(),
        parameters: vec![
            Parameter {
                name: "path".to_string(),
                type_hint: "string".to_string(),
                description: "Path to get attributes for".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "get_attributes",
            "path": "/export/data/file.txt"
        }),
    }
}
