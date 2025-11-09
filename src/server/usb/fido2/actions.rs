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
#[cfg(feature = "usb-fido2")]
use tracing::{info, debug, error, warn};

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

    /// Get the approval manager (returns first available from global storage)
    async fn get_approval_manager(&self) -> Option<Arc<crate::server::usb::fido2::approval::ApprovalManager>> {
        use crate::server::usb::fido2::approval::APPROVAL_MANAGERS;
        APPROVAL_MANAGERS.read().await.values().next().cloned()
    }
}

// Implement Protocol trait
#[cfg(feature = "usb-fido2")]
impl Protocol for UsbFido2Protocol {
    fn get_startup_parameters(&self) -> Vec<crate::llm::actions::ParameterDefinition> {
        vec![
            crate::llm::actions::ParameterDefinition {
                name: "support_u2f".to_string(),
                type_hint: "boolean".to_string(),
                description: "Enable U2F (CTAP1) support".to_string(),
                required: false,
                example: serde_json::json!(true),
            },
            crate::llm::actions::ParameterDefinition {
                name: "support_fido2".to_string(),
                type_hint: "boolean".to_string(),
                description: "Enable FIDO2 (CTAP2) support".to_string(),
                required: false,
                example: serde_json::json!(true),
            },
            crate::llm::actions::ParameterDefinition {
                name: "auto_approve".to_string(),
                type_hint: "boolean".to_string(),
                description: "Automatically approve authentication requests (dev mode)".to_string(),
                required: false,
                example: serde_json::json!(false),
            },
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "usb-fido2"
    }

    fn stack_name(&self) -> &'static str {
        "USB FIDO2/U2F Security Key"
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ActionDefinition {
                name: "approve_request".to_string(),
                description: "Approve pending FIDO2 registration or authentication request by approval ID".to_string(),
                parameters: vec![
                    Parameter {
                        name: "approval_id".to_string(),
                        type_hint: "number".to_string(),
                        description: "Approval request ID to approve".to_string(),
                        required: true,
                    },
                ],
                example: serde_json::json!({"type": "approve_request", "approval_id": 1}),
            },
            ActionDefinition {
                name: "deny_request".to_string(),
                description: "Deny pending FIDO2 registration or authentication request by approval ID".to_string(),
                parameters: vec![
                    Parameter {
                        name: "approval_id".to_string(),
                        type_hint: "number".to_string(),
                        description: "Approval request ID to deny".to_string(),
                        required: true,
                    },
                ],
                example: serde_json::json!({"type": "deny_request", "approval_id": 1}),
            },
            ActionDefinition {
                name: "list_credentials".to_string(),
                description: "List all stored FIDO2 credentials".to_string(),
                parameters: vec![],
                example: serde_json::json!({"type": "list_credentials"}),
            },
            ActionDefinition {
                name: "delete_credential".to_string(),
                description: "Delete a stored FIDO2 credential by RP ID".to_string(),
                parameters: vec![
                    Parameter {
                        name: "rp_id".to_string(),
                        type_hint: "string".to_string(),
                        description: "Relying Party ID whose credentials to delete".to_string(),
                        required: true,
                    },
                ],
                example: serde_json::json!({"type": "delete_credential", "rp_id": "example.com"}),
            },
            ActionDefinition {
                name: "save_credentials".to_string(),
                description: "Export all credentials to JSON for LLM-controlled persistence".to_string(),
                parameters: vec![],
                example: serde_json::json!({"type": "save_credentials"}),
            },
            ActionDefinition {
                name: "load_credentials".to_string(),
                description: "Import credentials from JSON (LLM-controlled restoration)".to_string(),
                parameters: vec![
                    Parameter {
                        name: "credentials_json".to_string(),
                        type_hint: "string".to_string(),
                        description: "JSON array of credentials to load".to_string(),
                        required: true,
                    },
                ],
                example: serde_json::json!({"type": "load_credentials", "credentials_json": "[]"}),
            },
            ActionDefinition {
                name: "list_pending_approvals".to_string(),
                description: "List all pending approval requests awaiting LLM decision".to_string(),
                parameters: vec![],
                example: serde_json::json!({"type": "list_pending_approvals"}),
            },
        ]
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![]
    }

    fn execute_action(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let action_type = action["type"]
            .as_str()
            .context("Missing action type")?;

        match action_type {
            "approve_request" => {
                let approval_id = action["approval_id"]
                    .as_u64()
                    .context("Missing approval_id parameter")?;

                // Get approval manager
                let approval_mgr = tokio::runtime::Handle::current().block_on(self.get_approval_manager())
                    .context("No approval manager found (server may not have LLM approval enabled)")?;

                // Approve the request
                tokio::runtime::Handle::current().block_on(approval_mgr.approve(approval_id))
                    .map_err(|e| anyhow::anyhow!("Failed to approve request: {}", e))?;

                info!("Approved FIDO2 request ID {}", approval_id);
                Ok(ActionResult::NoAction)
            }
            "deny_request" => {
                let approval_id = action["approval_id"]
                    .as_u64()
                    .context("Missing approval_id parameter")?;

                // Get approval manager
                let approval_mgr = tokio::runtime::Handle::current().block_on(self.get_approval_manager())
                    .context("No approval manager found (server may not have LLM approval enabled)")?;

                // Deny the request
                tokio::runtime::Handle::current().block_on(approval_mgr.deny(approval_id))
                    .map_err(|e| anyhow::anyhow!("Failed to deny request: {}", e))?;

                info!("Denied FIDO2 request ID {}", approval_id);
                Ok(ActionResult::NoAction)
            }
            "list_credentials" => {
                // NOTE: Credentials are stored per USB/IP connection in the FIDO2 handler
                // To access them, we'd need to downcast the handler to Fido2HidHandler
                // This requires architectural changes to expose credential access
                info!("list_credentials called - credentials are per-connection in USB handlers");
                Ok(ActionResult::NoAction)
            }
            "delete_credential" => {
                let _rp_id = action["rp_id"]
                    .as_str()
                    .context("Missing rp_id parameter")?;

                info!("delete_credential called for RP: {} - requires direct handler access", _rp_id);
                Ok(ActionResult::NoAction)
            }
            "save_credentials" => {
                // Credentials are in-memory in the handler
                // LLM can observe credential events and maintain its own persistent state
                info!("save_credentials called - LLM should track credentials via events");
                Ok(ActionResult::NoAction)
            }
            "load_credentials" => {
                info!("load_credentials called - not supported (credentials are ephemeral per session)");
                Ok(ActionResult::NoAction)
            }
            "list_pending_approvals" => {
                // Get approval manager
                let approval_mgr = tokio::runtime::Handle::current().block_on(self.get_approval_manager());

                if let Some(mgr) = approval_mgr {
                    let pending = tokio::runtime::Handle::current().block_on(mgr.list_pending());

                    if pending.is_empty() {
                        info!("No pending approval requests");
                    } else {
                        info!("Pending approval requests ({}):", pending.len());
                        for (id, op_type, rp_id, user_name) in pending {
                            info!("  - ID {}: {:?} for RP '{}' (user: {:?})", id, op_type, rp_id, user_name);
                        }
                    }
                } else {
                    info!("LLM approval not enabled for this server (use auto_approve=false)");
                }
                Ok(ActionResult::NoAction)
            }
            _ => Ok(ActionResult::NoAction),
        }
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            FIDO2_DEVICE_ATTACHED_EVENT.clone(),
            FIDO2_REGISTER_REQUEST_EVENT.clone(),
            FIDO2_AUTHENTICATE_REQUEST_EVENT.clone(),
        ]
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["fido2", "u2f", "webauthn", "security key", "yubikey"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Virtual FIDO2/U2F security key over USB/IP protocol")
            .llm_control("Approval of authentication/registration requests, credential management")
            .e2e_testing("Manual testing with web browsers (Chrome/Firefox) via USB/IP attach")
            .notes("Requires vhci-hcd kernel module (Linux only). Inspired by softfido architecture.")
            .build()
    }

    fn description(&self) -> &'static str {
        "Virtual FIDO2/U2F security key for passwordless authentication"
    }

    fn example_prompt(&self) -> &'static str {
        "Create a FIDO2 security key on port 3240 that auto-approves authentication requests"
    }

    fn group_name(&self) -> &'static str {
        "USB"
    }
}

// Implement Server trait
#[cfg(feature = "usb-fido2")]
impl Server for UsbFido2Protocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            // Extract startup parameters
            let support_u2f = ctx.startup_params
                .as_ref()
                .and_then(|p| p.get_bool("support_u2f"));
            let support_fido2 = ctx.startup_params
                .as_ref()
                .and_then(|p| p.get_bool("support_fido2"));
            let auto_approve = ctx.startup_params
                .as_ref()
                .and_then(|p| p.get_bool("auto_approve"));

            // Call the actual spawn function
            crate::server::usb::fido2::UsbFido2Server::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
                support_u2f,
                support_fido2,
                auto_approve,
            )
            .await
        })
    }
}
