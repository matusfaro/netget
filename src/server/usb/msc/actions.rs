//! USB Mass Storage Class protocol actions implementation

#[cfg(feature = "usb-msc")]
use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
#[cfg(feature = "usb-msc")]
use crate::protocol::EventType;
#[cfg(feature = "usb-msc")]
use crate::server::connection::ConnectionId;
#[cfg(feature = "usb-msc")]
use crate::state::app_state::AppState;
#[cfg(feature = "usb-msc")]
use anyhow::{Context, Result};
#[cfg(feature = "usb-msc")]
use serde_json::json;
#[cfg(feature = "usb-msc")]
use std::collections::HashMap;
#[cfg(feature = "usb-msc")]
use std::sync::{Arc, LazyLock};
#[cfg(feature = "usb-msc")]
use tokio::sync::Mutex;

// Event type definitions (static for efficient reuse)
#[cfg(feature = "usb-msc")]
pub static USB_MSC_ATTACHED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "usb_msc_attached",
        "Host attached to USB mass storage device",
    )
    .with_parameters(vec![
        Parameter::new("connection_id", "string", "Connection ID of the USB/IP session"),
        Parameter::new("total_sectors", "number", "Total number of 512-byte sectors"),
        Parameter::new("capacity_mb", "number", "Total capacity in megabytes"),
    ])
});

#[cfg(feature = "usb-msc")]
pub static USB_MSC_DETACHED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "usb_msc_detached",
        "Host detached from USB mass storage device",
    )
    .with_parameters(vec![
        Parameter::new("connection_id", "string", "Connection ID of the USB/IP session"),
    ])
});

#[cfg(feature = "usb-msc")]
pub static USB_MSC_READ_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("usb_msc_read", "Host read sectors from the mass storage device")
        .with_parameters(vec![
            Parameter::new("connection_id", "string", "Connection ID"),
            Parameter::new("lba", "number", "Logical Block Address (starting sector)"),
            Parameter::new("sector_count", "number", "Number of sectors read"),
            Parameter::new("bytes_read", "number", "Total bytes read"),
        ])
});

#[cfg(feature = "usb-msc")]
pub static USB_MSC_WRITE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "usb_msc_write",
        "Host wrote sectors to the mass storage device",
    )
    .with_parameters(vec![
        Parameter::new("connection_id", "string", "Connection ID"),
        Parameter::new("lba", "number", "Logical Block Address (starting sector)"),
        Parameter::new("sector_count", "number", "Number of sectors written"),
        Parameter::new("bytes_written", "number", "Total bytes written"),
    ])
});

/// USB Mass Storage Class protocol action handler
#[cfg(feature = "usb-msc")]
pub struct UsbMscProtocol {
    /// Map of active connections (for async actions)
    connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
}

#[cfg(feature = "usb-msc")]
pub struct ConnectionData {
    // Placeholder for MSC-specific connection data
    // Will be populated during full implementation
}

#[cfg(feature = "usb-msc")]
impl UsbMscProtocol {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

// Implement Protocol trait
#[cfg(feature = "usb-msc")]
impl Protocol for UsbMscProtocol {
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        vec![crate::llm::actions::ParameterDefinition {
            name: "disk_image".to_string(),
            description: "Path to disk image file (will be created if doesn't exist)"
                .to_string(),
            required: false,
            default: Some("/tmp/netget_msc_disk.img".to_string()),
        }]
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            mount_disk_action(),
            eject_disk_action(),
            set_write_protect_action(),
            wait_for_more_action(),
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "USB-MassStorage"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            USB_MSC_ATTACHED_EVENT.clone(),
            USB_MSC_DETACHED_EVENT.clone(),
            USB_MSC_READ_EVENT.clone(),
            USB_MSC_WRITE_EVENT.clone(),
        ]
    }

    fn stack_name(&self) -> &'static str {
        "USB>MSC>SCSI"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["usb", "storage", "disk", "msc", "scsi", "flash"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        crate::protocol::metadata::ProtocolMetadataV2::new(
            crate::protocol::metadata::ProtocolState::Experimental,
            "Virtual USB Mass Storage device using USB/IP protocol",
            "LLM controls virtual disk (mount, eject, write-protect)",
            "E2E tests pending full SCSI implementation",
            crate::protocol::metadata::PrivilegeRequirement::None,
        )
        .with_notes("Requires client to have vhci-hcd kernel module. SCSI command implementation pending.")
    }

    fn description(&self) -> &'static str {
        "Virtual USB Mass Storage device (flash drive/disk)"
    }

    fn example_prompt(&self) -> &'static str {
        "Create a USB mass storage device with a 100MB disk image"
    }

    fn group_name(&self) -> &'static str {
        "USB Devices"
    }
}

// Implement Server trait
#[cfg(feature = "usb-msc")]
impl Server for UsbMscProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>,
    > {
        let disk_image = ctx
            .startup_params
            .get("disk_image")
            .and_then(|v| v.as_str())
            .map(std::path::PathBuf::from);

        Box::pin(async move {
            crate::server::usb::msc::UsbMscServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
                disk_image,
            )
            .await
        })
    }

    fn execute_action(
        &self,
        action: serde_json::Value,
        _connection_id: Option<ConnectionId>,
        _app_state: &AppState,
    ) -> Result<ActionResult> {
        let action_type = action["type"]
            .as_str()
            .context("Action must have 'type' field")?;

        match action_type {
            "mount_disk" => {
                let _disk_image = action["disk_image"]
                    .as_str()
                    .context("mount_disk requires 'disk_image' field")?;
                // TODO: Implement disk mounting via SCSI
                Ok(ActionResult::NoAction)
            }
            "eject_disk" => {
                // TODO: Implement disk ejection via SCSI
                Ok(ActionResult::NoAction)
            }
            "set_write_protect" => {
                let _enabled = action["enabled"]
                    .as_bool()
                    .context("set_write_protect requires 'enabled' boolean field")?;
                // TODO: Implement write-protect flag
                Ok(ActionResult::NoAction)
            }
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!("Unknown action type: {}", action_type)),
        }
    }
}

// Action definitions

#[cfg(feature = "usb-msc")]
fn mount_disk_action() -> ActionDefinition {
    ActionDefinition {
        name: "mount_disk".to_string(),
        description: "Mount a disk image file as the virtual mass storage device".to_string(),
        parameters: vec![
            Parameter::new("disk_image", "string", "Path to disk image file"),
            Parameter::new(
                "write_protect",
                "boolean",
                "Enable write protection (default: false)",
            )
            .optional(),
        ],
        example: json!({
            "type": "mount_disk",
            "disk_image": "/path/to/disk.img",
            "write_protect": false
        }),
    }
}

#[cfg(feature = "usb-msc")]
fn eject_disk_action() -> ActionDefinition {
    ActionDefinition {
        name: "eject_disk".to_string(),
        description: "Eject the currently mounted disk image".to_string(),
        parameters: vec![],
        example: json!({
            "type": "eject_disk"
        }),
    }
}

#[cfg(feature = "usb-msc")]
fn set_write_protect_action() -> ActionDefinition {
    ActionDefinition {
        name: "set_write_protect".to_string(),
        description: "Enable or disable write protection on the virtual disk".to_string(),
        parameters: vec![Parameter::new(
            "enabled",
            "boolean",
            "true to enable write protection, false to disable",
        )],
        example: json!({
            "type": "set_write_protect",
            "enabled": true
        }),
    }
}

#[cfg(feature = "usb-msc")]
fn wait_for_more_action() -> ActionDefinition {
    ActionDefinition {
        name: "wait_for_more".to_string(),
        description: "Wait for more data or events before taking action".to_string(),
        parameters: vec![],
        example: json!({
            "type": "wait_for_more"
        }),
    }
}
