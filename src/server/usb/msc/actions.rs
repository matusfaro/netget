//! USB Mass Storage Class protocol actions implementation

#[cfg(feature = "usb-msc")]
use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
#[cfg(feature = "usb-msc")]
use crate::protocol::log_template::LogTemplate;
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

// Event type definitions (static for efficient reuse)
#[cfg(feature = "usb-msc")]
pub static USB_MSC_ATTACHED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "usb_msc_attached",
        "A USB/IP host attached to this virtual mass-storage device and can now read and write \
         sectors. Mount a disk image to give it contents, set write protection, or wait_for_more \
         to leave the currently mounted image in place.",
        json!({
            "type": "wait_for_more"
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "connection_id".to_string(),
            type_hint: "string".to_string(),
            description: "Connection ID of the USB/IP session".to_string(),
            required: true,
        },
        Parameter {
            name: "total_sectors".to_string(),
            type_hint: "number".to_string(),
            description: "Total number of 512-byte sectors".to_string(),
            required: true,
        },
        Parameter {
            name: "capacity_mb".to_string(),
            type_hint: "number".to_string(),
            description: "Total capacity in megabytes".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        mount_disk_action(),
        eject_disk_action(),
        set_write_protect_action(),
        wait_for_more_action(),
    ])
});

#[cfg(feature = "usb-msc")]
pub static USB_MSC_DETACHED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "usb_msc_detached",
        "The host detached from this virtual mass-storage device. Purely informational - the \
         USB/IP session is gone, so there is nothing left to mount, eject or write-protect.",
        json!({"type": "show_message", "message": "USB mass storage host detached"}),
    )
    .with_parameters(vec![Parameter {
        name: "connection_id".to_string(),
        type_hint: "string".to_string(),
        description: "Connection ID of the USB/IP session".to_string(),
        required: true,
    }])
    .with_no_actions()
});

#[cfg(feature = "usb-msc")]
pub static USB_MSC_READ_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "usb_msc_read",
        "The host read sectors from the mass-storage device. Informational: the read has already \
         been served from the mounted image. Answer wait_for_more unless you want to swap the \
         image or change write protection in response.",
        json!({
            "type": "wait_for_more"
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "connection_id".to_string(),
            type_hint: "string".to_string(),
            description: "Connection ID".to_string(),
            required: true,
        },
        Parameter {
            name: "lba".to_string(),
            type_hint: "number".to_string(),
            description: "Logical Block Address (starting sector)".to_string(),
            required: true,
        },
        Parameter {
            name: "sector_count".to_string(),
            type_hint: "number".to_string(),
            description: "Number of sectors read".to_string(),
            required: true,
        },
        Parameter {
            name: "bytes_read".to_string(),
            type_hint: "number".to_string(),
            description: "Total bytes read".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        mount_disk_action(),
        eject_disk_action(),
        set_write_protect_action(),
        wait_for_more_action(),
    ])
});

#[cfg(feature = "usb-msc")]
pub static USB_MSC_WRITE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "usb_msc_write",
        "The host wrote sectors to the mass-storage device. Informational: the write has already \
         been applied to the mounted image. Answer wait_for_more unless you want to swap the \
         image or write-protect it in response.",
        json!({
            "type": "wait_for_more"
        }),
    )
    .with_parameters(vec![
        Parameter {
            name: "connection_id".to_string(),
            type_hint: "string".to_string(),
            description: "Connection ID".to_string(),
            required: true,
        },
        Parameter {
            name: "lba".to_string(),
            type_hint: "number".to_string(),
            description: "Logical Block Address (starting sector)".to_string(),
            required: true,
        },
        Parameter {
            name: "sector_count".to_string(),
            type_hint: "number".to_string(),
            description: "Number of sectors written".to_string(),
            required: true,
        },
        Parameter {
            name: "bytes_written".to_string(),
            type_hint: "number".to_string(),
            description: "Total bytes written".to_string(),
            required: true,
        },
    ])
    .with_actions(vec![
        mount_disk_action(),
        eject_disk_action(),
        set_write_protect_action(),
        wait_for_more_action(),
    ])
});

/// The per-connection MSC handlers, keyed by connection.
///
/// A `std::sync::Mutex` rather than a tokio one because `usbip` requires
/// `Arc<Mutex<Box<dyn UsbInterfaceHandler + Send>>>` from `std`, and because `execute_action`
/// is synchronous — it runs on a tokio worker, where the `Handle::current().block_on(...)`
/// this registry used to need would panic outright. The guard is never held across an
/// `.await`.
#[cfg(feature = "usb-msc")]
type SharedHandler = Arc<std::sync::Mutex<Box<dyn usbip::UsbInterfaceHandler + Send>>>;

/// USB Mass Storage Class protocol action handler
#[cfg(feature = "usb-msc")]
pub struct UsbMscProtocol {
    handlers: Arc<std::sync::Mutex<HashMap<ConnectionId, SharedHandler>>>,
}

#[cfg(feature = "usb-msc")]
impl Default for UsbMscProtocol {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "usb-msc")]
impl UsbMscProtocol {
    pub fn new() -> Self {
        Self {
            handlers: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Register the handler that drives one attached device.
    pub fn set_handler(&self, connection_id: ConnectionId, handler: SharedHandler) {
        if let Ok(mut handlers) = self.handlers.lock() {
            handlers.insert(connection_id, handler);
        }
    }

    /// Drop the handler for a device whose host has detached, so a later action cannot reach
    /// a device that is gone.
    pub fn remove_handler(&self, connection_id: ConnectionId) {
        if let Ok(mut handlers) = self.handlers.lock() {
            handlers.remove(&connection_id);
        }
    }

    /// Resolve which attached device an action refers to.
    ///
    /// An explicit `connection_id` wins. Otherwise the single attached device is used — and if
    /// there is not exactly one, this is an error rather than a guess: ejecting the wrong disk
    /// is indistinguishable from ejecting the right one from the model's side.
    fn resolve_handler(&self, action: &serde_json::Value) -> Result<SharedHandler> {
        let handlers = self
            .handlers
            .lock()
            .map_err(|_| anyhow::anyhow!("USB MSC handler registry poisoned"))?;

        // Models emit ids both as numbers and as strings; accept either.
        let requested = action["connection_id"].as_u64().or_else(|| {
            action["connection_id"]
                .as_str()
                .and_then(|s| s.trim().parse::<u64>().ok())
        });

        if let Some(id) = requested {
            let connection_id = ConnectionId::new(id as u32);
            return handlers.get(&connection_id).cloned().ok_or_else(|| {
                anyhow::anyhow!(
                    "No USB mass storage device attached on connection {}",
                    connection_id
                )
            });
        }

        match handlers.len() {
            0 => Err(anyhow::anyhow!(
                "No USB mass storage host is attached, so there is no device to act on"
            )),
            1 => Ok(handlers.values().next().cloned().expect("len checked")),
            _ => {
                let mut ids: Vec<u32> = handlers.keys().map(|c| c.as_u32()).collect();
                ids.sort_unstable();
                Err(anyhow::anyhow!(
                    "{} USB mass storage hosts are attached ({:?}); the action must name one \
                     with 'connection_id'",
                    ids.len(),
                    ids
                ))
            }
        }
    }

    /// Run `f` against the MSC handler an action refers to.
    fn with_msc_handler<T>(
        &self,
        action: &serde_json::Value,
        f: impl FnOnce(&mut super::handler::UsbMscHandler) -> T,
    ) -> Result<T> {
        let handler = self.resolve_handler(action)?;
        let mut guard = handler
            .lock()
            .map_err(|_| anyhow::anyhow!("USB MSC handler mutex poisoned"))?;
        let msc = guard
            .as_any()
            .downcast_mut::<super::handler::UsbMscHandler>()
            .context("Handler is not a USB mass storage handler")?;
        Ok(f(msc))
    }
}

// Implement Protocol trait
#[cfg(feature = "usb-msc")]
impl Protocol for UsbMscProtocol {
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        vec![crate::llm::actions::ParameterDefinition {
            name: "disk_image".to_string(),
            type_hint: "string".to_string(),
            description: "Path to disk image file (default: ./tmp/netget_msc_disk.img, will be created if doesn't exist)"
                .to_string(),
            required: false,
            example: serde_json::json!("./tmp/netget_msc_disk.img"),
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
        crate::protocol::metadata::ProtocolMetadataV2::builder()
            .state(crate::protocol::metadata::DevelopmentState::Experimental)
            .implementation(
                "Virtual USB mass storage over USB/IP: BOT plus a SCSI-2 subset (INQUIRY, \
                 TEST UNIT READY, READ CAPACITY(10), READ(10), WRITE(10), REQUEST SENSE, \
                 MODE SENSE(6), PREVENT/ALLOW MEDIUM REMOVAL, READ FORMAT CAPACITIES) served \
                 synchronously from a memory-mapped disk image. usbip::handler runs on the \
                 accepted socket, so the listen port is whatever the caller asks for.",
            )
            .llm_control(
                "mount_disk swaps the image (existing images keep their own size), eject_disk \
                 makes every medium command report NOT READY, set_write_protect toggles \
                 DATA PROTECT on WRITE(10). All four events fire: usb_msc_attached on connect, \
                 usb_msc_read / usb_msc_write after sector transfers (coalesced - a mount does \
                 hundreds), usb_msc_detached when the USB/IP session ends.",
            )
            .e2e_testing(
                "E2E drives a real USB/IP client over TCP (OP_REQ_IMPORT, then CBW/CSW over \
                 the bulk endpoints) against a FAT16 image built in the test, and asserts the \
                 sectors the host reads contain the file. No usbip kernel module or root.",
            )
            .privilege_requirement(crate::protocol::metadata::PrivilegeRequirement::None)
            .notes(
                "Storage is a disk image file on the host, which sits awkwardly with the \
                 no-storage rule; an LLM-driven file mode is the intended direction. Attaching \
                 from a real host still needs vhci-hcd and root on the client side and is \
                 untested - the E2E harness proves the device side, not that an OS mounts the \
                 filesystem. Single LUN only.",
            )
            .build()
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

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        use serde_json::json;

        StartupExamples::new(
            // LLM mode: LLM handles USB mass storage device
            json!({
                "type": "open_server",
                "port": 3240,
                "base_stack": "USB-MassStorage",
                "instruction": "Create a USB mass storage device with a 100MB disk image",
                "startup_params": {
                    "disk_image": "./tmp/netget_msc_disk.img"
                }
            }),
            // Script mode: Code-based storage handling
            json!({
                "type": "open_server",
                "port": 3240,
                "base_stack": "USB-MassStorage",
                "startup_params": {
                    "disk_image": "./tmp/netget_msc_disk.img"
                },
                "event_handlers": [{
                    "event_pattern": "usb_msc_attached",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "<msc_handler>"
                    }
                }]
            }),
            // Static mode: Fixed storage action
            json!({
                "type": "open_server",
                "port": 3240,
                "base_stack": "USB-MassStorage",
                "startup_params": {
                    "disk_image": "./tmp/netget_msc_disk.img"
                },
                "event_handlers": [{
                    "event_pattern": "usb_msc_attached",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "wait_for_more"
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait
#[cfg(feature = "usb-msc")]
impl Server for UsbMscProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<std::net::SocketAddr>> + Send>>
    {
        Box::pin(async move {
            let disk_image = ctx
                .startup_params
                .as_ref()
                .map(|p| p.get_optional_string("disk_image"))
                .transpose()?
                .flatten()
                .map(std::path::PathBuf::from);

            crate::server::usb::msc::UsbMscServer::spawn_with_llm_actions(
                ctx.legacy_listen_addr(),
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
                disk_image,
            )
            .await
        })
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action["type"]
            .as_str()
            .context("Action must have 'type' field")?;

        match action_type {
            "mount_disk" => {
                let disk_image_path = action["disk_image"]
                    .as_str()
                    .context("mount_disk requires 'disk_image' field")?;
                // Write protection defaults to *on*, matching how a device starts up. A model
                // that wants the host to be able to write has to say so.
                let write_protect = action["write_protect"].as_bool().unwrap_or(true);
                let size_mb = action["size_mb"].as_u64().unwrap_or(10);
                let size_mb = u32::try_from(size_mb).context("size_mb is out of range")?;
                if size_mb == 0 {
                    anyhow::bail!("size_mb must be at least 1");
                }

                // Open the image before touching the device: a bad path must not leave the
                // handler half-remounted.
                let path = std::path::Path::new(disk_image_path);
                let disk = super::disk::DiskImage::open_or_create(path, size_mb)
                    .with_context(|| format!("Failed to open disk image '{}'", disk_image_path))?;
                let sectors = disk.total_sectors();
                let disk = std::sync::Arc::new(std::sync::Mutex::new(disk));

                self.with_msc_handler(&action, |msc| {
                    msc.mount_disk(disk);
                    msc.set_write_protect(write_protect);
                })?;

                tracing::info!(
                    "USB MSC: Mounted disk '{}' ({} sectors, write_protect={})",
                    disk_image_path,
                    sectors,
                    write_protect
                );
                Ok(ActionResult::NoAction)
            }
            "eject_disk" => {
                self.with_msc_handler(&action, |msc| msc.eject_disk())?;
                tracing::info!("USB MSC: Disk ejected");
                Ok(ActionResult::NoAction)
            }
            "set_write_protect" => {
                let enabled = action["enabled"]
                    .as_bool()
                    .context("set_write_protect requires 'enabled' boolean field")?;
                self.with_msc_handler(&action, |msc| msc.set_write_protect(enabled))?;
                tracing::info!(
                    "USB MSC: Write protection {}",
                    if enabled { "enabled" } else { "disabled" }
                );
                Ok(ActionResult::NoAction)
            }
            "wait_for_more" => Ok(ActionResult::WaitForMore),
            _ => Err(anyhow::anyhow!("Unknown action type: {}", action_type)),
        }
    }
}

// Action definitions

/// Optional device selector shared by every action.
///
/// It is optional on purpose. A virtual drive normally has exactly one host attached, and
/// asking a model to copy an id back is a reliable source of wrong answers. When exactly one
/// host is attached the action needs no id; when several are, an omitted id is an error naming
/// the candidates rather than a guess.
#[cfg(feature = "usb-msc")]
fn connection_id_parameter() -> Parameter {
    Parameter {
        name: "connection_id".to_string(),
        type_hint: "string".to_string(),
        description: "Which attached device to act on. Omit it when only one host is attached; \
            required (copy it from the event) when there are several."
            .to_string(),
        required: false,
    }
}

#[cfg(feature = "usb-msc")]
fn mount_disk_action() -> ActionDefinition {
    ActionDefinition {
        name: "mount_disk".to_string(),
        description: "Mount a disk image file as the virtual mass storage device. An image that \
            already exists is served as-is at its own size; a missing one is created empty."
            .to_string(),
        parameters: vec![
            connection_id_parameter(),
            Parameter {
                name: "disk_image".to_string(),
                type_hint: "string".to_string(),
                description: "Path to disk image file".to_string(),
                required: true,
            },
            Parameter {
                name: "write_protect".to_string(),
                type_hint: "boolean".to_string(),
                description: "Enable write protection (default: true)".to_string(),
                required: false,
            },
            Parameter {
                name: "size_mb".to_string(),
                type_hint: "number".to_string(),
                description: "Size in megabytes, used only when creating a new image \
                    (default: 10). Ignored for an image that already exists."
                    .to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "mount_disk",
            "disk_image": "/path/to/disk.img",
            "write_protect": true
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> USB MSC mount disk '{disk_image}'")
                .with_debug("USB-MSC mount_disk: path={disk_image} write_protect={write_protect}"),
        ),
    }
}

#[cfg(feature = "usb-msc")]
fn eject_disk_action() -> ActionDefinition {
    ActionDefinition {
        name: "eject_disk".to_string(),
        description: "Eject the currently mounted disk image. Every command that needs the \
            medium then fails with NOT READY until mount_disk supplies a new image."
            .to_string(),
        parameters: vec![connection_id_parameter()],
        example: json!({
            "type": "eject_disk"
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> USB MSC eject disk")
                .with_debug("USB-MSC eject_disk: connection_id={connection_id}"),
        ),
    }
}

#[cfg(feature = "usb-msc")]
fn set_write_protect_action() -> ActionDefinition {
    ActionDefinition {
        name: "set_write_protect".to_string(),
        description: "Enable or disable write protection on the virtual disk".to_string(),
        parameters: vec![
            connection_id_parameter(),
            Parameter {
                name: "enabled".to_string(),
                type_hint: "boolean".to_string(),
                description: "true to enable write protection, false to disable".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "set_write_protect",
            "enabled": true
        }),
        log_template: Some(
            LogTemplate::new()
                .with_info("-> USB MSC write protect {enabled}")
                .with_debug("USB-MSC set_write_protect: enabled={enabled}"),
        ),
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
        log_template: Some(
            LogTemplate::new()
                .with_info("-> USB MSC wait for more")
                .with_debug("USB-MSC wait_for_more"),
        ),
    }
}
