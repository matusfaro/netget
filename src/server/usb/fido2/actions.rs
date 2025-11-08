//! USB FIDO2/U2F Security Key protocol actions implementation
//!
//! This module implements a virtual FIDO2/U2F security key over USB/IP.
//! Architecture inspired by softfido (https://github.com/ellerh/softfido)
//! but implemented independently for NetGet.

#[cfg(feature = "usb-fido2")]
use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
#[cfg(feature = "usb-fido2")]
use crate::protocol::EventType;
#[cfg(feature = "usb-fido2")]
use crate::server::connection::ConnectionId;
#[cfg(feature = "usb-fido2")]
use crate::state::app_state::AppState;
#[cfg(feature = "usb-fido2")]
use anyhow::{Context, Result};
#[cfg(feature = "usb-fido2")]
use serde_json::json;
#[cfg(feature = "usb-fido2")]
use std::collections::HashMap;
#[cfg(feature = "usb-fido2")]
use std::sync::{Arc, LazyLock};
#[cfg(feature = "usb-fido2")]
use tokio::sync::Mutex;

// Event type definitions
#[cfg(feature = "usb-fido2")]
pub static FIDO2_DEVICE_ATTACHED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "fido2_device_attached",
        "FIDO2 security key attached to host",
    )
    .with_parameters(vec![
        Parameter::new("connection_id", "string", "Connection ID of the USB/IP session"),
        Parameter::new("supports_u2f", "boolean", "Supports U2F (CTAP1) protocol"),
        Parameter::new("supports_fido2", "boolean", "Supports FIDO2 (CTAP2) protocol"),
    ])
});

#[cfg(feature = "usb-fido2")]
pub static FIDO2_REGISTER_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "fido2_register_request",
        "User requested to register new credential",
    )
    .with_parameters(vec![
        Parameter::new("connection_id", "string", "Connection ID"),
        Parameter::new("rp_id", "string", "Relying party ID (website/app)"),
        Parameter::new("user_name", "string", "User name for the credential"),
        Parameter::new("requires_approval", "boolean", "Requires user presence confirmation"),
    ])
});

#[cfg(feature = "usb-fido2")]
pub static FIDO2_AUTHENTICATE_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "fido2_authenticate_request",
        "User requested to authenticate with credential",
    )
    .with_parameters(vec![
        Parameter::new("connection_id", "string", "Connection ID"),
        Parameter::new("rp_id", "string", "Relying party ID (website/app)"),
        Parameter::new("credential_count", "number", "Number of matching credentials"),
        Parameter::new("requires_approval", "boolean", "Requires user presence confirmation"),
    ])
});

/// USB FIDO2 protocol action handler
#[cfg(feature = "usb-fido2")]
pub struct UsbFido2Protocol {
    /// Map of active connections
    connections: Arc<Mutex<HashMap<ConnectionId, ConnectionData>>>,
    /// Map of USB/IP HID handlers for each connection
    handlers: Arc<Mutex<HashMap<ConnectionId, Arc<std::sync::Mutex<Box<dyn usbip::UsbInterfaceHandler + Send>>>>>>,
}

#[cfg(feature = "usb-fido2")]
pub struct ConnectionData {
    /// Whether user approval is pending
    pub approval_pending: bool,
    /// Pending operation type (register/authenticate)
    pub pending_operation: Option<String>,
}

#[cfg(feature = "usb-fido2")]
impl UsbFido2Protocol {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            handlers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Store the USB/IP FIDO2 handler for a connection
    pub async fn set_handler(
        &self,
        connection_id: ConnectionId,
        handler: Arc<std::sync::Mutex<Box<dyn usbip::UsbInterfaceHandler + Send>>>,
    ) {
        self.handlers.lock().await.insert(connection_id, handler);
    }

    /// Get the USB/IP FIDO2 handler for a connection
    async fn get_handler(
        &self,
        connection_id: ConnectionId,
    ) -> Option<Arc<std::sync::Mutex<Box<dyn usbip::UsbInterfaceHandler + Send>>>> {
        self.handlers.lock().await.get(&connection_id).cloned()
    }
}

// Implement Protocol trait
#[cfg(feature = "usb-fido2")]
impl Protocol for UsbFido2Protocol {
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        vec![
            crate::llm::actions::ParameterDefinition {
                name: "support_u2f".to_string(),
                param_type: "boolean".to_string(),
                description: "Enable U2F (CTAP1) support".to_string(),
                required: false,
                default_value: Some("true".to_string()),
            },
            crate::llm::actions::ParameterDefinition {
                name: "support_fido2".to_string(),
                param_type: "boolean".to_string(),
                description: "Enable FIDO2 (CTAP2) support".to_string(),
                required: false,
                default_value: Some("true".to_string()),
            },
            crate::llm::actions::ParameterDefinition {
                name: "auto_approve".to_string(),
                param_type: "boolean".to_string(),
                description: "Automatically approve authentication requests (dev mode)".to_string(),
                required: false,
                default_value: Some("false".to_string()),
            },
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "usb-fido2"
    }

    fn stack_name(&self) -> &'static str {
        "USB FIDO2/U2F Security Key"
    }

    fn get_async_actions(&self) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "approve_request".to_string(),
                description: "Approve pending FIDO2 registration or authentication request".to_string(),
                parameters: vec![
                    Parameter::new("connection_id", "string", "Connection ID"),
                ],
            },
            ActionDefinition {
                name: "deny_request".to_string(),
                description: "Deny pending FIDO2 registration or authentication request".to_string(),
                parameters: vec![
                    Parameter::new("connection_id", "string", "Connection ID"),
                ],
            },
            ActionDefinition {
                name: "list_credentials".to_string(),
                description: "List all stored FIDO2 credentials".to_string(),
                parameters: vec![],
            },
            ActionDefinition {
                name: "delete_credential".to_string(),
                description: "Delete a stored FIDO2 credential".to_string(),
                parameters: vec![
                    Parameter::new("credential_id", "string", "Credential ID to delete"),
                ],
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![]
    }

    fn execute_action(
        &self,
        action: serde_json::Value,
        connection_id: Option<ConnectionId>,
        _app_state: Arc<AppState>,
    ) -> Result<ActionResult> {
        let action_type = action["type"]
            .as_str()
            .context("Missing action type")?;

        match action_type {
            "approve_request" => {
                // TODO: Implement approval logic
                // This would signal the FIDO2 handler to proceed with the pending operation
                Ok(ActionResult::NoAction)
            }
            "deny_request" => {
                // TODO: Implement denial logic
                Ok(ActionResult::NoAction)
            }
            "list_credentials" => {
                // TODO: Return list of credentials
                Ok(ActionResult::NoAction)
            }
            "delete_credential" => {
                // TODO: Delete credential from storage
                Ok(ActionResult::NoAction)
            }
            _ => Ok(ActionResult::NoAction),
        }
    }

    fn get_event_types(&self) -> Vec<&EventType> {
        vec![
            &FIDO2_DEVICE_ATTACHED_EVENT,
            &FIDO2_REGISTER_REQUEST_EVENT,
            &FIDO2_AUTHENTICATE_REQUEST_EVENT,
        ]
    }
}

// Implement Server trait
#[cfg(feature = "usb-fido2")]
impl Server for UsbFido2Protocol {}
